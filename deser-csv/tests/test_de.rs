use std::borrow::Cow;
use std::collections::BTreeMap;

use deser::adapters::Separated;
use deser::{Deserialize, ErrorKind};
use deser_csv::{
    Deserializer, DeserializerConfig, Escape, Headers, Nulls, Terminator, Trim, from_slice,
    from_str,
};
use deser_location::Spanned;
use deser_path::{Path, PathLayer};

type Rows = Vec<Vec<String>>;

const NO_HEADERS: DeserializerConfig = DeserializerConfig::new().headers(Headers::None);

/// Parses without names into strings.
fn rows(input: &str) -> Rows {
    NO_HEADERS.from_str(input).unwrap()
}

fn rows_with(config: &DeserializerConfig, input: &str) -> Rows {
    config
        .clone()
        .headers(Headers::None)
        .from_str(input)
        .unwrap()
}

fn row(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|field| field.to_string()).collect()
}

#[derive(Debug, Deserialize, PartialEq)]
struct Person<'a> {
    name: &'a str,
    age: u32,
    email: Option<String>,
}

#[test]
fn test_repeated_names() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        name: String,
        tag: Vec<String>,
        other: Vec<u32>,
    }

    // columns with the same name are collected, a column given once is a
    // collection of one value and a missing column an empty collection
    let rows: Vec<Row> = from_str("name,tag,tag\na,x,y\n").unwrap();
    assert_eq!(
        rows,
        [Row {
            name: "a".into(),
            tag: vec!["x".into(), "y".into()],
            other: vec![],
        }]
    );
    let rows: Vec<Row> = from_str("tag,name,other\nx,a,1\n").unwrap();
    assert_eq!(rows[0].tag, ["x"]);
    assert_eq!(rows[0].other, [1]);
}

