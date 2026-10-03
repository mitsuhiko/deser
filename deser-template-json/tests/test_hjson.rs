//! The syntax of Hjson other than comments and trailing commas.
use std::collections::BTreeMap;

use deser::Deserialize;
use deser::ext::Decimal;
use deser_location::Spanned;

use super::common::{NEWLINE, STOP, STRICT, check_stream, tracked};
use super::dialect;
use dialect::{DeserializerConfig, from_slice, from_str};

/// The example from <https://hjson.github.io/>.
const EXAMPLE: &str = r#"{
  // use #, // or /**/ comments,
  // omit quotes for keys
  key: 1
  // omit quotes for strings
  contains: everything on this line
  // omit commas at the end of a line
  cool: {
    foo: 1
    bar: 2
  }
  // allow trailing commas
  list: [
    1,
    2,
  ]
  // and use multiline strings
  realist:
    '''
    My half empty glass,
    I will fill your empty half.
    Now you are half full.
    '''
}"#;

#[test]
fn test_example() {
    #[derive(Deserialize, Debug)]
    struct Example<'a> {
        key: u32,
        contains: &'a str,
        cool: BTreeMap<&'a str, u16>,
        list: Vec<usize>,
        realist: String,
        missing: Option<f64>,
    }

    let example: Example = from_str(EXAMPLE).unwrap();
    assert_eq!(example.key, 1);
    assert_eq!(example.contains, "everything on this line");
    assert_eq!(
        example.cool.into_iter().collect::<Vec<_>>(),
        [("bar", 2), ("foo", 1)]
    );
    assert_eq!(example.list, [1, 2]);
    assert_eq!(
        example.realist,
        "My half empty glass,\nI will fill your empty half.\nNow you are half full."
    );
    assert_eq!(example.missing, None);

    // the same from bytes and without the braces at the root
    let inner = &EXAMPLE[1..EXAMPLE.len() - 1];
    let example: Example = from_slice(inner.as_bytes()).unwrap();
    assert_eq!(example.key, 1);
    assert_eq!(example.list, [1, 2]);
}

#[test]
fn test_implicit_values() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Config {
        port: u16,
        name: String,
        version: String,
        enabled: bool,
        label: String,
        backup: Option<String>,
        ratio: f64,
    }

    // numbers, booleans and null without quotes are values whose type is
    // inferred from their text, a string receives the text
    let config: Config = from_str(
        "port: 8080\nname: 8080\nversion: 1.0\nenabled: true\n\
         label: true\nbackup: null\nratio: 0.5",
    )
    .unwrap();
    assert_eq!(
        config,
        Config {
            port: 8080,
            name: "8080".into(),
            version: "1.0".into(),
            enabled: true,
            label: "true".into(),
            backup: None,
            ratio: 0.5,
        }
    );
    assert_eq!(from_str::<String>("null").unwrap(), "null");
    assert_eq!(from_str::<Option<String>>("null").unwrap(), None);
    assert_eq!(
        from_str::<Option<String>>("'null'").unwrap().unwrap(),
        "null"
    );

    // with quotes they are strings
    assert!(from_str::<u16>(r#""8080""#).is_err());
    assert!(from_str::<bool>("'true'").is_err());
}

#[test]
fn test_numbers() {
    // numbers without quotes are numbers only if nothing but whitespace,
    // a comma, the end of a container or a comment follows them
    let values: Vec<String> = from_str("[\n5 minutes\n5 # minutes\n5, 6\n5.\n01\n-\n]").unwrap();
    assert_eq!(values, ["5 minutes", "5", "5", "6", "5.", "01", "-"]);
    let values: Vec<f64> = from_str("[1, -2.5e3 # c\n 3.25]").unwrap();
    assert_eq!(values, [1.0, -2500.0, 3.25]);
    assert!(from_str::<Vec<u32>>("[\n5 minutes\n]").is_err());

    // integers beyond 64 bits and exact numbers work like in JSON
    assert_eq!(
        from_str::<u128>("18446744073709551616").unwrap(),
        1u128 << 64
    );
    let value: Decimal = from_str("0.10000000000000000001").unwrap();
    assert_eq!(value.as_str(), "0.10000000000000000001");
    const INEXACT: DeserializerConfig = DeserializerConfig::builder().exact_numbers(false).build();
    let value: String = INEXACT.from_str("0.10").unwrap();
    assert_eq!(value, "0.10");
}

#[test]
fn test_strings() {
    let values: Vec<String> = from_str(
        r#"[
            no quotes, # // /* are part of it   
            'single "quotes"'
            "double 'quotes'"
            'escapes \' \" \n'
            '''
            multiline
              indented
            '''
        ]"#,
    )
    .unwrap();
    assert_eq!(
        values,
        [
            "no quotes, # // /* are part of it",
            r#"single "quotes""#,
            "double 'quotes'",
            "escapes ' \" \n",
            "multiline\n  indented",
        ]
    );

    // strings without quotes and keys are borrowed
    let map: BTreeMap<&str, &str> = from_str("ab: c\n'd': e f  \n").unwrap();
    assert_eq!(map["ab"], "c");
    assert_eq!(map["d"], "e f");

    // they cannot start with a punctuator
    for input in ["x: ]", "x: }", "x: ,", "x: :", "[\n:x\n]"] {
        assert!(
            from_str::<BTreeMap<String, String>>(input).is_err(),
            "{input}"
        );
    }
}

