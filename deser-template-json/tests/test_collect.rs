//! Tests for collecting errors (see `State::set_collect_errors`).
use super::{DIALECT, dialect};
use std::collections::BTreeMap;

use deser::{Deserialize, Error};

/// Deserializes with errors collected.
fn collect<'de, T: Deserialize<'de>>(json: &'de str) -> Result<T, Error> {
    dialect::Deserializer::from_str(json).deserialize_with(|driver| {
        driver.state_mut().set_collect_errors(true);
    })
}

/// Returns the errors, one per line.
fn errors<T: std::fmt::Debug>(rv: Result<T, Error>) -> Vec<String> {
    rv.unwrap_err()
        .errors()
        .map(|err| err.to_string())
        .collect()
}

/// Returns the expected errors of the dialect.
///
/// In Hjson numbers are implicit values, strings receive their text.
fn expected_errors(errors: &[&str]) -> Vec<String> {
    errors
        .iter()
        .filter(|err| !(DIALECT.hjson && err.contains("integer, expected string")))
        .map(|err| err.to_string())
        .collect()
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct Server {
    host: String,
    port: u16,
    #[deser(default)]
    tags: Vec<String>,
    backup: Option<Box<Server>>,
}

#[test]
fn test_collect_errors() {
    let rv = collect::<Vec<Server>>(
        r#"[
  {"host": 1, "port": "x", "tags": ["a", 2, "b", 3]},
  {"host": "b", "extra": {"a": [1]}, "port": 80},
  {"tags": []},
  {"host": "d", "port": 1, "backup": {"host": "e", "port": -1}}
]"#,
    );
    assert_eq!(
        errors(rv),
        expected_errors(&[
            "InvalidType: unexpected unsigned integer, expected string at line 2 column 12",
            "InvalidType: unexpected string, expected u16 at line 2 column 23",
            "InvalidType: unexpected unsigned integer, expected string at line 2 column 42",
            "InvalidType: unexpected unsigned integer, expected string at line 2 column 50",
            "UnknownField: unknown field `extra`, expected one of `host`, `port`, `tags`, `backup` at line 3 column 17",
            "MissingField: missing field `host` at line 4 column 14",
            "MissingField: missing field `port` at line 4 column 14",
            "OutOfRange: invalid value -1, expected u16 at line 5 column 60",
        ])
    );
}