#[test]
fn test_basics() {
    let input = String::from("name,age,email\njane,42,jane@example.com\njohn,23,\n");
    let people: Vec<Person> = from_str(&input).unwrap();
    assert_eq!(
        people,
        [
            Person {
                name: "jane",
                age: 42,
                email: Some("jane@example.com".into()),
            },
            Person {
                name: "john",
                age: 23,
                email: Some("".into()),
            },
        ]
    );
    // unquoted fields are borrowed from the input
    assert!(
        input
            .as_bytes()
            .as_ptr_range()
            .contains(&people[0].name.as_ptr())
    );

    // empty input and input with only names have no records
    assert!(from_str::<Vec<Person>>("").unwrap().is_empty());
    assert!(
        from_str::<Vec<Person>>("name,age,email\n")
            .unwrap()
            .is_empty()
    );
    assert!(
        from_str::<Vec<Person>>("name,age,email")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn test_quoted_fields() {
    assert_eq!(
        rows("\"a,b\",\"c\nd\",\"e\"\"f\",\"\"\n"),
        [row(&["a,b", "c\nd", "e\"f", ""])]
    );
    // quoted line breaks are kept as they are
    assert_eq!(rows("\"a\r\nb\"\r\nc\r\n"), [row(&["a\r\nb"]), row(&["c"])]);
    // fields with doubled quotes cannot be borrowed
    let err = NO_HEADERS
        .from_str::<Vec<Vec<&str>>>("\"a\"\"b\"")
        .unwrap_err();
    assert!(
        err.message()
            .starts_with("unexpected owned string, expected a borrowed string")
    );
    let rows: Vec<Vec<Cow<str>>> = NO_HEADERS.from_str("\"a\"\"b\",c").unwrap();
    assert_eq!(rows, [["a\"b", "c"]]);
}

#[test]
fn test_line_endings() {
    let expected = [row(&["a", "b"]), row(&["c", "d"])];
    for input in [
        "a,b\nc,d",
        "a,b\nc,d\n",
        "a,b\r\nc,d\r\n",
        "a,b\rc,d\r",
        "a,b\r\nc,d\n",
        "a,b\n\n\nc,d\n\n",
        "\r\n\r\na,b\r\n\r\nc,d",
    ] {
        assert_eq!(rows(input), expected, "{:?}", input);
    }

    // the ASCII record and unit separators
    let config = DeserializerConfig::new()
        .delimiter(0x1f)
        .terminator(Terminator::Byte(0x1e));
    assert_eq!(
        rows_with(&config, "a\x1fb\nc\x1ec\x1fd"),
        [row(&["a", "b\nc"]), row(&["c", "d"])]
    );
}

#[test]
fn test_blank_lines() {
    let config = DeserializerConfig::new().skip_blank_lines(false);
    assert_eq!(
        rows_with(&config, "a\n\nb\r\n\r\n"),
        [row(&["a"]), row(&[""]), row(&["b"]), row(&[""])]
    );
    // lines with whitespace are not blank
    assert_eq!(rows("a\n \nb"), [row(&["a"]), row(&[" "]), row(&["b"])]);
}

#[test]
fn test_comments() {
    let config = DeserializerConfig::new().comment(Some(b'#')).flexible(true);
    assert_eq!(
        rows_with(&config, "# first\na,#b\n#\n  # c\n# last"),
        [row(&["a", "#b"]), row(&["  # c"])]
    );
    // comments before the names
    let rows: Vec<BTreeMap<String, String>> =
        config.from_str("# generated\nname\n# x\njane\n").unwrap();
    assert_eq!(rows[0]["name"], "jane");
}

#[test]
fn test_byte_order_mark() {
    let people: Vec<Person> = from_slice(b"\xef\xbb\xbfname,age,email\njane,42,\n").unwrap();
    assert_eq!(people[0].name, "jane");
    // only at the start
    assert_eq!(rows("a,\u{feff}b"), [row(&["a", "\u{feff}b"])]);

    let err = from_slice::<Vec<Person>>(b"\xff\xfen\x00a\x00").unwrap_err();
    assert_eq!(err.message(), "input is UTF-16, only UTF-8 is supported");
}

#[test]
fn test_names() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        a: u32,
        b: u32,
    }

    // names are decoded like fields
    let rows: Vec<Row> = from_str("\"a\",b\n1,2\n").unwrap();
    assert_eq!(rows, [Row { a: 1, b: 2 }]);

    // given names
    let config = DeserializerConfig::new().headers(Headers::Given(&["b", "a"]));
    let rows: Vec<Row> = config.from_str("1,2\n").unwrap();
    assert_eq!(rows, [Row { a: 2, b: 1 }]);

    // skipped names, records are sequences
    let config = DeserializerConfig::new().headers(Headers::Skip);
    let rows: Vec<(u32, u32)> = config.from_str("a,b\n1,2\n").unwrap();
    assert_eq!(rows, [(1, 2)]);

    let mut de = Deserializer::from_str("a,b\n1,2\n");
    assert_eq!(de.headers(), None);
    assert_eq!(
        de.deserialize_record::<Row>().unwrap(),
        Some(Row { a: 1, b: 2 })
    );
    assert_eq!(de.headers().unwrap(), ["a", "b"]);

    let err = from_slice::<Vec<Row>>(b"a,\xff\n").unwrap_err();
    assert_eq!(err.message(), "name is not valid UTF-8");
    assert_eq!((err.line(), err.column()), (Some(1), Some(3)));
}

#[test]
fn test_duplicate_names() {
    // duplicate keys are errors like in other formats
    let err = from_str::<Vec<BTreeMap<String, u32>>>("a,a\n1,2\n").unwrap_err();
    assert_eq!(err.message(), "duplicate key in map");
    assert_eq!((err.line(), err.column()), (Some(2), Some(4)));
}