#[test]
fn test_root() {
    // a map without braces
    let map: BTreeMap<String, u32> = from_str("# c\na: 1, b: 2\n'c' : 3").unwrap();
    assert_eq!(map.len(), 3);
    // a single value otherwise
    assert_eq!(from_str::<String>("a b: c").unwrap(), "a b: c");
    assert_eq!(from_str::<String>("'a'").unwrap(), "a");
    assert_eq!(from_str::<u32>("1 # c").unwrap(), 1);
    assert_eq!(from_str::<Vec<u32>>("[1]").unwrap(), [1]);
    // which must be the only one
    assert!(from_str::<String>("'a' 'b'").is_err());
    // the map ends at the end of the input
    assert!(from_str::<BTreeMap<String, u32>>("a: 1\n}").is_err());
}

#[test]
fn test_streams() {
    // multiline strings are indented relative to the column of their
    // quotes, which is tracked when the input arrives in chunks
    let input = "a: x\n  b:  '''\n       x\n        y\n      '''\nc: [\n  '''\n    z\n  '''\n]\n";
    check_stream(&STRICT, input, 1);
    #[derive(Deserialize)]
    struct Doc {
        a: String,
        b: String,
        c: Vec<String>,
    }
    let doc: Doc = from_str(input).unwrap();
    assert_eq!(doc.a, "x");
    assert_eq!(doc.b, " x\n  y");
    assert_eq!(doc.c, ["  z"]);

    // numbers and literals end at the end of the line
    check_stream(&STOP, "1\n'x' true\n[1, 2] {a: b\n}\n3", 6);
    check_stream(&NEWLINE, "a: 1, b: x y\n\n[1, 2]\n'''x'''", 3);
    check_stream(&NEWLINE, "# c\n1 # 2\n", 1);
}

#[test]
fn test_locations() {
    #[derive(Deserialize)]
    struct Doc {
        a: Spanned<u32>,
        b: Spanned<String>,
        c: Spanned<String>,
    }

    let doc: Doc = tracked("a: 1 # c\nb: two  words  \nc:\n  '''\n  x\n  '''", STRICT)
        .deserialize()
        .unwrap();
    let span = |span: Option<deser_location::Span>| format!("{:?}", span.unwrap());
    assert_eq!(span(doc.a.span), "1:4-1:5");
    assert_eq!(span(doc.b.span), "2:4-2:14");
    assert_eq!(span(doc.c.span), "4:3-6:6");
}
