//! Comments and trailing commas (JSONC, JSON5 and Hjson).
use std::collections::BTreeMap;

use deser::Deserialize;
use deser::de::Recording;
use deser_location::Spanned;

use super::common::{Chunked, NEWLINE, STOP, STRICT, check_stream, chunk_sizes, tracked};
use super::{DIALECT, dialect};
use dialect::{Deserializer, DeserializerConfig, Trailing, from_slice, from_str};

#[test]
fn test_comments_and_trailing_commas() {
    #[derive(Deserialize, Debug)]
    struct Config<'a> {
        name: &'a str,
        ports: Vec<u16>,
        env: BTreeMap<String, String>,
    }

    let config: Config = from_str(
        r#"// leading comment
        {
            /* the name */ "name": "api", // borrowed
            "ports": [
                80, /* http */
                443, // https
            ],
            "env": {"A": "1", /* "B": "2", */},
        }
        // trailing comment"#,
    )
    .unwrap();
    assert_eq!(config.name, "api");
    assert_eq!(config.ports, [80, 443]);
    assert_eq!(config.env.len(), 1);
}

#[test]
fn test_json_is_jsonc() {
    let value: Vec<String> = from_str(r#"["/* not a comment */", "// neither"]"#).unwrap();
    assert_eq!(value, ["/* not a comment */", "// neither"]);
}

#[test]
fn test_errors() {
    fn fails(input: &str) -> String {
        let err = from_str::<Vec<u32>>(input).unwrap_err();
        format!(
            "{} at {}:{}",
            err.message(),
            err.line().unwrap(),
            err.column().unwrap()
        )
    }

    assert_eq!(
        fails("[1, /* unterminated"),
        "unexpected end of file at 1:5"
    );
    assert_eq!(fails("[1] /* unterminated"), "garbage after input at 1:5");
    if DIALECT.hjson {
        // a string without quotes
        assert_eq!(fails("[1 / 2]"), "unexpected string, expected u32 at 1:2");
    } else {
        assert_eq!(fails("[1 / 2]"), "expected a comma at 1:4");
    }
    assert_eq!(fails("[1, 2,,]"), "unexpected comma at 1:7");
    assert_eq!(fails("[,]"), "unexpected comma at 1:2");
    // comments are not whitespace within tokens
    assert!(from_str::<i32>("-/**/1").is_err());
    assert!(from_str::<bool>("tr/**/ue").is_err());
    // comments in byte slices are validated as UTF-8
    assert!(from_slice::<Vec<u32>>("[1, /* ä */ 2]".as_bytes()).is_ok());
    assert!(from_slice::<Vec<u32>>(b"[1, /* \xff */ 2]").is_err());
    assert!(from_slice::<Vec<u32>>(b"[1] // \xff").is_err());
}

#[test]
fn test_spans() {
    #[derive(Deserialize)]
    struct Doc {
        a: Spanned<u32>,
        b: Spanned<Vec<Spanned<bool>>>,
    }

    let doc: Doc = tracked(
        "{\n  // a\n  \"a\": /* one */ 1,\n  \"b\": [true, /**/ false,],\n}",
        STRICT,
    )
    .deserialize()
    .unwrap();
    let span = |s: Option<deser_location::Span>| format!("{:?}", s.unwrap());
    assert_eq!(span(doc.a.span), "3:18-3:19");
    assert_eq!(span(doc.b.span), "4:8-4:27");
    assert_eq!(span(doc.b.value[1].span), "4:20-4:25");
}

#[test]
fn test_values_separated_by_comments() {
    let config = DeserializerConfig::builder()
        .trailing(Trailing::Stop)
        .build();
    let mut de = Deserializer::from_str_with_config("1 /* a */ 2 // b\n 3 // end", config);
    let values = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2, 3]);
    assert!(de.is_end());
    de.end().unwrap();

    let config = DeserializerConfig::builder()
        .trailing(Trailing::Newline)
        .build();
    let mut de = Deserializer::from_str_with_config("// header\n1 // one\n\n// x\n2\n", config);
    let values = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2]);

    let de = Deserializer::from_str("/* unterminated");
    assert!(!de.is_end());
    assert!(de.end().is_err());
}

#[test]
fn test_stop() {
    check_stream(
        &STOP,
        r#"// values
        1 /* ] */ [1, /* ] */ 2,] {"a": "/*", /* } */ "b": [],} // "
        "x"/**/3 // end"#,
        5,
    );
}

#[test]
fn test_strict() {
    check_stream(&STRICT, "/* a */ [1, // b\n 2,] // c", 1);
    check_stream(&STRICT, "// only a comment", 0);
}

#[test]
fn test_end_after_comments() {
    // comments after the value can be split across reads
    let input = b"[1] /* a */ // b\n/* c */";
    for size in chunk_sizes(input.len()) {
        let mut reader = STRICT.reader(Chunked { input, size });
        reader.read::<Vec<u32>>().unwrap();
        reader.end().unwrap();
    }
    let input = b"[1] /* unterminated";
    for size in chunk_sizes(input.len()) {
        let mut reader = STRICT.reader(Chunked { input, size });
        reader.read::<Vec<u32>>().unwrap();
        assert!(reader.end().is_err(), "size {size}");
    }
}

#[test]
fn test_newline() {
    check_stream(
        &NEWLINE,
        "// header\n[1, 2,] // a\n\n{\"a\": 1} /* b */\n",
        2,
    );
    // line breaks in comments do not end the line, strings can contain
    // what looks like a comment.  In Hjson every line break ends the line
    // as strings without quotes can contain what looks like a comment.
    if DIALECT.hjson {
        return;
    }
    check_stream(
        &NEWLINE,
        "/* a\n b */ [1, /* c\n */ 2]\n\"/*\"\n{\"a\": \"*/\"} // d\n/* e\n*/\n3",
        4,
    );
}

#[test]
fn test_unterminated_comment() {
    for config in [&STRICT, &STOP] {
        for size in 1..=8 {
            let mut reader = config.reader(Chunked {
                input: b"[1] /* x",
                size,
            });
            let rv = (|| {
                while reader.read::<Recording>()?.is_some() {}
                Ok::<_, deser::Error>(())
            })();
            assert!(rv.is_err(), "size {size}");
        }
    }
}