#[test]
fn test_trim() {
    let config = DeserializerConfig::new().trim(Trim::Fields);
    assert_eq!(
        rows_with(&config, " a , \"b \" ,\tc\t, \" \" \n"),
        [row(&["a", "b ", "c", " "])]
    );

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        a: String,
        b: String,
    }
    let config = DeserializerConfig::new().trim(Trim::Headers);
    let rows: Vec<Row> = config.from_str(" a , b \n x , y \n").unwrap();
    assert_eq!(
        rows,
        [Row {
            a: " x ".into(),
            b: " y ".into()
        }]
    );
    let rows: Vec<Row> = config
        .trim(Trim::All)
        .from_str(" a , b \n x , y \n")
        .unwrap();
    assert_eq!(
        rows,
        [Row {
            a: "x".into(),
            b: "y".into()
        }]
    );
}

#[test]
fn test_nulls() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        text: Option<String>,
        number: Option<u32>,
    }

    let rows: Vec<Row> = from_str("text,number\n,\n\"\",1\n").unwrap();
    assert_eq!(
        rows,
        [
            Row {
                text: Some("".into()),
                number: None
            },
            Row {
                text: Some("".into()),
                number: Some(1)
            }
        ]
    );

    let config = DeserializerConfig::new().nulls(Nulls::Empty);
    let rows: Vec<Row> = config.from_str("text,number\n,\n\"\",1\n").unwrap();
    assert_eq!(
        rows,
        [
            Row {
                text: None,
                number: None
            },
            Row {
                text: Some("".into()),
                number: Some(1)
            }
        ]
    );

    let config = DeserializerConfig::new().nulls(Nulls::Text("NULL"));
    let rows: Vec<Row> = config
        .from_str("text,number\nNULL,NULL\n\"NULL\",\n")
        .unwrap();
    assert_eq!(
        rows,
        [
            Row {
                text: None,
                number: None
            },
            Row {
                text: Some("NULL".into()),
                number: None
            }
        ]
    );
}

#[test]
fn test_flexible() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        a: u32,
        #[deser(default)]
        b: u32,
        #[deser(flatten)]
        rest: BTreeMap<String, String>,
    }

    let err = from_str::<Vec<Row>>("a,b\n1\n").unwrap_err();
    assert_eq!(err.message(), "record has 1 field, expected 2");
    assert_eq!((err.line(), err.column()), (Some(2), Some(1)));
    let err = from_str::<Vec<Row>>("a,b\n1,2,3\n").unwrap_err();
    assert_eq!(err.message(), "record has 3 fields, expected 2");

    let config = DeserializerConfig::new().flexible(true);
    let rows: Vec<Row> = config.from_str("a,b\n1\n1,2,3,4\n").unwrap();
    assert_eq!(
        rows,
        [
            Row {
                a: 1,
                b: 0,
                rest: BTreeMap::new()
            },
            Row {
                a: 1,
                b: 2,
                rest: BTreeMap::from([("2".into(), "3".into()), ("3".into(), "4".into())])
            }
        ]
    );

    // without names the first record has the number of fields
    let err = NO_HEADERS.from_str::<Rows>("a,b\nc\n").unwrap_err();
    assert_eq!(err.message(), "record has 1 field, expected 2");
    assert_eq!(
        rows_with(&config, "a,b\nc\n"),
        [row(&["a", "b"]), row(&["c"])]
    );
}

#[test]
fn test_strict_quotes() {
    let err = NO_HEADERS.from_str::<Rows>("a,b\"c\n").unwrap_err();
    assert_eq!(err.message(), "unexpected quote in an unquoted field");
    assert_eq!((err.line(), err.column()), (Some(1), Some(4)));

    let err = NO_HEADERS.from_str::<Rows>("a\n\"b\"c,d\n").unwrap_err();
    assert_eq!(err.message(), "unexpected character after a quoted field");
    assert_eq!((err.line(), err.column()), (Some(2), Some(4)));

    let err = NO_HEADERS.from_str::<Rows>("a\n\"b,c\nd\n").unwrap_err();
    assert_eq!(err.message(), "unterminated quoted field");
    assert_eq!((err.line(), err.column()), (Some(2), Some(1)));

    // whitespace around quotes is only accepted when trimming
    let err = NO_HEADERS.from_str::<Rows>("a, \"b\"\n").unwrap_err();
    assert_eq!(err.message(), "unexpected quote in an unquoted field");

    let lenient = DeserializerConfig::new().lenient_quotes(true);
    assert_eq!(
        rows_with(&lenient, "5'10\",\"a\"b,\"c\"\"\"d\"\n"),
        [row(&["5'10\"", "ab", "c\"d"])]
    );
    // an unterminated quote is still an error
    assert!(
        lenient
            .headers(Headers::None)
            .from_str::<Rows>("\"a")
            .is_err()
    );
}

