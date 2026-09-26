use std::collections::BTreeMap;

use deser::{Deserialize, ErrorKind, Serialize};
use deser_toml::{Datetime, from_str, to_string};

mod common;

use common::Value;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Config {
    name: String,
    port: u16,
    ratio: f32,
    owner: Option<String>,
    tags: Vec<String>,
    created: Datetime,
    database: Database,
    servers: Vec<Server>,
    mode: Mode,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Database {
    url: String,
    pool: BTreeMap<String, u32>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Server {
    host: String,
    weight: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Mode {
    Fast,
    Slow,
}

#[test]
fn test_struct() {
    let config = Config {
        name: "web".into(),
        port: 8080,
        ratio: 0.1,
        owner: None,
        tags: vec!["a".into(), "b".into()],
        created: "1979-05-27T07:32:00Z".parse().unwrap(),
        database: Database {
            url: "postgres://localhost".into(),
            pool: [("min".into(), 1), ("max".into(), 10)].into(),
        },
        servers: vec![
            Server {
                host: "alpha".into(),
                weight: Some(1),
            },
            Server {
                host: "beta".into(),
                weight: None,
            },
        ],
        mode: Mode::Fast,
    };
    let toml = to_string(&config).unwrap();
    assert_eq!(
        toml,
        r#"name = "web"
port = 8080
ratio = 0.1
tags = ["a", "b"]
created = 1979-05-27T07:32:00Z
mode = "Fast"

[database]
url = "postgres://localhost"

[database.pool]
max = 10
min = 1

[[servers]]
host = "alpha"
weight = 1

[[servers]]
host = "beta"
"#
    );
    assert_eq!(from_str::<Config>(&toml).unwrap(), config);
}

fn roundtrip(value: &Value) -> String {
    let toml = to_string(value).unwrap();
    let rv: Value = match from_str(&toml) {
        Ok(rv) => rv,
        Err(err) => panic!("cannot parse {:?}: {}", toml, err),
    };
    assert_eq!(&rv, value, "roundtrip through {:?}", toml);
    toml
}

#[test]
fn test_strings() {
    let value = table! {
        "plain" => "hello",
        "quotes" => "say \"hi\"",
        "path" => r"C:\Users",
        "both" => "it's \"x\"",
        "multi" => "a\nb\n",
        "multi_quotes" => "\"\"\"\"\n\"\"",
        "multi_leading_newline" => "\nx",
        "multi_backslash" => "a\\\nb\\",
        "crlf" => "a\r\nb",
        "control" => "\0\x08\x0c\x1b\x7f\t",
        "unicode" => "äöü 😀",
        "empty" => "",
    };
    let toml = roundtrip(&value);
    assert_eq!(
        toml,
        r#"plain = "hello"
quotes = 'say "hi"'
path = 'C:\Users'
both = "it's \"x\""
multi = """
a
b
"""
multi_quotes = """
""\""
"""""
multi_leading_newline = """

x"""
multi_backslash = """
a\\
b\\"""
crlf = """
a\r
b"""
control = "\u0000\b\f\u001B\u007F\t"
unicode = "äöü 😀"
empty = ""
"#
    );
}

#[test]
fn test_keys() {
    let value = table! {
        "bare-key_1" => 1,
        "with space" => 2,
        "" => 3,
        "dotted.key" => 4,
        "quote\"" => 5,
        "ä" => 6,
        "nested key" => table! {"inner key" => table! {"x" => 1}},
    };
    let toml = roundtrip(&value);
    assert_eq!(
        toml,
        r#"bare-key_1 = 1
"with space" = 2
"" = 3
"dotted.key" = 4
"quote\"" = 5
"ä" = 6

["nested key"."inner key"]
x = 1
"#
    );

    // non string keys are converted
    let mut map = BTreeMap::new();
    map.insert(1u32, true);
    map.insert(2u32, false);
    assert_eq!(to_string(&map).unwrap(), "1 = true\n2 = false\n");
    let mut map = BTreeMap::new();
    map.insert('c', 1);
    assert_eq!(to_string(&map).unwrap(), "c = 1\n");
}

#[test]
fn test_numbers() {
    let value = table! {
        "int" => 42,
        "neg" => -42,
        "float" => 1.0,
        "big" => 1e100,
        "small" => 1.5e-7,
        "negzero" => -0.0,
        "inf" => f64::INFINITY,
        "neginf" => f64::NEG_INFINITY,
    };
    let toml = roundtrip(&value);
    assert_eq!(
        toml,
        "int = 42\nneg = -42\nfloat = 1.0\nbig = 1e100\nsmall = 1.5e-7\nnegzero = -0.0\n\
         inf = inf\nneginf = -inf\n"
    );

    let mut map = BTreeMap::new();
    map.insert("nan", f64::NAN);
    assert_eq!(to_string(&map).unwrap(), "nan = nan\n");
    let mut map = BTreeMap::new();
    map.insert("a", 0.1f32);
    assert_eq!(to_string(&map).unwrap(), "a = 0.1\n");
    let mut map = BTreeMap::new();
    map.insert("a", u64::MAX);
    assert_eq!(to_string(&map).unwrap(), "a = 18446744073709551615\n");
    let mut map = BTreeMap::new();
    map.insert("a", i128::MIN);
    assert_eq!(to_string(&map).unwrap_err().kind(), ErrorKind::OutOfRange);
    let mut map = BTreeMap::new();
    map.insert("a", -1i128);
    assert_eq!(to_string(&map).unwrap(), "a = -1\n");
}

#[test]
fn test_tables() {
    let value = table! {
        "a" => 1,
        "empty" => table! {},
        "only_tables" => table! {"x" => table! {"y" => 1}, "z" => table! {}},
        "b" => 2,
        "aot" => array![table! {"x" => 1, "sub" => table! {"y" => 2}}, table! {}],
        "mixed" => array![1, table! {"x" => 1}],
        "nested_inline" => array![array![table! {"x" => table! {"y" => array![]}}]],
        "empty_array" => array![],
    };
    let toml = roundtrip(&value);
    assert_eq!(
        toml,
        r#"a = 1
b = 2
mixed = [1, { x = 1 }]
nested_inline = [[{ x = { y = [] } }]]
empty_array = []

[empty]

[only_tables.x]
y = 1

[only_tables.z]

[[aot]]
x = 1

[aot.sub]
y = 2

[[aot]]
"#
    );
}

#[test]
fn test_datetimes() {
    let value = table! {
        "odt" => "1979-05-27T00:32:00.999-07:00".parse::<Datetime>().unwrap(),
        "ldt" => "1979-05-27T07:32".parse::<Datetime>().unwrap(),
        "ld" => "1979-05-27".parse::<Datetime>().unwrap(),
        "lt" => "07:32:00".parse::<Datetime>().unwrap(),
    };
    let toml = roundtrip(&value);
    assert_eq!(
        toml,
        "odt = 1979-05-27T00:32:00.999-07:00\nldt = 1979-05-27T07:32:00\n\
         ld = 1979-05-27\nlt = 07:32:00\n"
    );

    let invalid = Datetime {
        date: None,
        time: None,
        offset: None,
    };
    let mut map = BTreeMap::new();
    map.insert("a", invalid);
    assert!(to_string(&map).is_err());
}

#[test]
fn test_nulls() {
    let mut map = BTreeMap::new();
    map.insert("a", Some(1));
    map.insert("b", None);
    assert_eq!(to_string(&map).unwrap(), "a = 1\n");

    let mut map = BTreeMap::new();
    map.insert("a", vec![Some(1), None]);
    let err = to_string(&map).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
}

#[test]
fn test_unsupported() {
    // the document has to be a table
    assert_eq!(
        to_string(&vec![1, 2]).unwrap_err().kind(),
        ErrorKind::UnsupportedType
    );
    assert_eq!(
        to_string(&42).unwrap_err().kind(),
        ErrorKind::UnsupportedType
    );
    assert_eq!(
        to_string(&None::<u32>).unwrap_err().kind(),
        ErrorKind::UnsupportedType
    );

    let mut map = BTreeMap::new();
    map.insert(vec![1u32], 1);
    assert_eq!(
        to_string(&map).unwrap_err().kind(),
        ErrorKind::UnsupportedType
    );
}

#[test]
fn test_deep_nesting() {
    let depth = if cfg!(miri) { 100 } else { 5000 };
    let mut value = Value::Int(1);
    for idx in 0..depth {
        value = if idx % 2 == 0 {
            array![value]
        } else {
            table! {"a" => value}
        };
    }
    let value = table! {"a" => value};
    let toml = to_string(&value).unwrap();
    // compared by serializing again as comparing recurses
    let rv: Value = from_str(&toml).unwrap();
    assert_eq!(to_string(&rv).unwrap(), toml);
    drop_value(rv);
    drop_value(value);

    // deeply nested tables are written as sections
    let mut value = Value::Int(1);
    for _ in 0..depth {
        value = table! {"a" => value};
    }
    let toml = to_string(&value).unwrap();
    assert!(toml.starts_with("[a.a.a."));
    let rv: Value = from_str(&toml).unwrap();
    assert_eq!(to_string(&rv).unwrap(), toml);
    drop_value(rv);
    drop_value(value);
}

/// Drops a value without recursion.
fn drop_value(value: Value) {
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Array(items) => stack.extend(items),
            Value::Table(items) => stack.extend(items.into_iter().map(|x| x.1)),
            _ => {}
        }
    }
}
