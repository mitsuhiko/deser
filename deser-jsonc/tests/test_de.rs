use std::collections::BTreeMap;

use deser::Deserialize;
use deser_jsonc::{Deserializer, DeserializerConfig, Trailing, from_slice, from_str};
use deser_location::Spanned;

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
    assert_eq!(fails("[1 / 2]"), "expected a comma at 1:4");
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

    let doc: Doc = DeserializerConfig::new()
        .track_locations(true)
        .from_str("{\n  // a\n  \"a\": /* one */ 1,\n  \"b\": [true, /**/ false,],\n}")
        .unwrap();
    let span = |s: Option<deser_location::Span>| format!("{:?}", s.unwrap());
    assert_eq!(span(doc.a.span), "3:18-3:19");
    assert_eq!(span(doc.b.span), "4:8-4:27");
    assert_eq!(span(doc.b.value[1].span), "4:20-4:25");
}

#[test]
fn test_values_separated_by_comments() {
    let config = DeserializerConfig::new().trailing(Trailing::Stop);
    let mut de = Deserializer::from_str_with_config("1 /* a */ 2 // b\n 3 // end", &config);
    let values = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2, 3]);
    assert!(de.is_end());
    de.end().unwrap();

    let config = DeserializerConfig::new().trailing(Trailing::Newline);
    let mut de = Deserializer::from_str_with_config("// header\n1 // one\n\n// x\n2\n", &config);
    let values = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2]);

    let de = Deserializer::from_str("/* unterminated");
    assert!(!de.is_end());
    assert!(de.end().is_err());
}