#[test]
fn test_record_errors_continue() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        a: u32,
    }

    let mut de = Deserializer::from_str("a\n1\nx\n\"2\"z\n3\n");
    let results: Vec<_> = de
        .records::<Row>()
        .map(|result| result.map_err(|err| (err.message().to_string(), err.line())))
        .collect();
    assert_eq!(
        results,
        [
            Ok(Row { a: 1 }),
            Err(("invalid value \"x\", expected u32".into(), Some(3))),
            Err(("unexpected character after a quoted field".into(), Some(4))),
            Ok(Row { a: 3 })
        ]
    );
    assert!(de.is_end());
}

#[test]
fn test_escapes() {
    let config = DeserializerConfig::new()
        .escape(Escape::Backslash)
        .double_quote(false);
    assert_eq!(
        rows_with(&config, "a\\,b,\"c\\\"d\",e\\tf\\\\,g\\\nh\n"),
        [row(&["a,b", "c\"d", "e\tf\\", "g\nh"])]
    );

    let config = DeserializerConfig::new().escape(Escape::Char(b'!'));
    assert_eq!(
        rows_with(&config, "a!,b,\"c!\"d\",e!tf\n"),
        [row(&["a,b", "c\"d", "etf"])]
    );

    // an escape at the end of the input
    let err = config
        .headers(Headers::None)
        .from_str::<Rows>("a!")
        .unwrap_err();
    assert_eq!(err.message(), "escape character at the end of the input");
}

#[test]
fn test_tsv() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row<'a> {
        name: Cow<'a, str>,
        note: Option<Cow<'a, str>>,
    }

    let rows: Vec<Row> = DeserializerConfig::tsv()
        .from_str("name\tnote\na\"b\tx\\ty\\nz\nc\t\\N\nd\t\\\\N\n")
        .unwrap();
    assert_eq!(
        rows,
        [
            Row {
                name: "a\"b".into(),
                note: Some("x\ty\nz".into())
            },
            Row {
                name: "c".into(),
                note: None
            },
            Row {
                name: "d".into(),
                note: Some("\\N".into())
            },
        ]
    );
}

#[test]
fn test_sep_line() {
    let config = DeserializerConfig::new().sep_line(true);
    for input in ["sep=;\na;b\n", "SEP=;\r\na;b", "\u{feff}sep=;\na;b"] {
        assert_eq!(rows_with(&config, input), [row(&["a", "b"])], "{:?}", input);
    }
    // other lines are records
    assert_eq!(rows_with(&config, "sep=;;\n"), [row(&["sep=;;"])]);
    assert_eq!(rows_with(&config, "sep"), [row(&["sep"])]);
    // without the option it's a record
    assert_eq!(rows("sep=;\na;b"), [row(&["sep=;"]), row(&["a;b"])]);
}

#[test]
fn test_invalid_config() {
    let err = DeserializerConfig::new()
        .quote(Some(b','))
        .from_str::<Rows>("a")
        .unwrap_err();
    assert_eq!(
        err.message(),
        "the quote character conflicts with another special character"
    );
}