#[test]
fn test_collect_errors_display() {
    let err = collect::<Vec<u32>>(r#"[1, "a", 2, "b"]"#).unwrap_err();
    assert_eq!(err.errors().count(), 2);
    assert_eq!(
        err.to_string(),
        "InvalidType: unexpected string, expected u32 at line 1 column 5 (and 1 more error)"
    );
    assert_eq!(
        format!("{:#}", err),
        "InvalidType: unexpected string, expected u32 at line 1 column 5\n\
         InvalidType: unexpected string, expected u32 at line 1 column 13"
    );
}

#[test]
fn test_collect_without_errors() {
    let servers = collect::<Vec<Server>>(r#"[{"host": "a", "port": 1}]"#).unwrap();
    assert_eq!(servers[0].host, "a");
}

#[test]
fn test_collect_is_off_by_default() {
    let err = dialect::from_str::<Vec<u32>>(r#"[1, "a", 2, "b"]"#).unwrap_err();
    assert_eq!(err.errors().count(), 1);
}

#[test]
fn test_collect_maps_and_sets() {
    // the entry of `a` failed, the second one is not a duplicate
    let rv = collect::<BTreeMap<String, std::collections::BTreeSet<u32>>>(
        r#"{"a": [1, "x"], "b": 2, "a": [3]}"#,
    );
    assert_eq!(
        errors(rv),
        [
            "InvalidType: unexpected string, expected u32 at line 1 column 11",
            "InvalidType: unexpected unsigned integer, expected BTreeSet at line 1 column 22",
        ]
    );
}

#[test]
fn test_collect_duplicate_keys_of_the_last_entry() {
    // the duplicate is only known once the map ends
    let rv = collect::<BTreeMap<String, u32>>(r#"{"a": 1, "b": "x", "a": 2}"#);
    assert_eq!(
        errors(rv),
        [
            "InvalidType: unexpected string, expected u32 at line 1 column 15",
            "DuplicateKey: duplicate key in map at line 1 column 26",
        ]
    );
}

#[test]
fn test_collect_duplicate_fields() {
    // duplicate keys are rejected by default
    let input = r#"{"host": "a", "host": "b", "port": 1, "port": 2}"#;
    assert_eq!(
        dialect::from_str::<Server>(input)
            .unwrap_err()
            .errors()
            .count(),
        1
    );
    assert_eq!(
        errors(collect::<Server>(input)),
        [
            "DuplicateKey: duplicate field `host` at line 1 column 23",
            "DuplicateKey: duplicate field `port` at line 1 column 47",
        ]
    );
}

#[test]
fn test_max_errors() {
    let rv = dialect::Deserializer::from_str(r#"[["a", "b"], ["c"], ["d"]]"#)
        .deserialize_with::<Vec<Vec<u32>>, _>(|driver| {
            driver.state_mut().set_collect_errors(true);
            driver.state_mut().set_max_errors(2);
        });
    // the third error ends the deserialization, the errors collected so
    // far are kept
    assert_eq!(
        errors(rv),
        [
            "InvalidType: unexpected string, expected u32 at line 1 column 3",
            "InvalidType: unexpected string, expected u32 at line 1 column 8",
            "InvalidType: unexpected string, expected u32 at line 1 column 15",
        ]
    );
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(untagged)]
enum Value {
    Number(u32),
    Point { x: u32, y: u32 },
}

#[test]
fn test_collect_untagged() {
    // variants are tried strictly, errors in them are not collected.  The
    // error of a map is at its end.
    let rv = collect::<Vec<Value>>(r#"[1, {"x": 1, "y": 2}, {"x": 1, "y": "a"}, "b"]"#);
    assert_eq!(
        errors(rv),
        [
            "UnknownVariant: data did not match any variant of Value at line 1 column 40",
            "UnknownVariant: data did not match any variant of Value at line 1 column 43",
        ]
    );
    let values = collect::<Vec<Value>>(r#"[1, {"x": 1, "y": 2}]"#).unwrap();
    assert_eq!(values, [Value::Number(1), Value::Point { x: 1, y: 2 }]);
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "type")]
enum Shape {
    #[deser(rename = "circle")]
    Circle { radius: u32 },
    #[deser(rename = "rect")]
    Rect { width: u32, height: u32 },
}

#[test]
fn test_collect_tagged() {
    // the content before the tag is recorded and replayed
    let rv = collect::<Vec<Shape>>(
        r#"[{"radius": "x", "type": "circle"}, {"type": "rect", "width": "y"}, {"type": "x"}]"#,
    );
    assert_eq!(
        errors(rv),
        [
            "InvalidType: unexpected string, expected u32 at line 1 column 13",
            "InvalidType: unexpected string, expected u32 at line 1 column 63",
            "MissingField: missing field `height` at line 1 column 66",
            "UnknownVariant: unknown variant `x` of Shape, expected `circle` or `rect` at line 1 column 81",
        ]
    );
}

#[derive(Debug, Deserialize, PartialEq)]
struct Inner {
    a: u32,
    b: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Outer {
    name: String,
    #[deser(flatten)]
    inner: Inner,
    #[deser(flatten)]
    rest: BTreeMap<String, u32>,
}

#[test]
fn test_collect_flatten() {
    // errors of the keys flattened fields take are theirs, fields that
    // failed are not missing
    let rv = collect::<Outer>(r#"{"a": "x", "name": 1, "other": "y", "more": 2}"#);
    if DIALECT.hjson {
        // `name` receives the text of the number, like it does for
        // `"name": "1"` in JSON the error of `other` is not reported then
        assert_eq!(
            errors(rv),
            [
                "InvalidType: unexpected string, expected u32 at line 1 column 7",
                "MissingField: missing field `b` at line 1 column 46",
            ]
        );
        return;
    }
    assert_eq!(
        errors(rv),
        [
            "InvalidType: unexpected unsigned integer, expected string at line 1 column 20",
            "InvalidType: unexpected string, expected u32 at line 1 column 7",
            "MissingField: missing field `b` at line 1 column 46",
            "InvalidType: unexpected string, expected u32 at line 1 column 32",
        ]
    );
}

#[test]
fn test_collect_separated() {
    use deser::adapters::{Separated, TrimWhitespace};

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Config {
        #[deser(as = Separated<',', TrimWhitespace>)]
        ports: Vec<u16>,
    }

    // the pieces are the elements of the sequence, their errors are
    // collected like the errors of elements
    let rv = collect::<Config>(r#"{"ports": "80, x, 443, y"}"#);
    assert_eq!(
        errors(rv),
        [
            r#"InvalidValue: invalid value "x", expected u16 at line 1 column 11"#,
            r#"InvalidValue: invalid value "y", expected u16 at line 1 column 11"#,
        ]
    );
    let config = collect::<Config>(r#"{"ports": "80, 443"}"#).unwrap();
    assert_eq!(config.ports, [80, 443]);
}