#[test]
fn test_bytes() {
    #[derive(Debug, Deserialize)]
    struct Row {
        data: Vec<u8>,
    }

    // fields that are not UTF-8 are bytes
    let rows: Vec<Row> = from_slice(b"data\nhello_\xff\n\"\xfe\"\"\"\n").unwrap();
    assert_eq!(rows[0].data, b"hello_\xff");
    assert_eq!(rows[1].data, b"\xfe\"");
    // other fields are base64
    let rows: Vec<Row> = from_str("data\naGk=\n").unwrap();
    assert_eq!(rows[0].data, b"hi");

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Text {
        data: String,
    }
    let err = from_slice::<Vec<Text>>(b"data\n\xff\n").unwrap_err();
    assert_eq!(err.message(), "unexpected bytes, expected string");
}

#[test]
fn test_lexical() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "kind", rename_all = "lowercase")]
    enum Event {
        Click { x: i32, y: i32 },
        Key { code: u32, shift: bool },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        id: u64,
        #[deser(flatten)]
        event: Event,
    }

    let config = DeserializerConfig::new().nulls(Nulls::Empty);
    let rows: Vec<Row> = config
        .from_str("id,kind,x,y,code,shift\n1,click,-3,4,,\n2,key,,,13,yes\n")
        .unwrap();
    assert_eq!(
        rows,
        [
            Row {
                id: 1,
                event: Event::Click { x: -3, y: 4 }
            },
            Row {
                id: 2,
                event: Event::Key {
                    code: 13,
                    shift: true
                }
            },
        ]
    );
}

#[test]
fn test_separated() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        name: String,
        #[deser(as = Separated<';'>)]
        tags: Vec<String>,
        #[deser(as = Separated<' '>)]
        scores: Vec<u32>,
    }

    let rows: Vec<Row> = from_str("name,tags,scores\na,x;y,1 2 3\n").unwrap();
    assert_eq!(
        rows,
        [Row {
            name: "a".into(),
            tags: vec!["x".into(), "y".into()],
            scores: vec![1, 2, 3],
        }]
    );
}

#[test]
fn test_errors_with_paths() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    #[deser(deny_unknown_fields)]
    struct Row {
        name: String,
        age: u32,
    }

    let input = "name,age\njane,42\n\"john\nsmith\",x\n";
    let err = Deserializer::from_str(input)
        .deserialize_with::<Vec<Row>, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.message(), "invalid value \"x\", expected u32");
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "[1].age");
    assert_eq!((err.line(), err.column()), (Some(4), Some(8)));

    let err = from_str::<Vec<Row>>("name,age,agee\njane,42,1\n").unwrap_err();
    assert_eq!(
        err.message(),
        "unknown field `agee`, expected `name` or `age`"
    );
    assert_eq!((err.line(), err.column()), (Some(2), Some(9)));

    let err = from_str::<Vec<Row>>("name\njane\n").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
}

#[test]
fn test_locations() {
    #[derive(Debug, Deserialize)]
    struct Row {
        name: Spanned<String>,
        age: Spanned<u32>,
    }

    let config = DeserializerConfig::new().track_locations(true);
    let rows: Vec<Row> = config
        .from_str("name,age\njane,42\n\"jo\nhn\",23\n")
        .unwrap();
    assert_eq!(rows[0].age.span.unwrap().to_string(), "2:6-2:8");
    assert_eq!(rows[1].name.span.unwrap().to_string(), "3:1-4:4");
    assert_eq!(rows[1].age.span.unwrap().to_string(), "4:5-4:7");
}

#[test]
fn test_maps_and_sequences() {
    let rows: Vec<BTreeMap<String, Option<u32>>> = from_str("a,b\n1,\n").unwrap();
    assert_eq!(
        rows[0],
        BTreeMap::from([("a".into(), Some(1)), ("b".into(), None)])
    );

    // records can be deserialized one at a time into different types
    let mut de = Deserializer::from_str_with_config("1,2\na,b,c\n", &NO_HEADERS.flexible(true));
    assert_eq!(de.deserialize_record::<[u32; 2]>().unwrap(), Some([1, 2]));
    assert_eq!(
        de.deserialize_record::<(char, String, Cow<str>)>().unwrap(),
        Some(('a', "b".into(), "c".into()))
    );
    assert_eq!(de.deserialize_record::<Vec<u32>>().unwrap(), None);
}
