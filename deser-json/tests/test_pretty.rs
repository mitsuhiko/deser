use std::collections::BTreeMap;

use deser::Serialize;
use deser::hints::{Compact, Expanded};
use deser_json::{Indent, SerializerConfig, to_string};

const PRETTY: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(2));

#[derive(Serialize)]
struct Service {
    name: &'static str,
    ports: Vec<u16>,
    labels: BTreeMap<&'static str, &'static str>,
    volumes: Vec<String>,
    env: BTreeMap<String, String>,
    command: Option<&'static str>,
}

fn service() -> Service {
    Service {
        name: "web",
        ports: vec![80, 443],
        labels: BTreeMap::from([("env", "prod"), ("tier", "web")]),
        volumes: vec![],
        env: BTreeMap::new(),
        command: None,
    }
}

/// Removes the whitespace outside of strings.
fn strip(json: &str) -> String {
    let mut rv = String::new();
    let mut in_str = false;
    let mut escaped = false;
    for c in json.chars() {
        if in_str {
            in_str = escaped || c != '"';
            escaped = !escaped && c == '\\';
        } else if c.is_whitespace() {
            continue;
        } else {
            in_str = c == '"';
        }
        rv.push(c);
    }
    rv
}

#[test]
fn test_pretty() {
    assert_eq!(
        PRETTY.to_string(&service()).unwrap(),
        r#"{
  "name": "web",
  "ports": [
    80,
    443
  ],
  "labels": {
    "env": "prod",
    "tier": "web"
  },
  "volumes": [],
  "env": {},
  "command": null
}"#
    );
    const FOUR: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(4));
    assert_eq!(
        FOUR.to_string(&vec![vec![1]]).unwrap(),
        "[\n    [\n        1\n    ]\n]"
    );
    const TAB: SerializerConfig = SerializerConfig::new().pretty(Indent::Tab);
    assert_eq!(
        TAB.to_string(&vec![vec![1]]).unwrap(),
        "[\n\t[\n\t\t1\n\t]\n]"
    );
    const ZERO: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(0));
    assert_eq!(
        ZERO.to_string(&vec![vec![1, 2]]).unwrap(),
        "[\n[\n1,\n2\n]\n]"
    );
}

#[test]
fn test_scalars_and_empty() {
    assert_eq!(PRETTY.to_string(&42).unwrap(), "42");
    assert_eq!(PRETTY.to_string(&"a\nb").unwrap(), r#""a\nb""#);
    assert_eq!(PRETTY.to_string(&Vec::<u32>::new()).unwrap(), "[]");
    assert_eq!(
        PRETTY.to_string(&BTreeMap::<u32, u32>::new()).unwrap(),
        "{}"
    );
    assert_eq!(
        PRETTY.to_string(&vec![Vec::<u32>::new()]).unwrap(),
        "[\n  []\n]"
    );
}

#[test]
fn test_deep_nesting() {
    let mut value = String::new();
    for _ in 0..100 {
        value.push('[');
    }
    for _ in 0..100 {
        value.push(']');
    }
    let parsed: deser::de::Recording = deser_json::from_str(&value).unwrap();
    let json = SerializerConfig::new()
        .pretty(Indent::Spaces(100))
        .to_string(&parsed)
        .unwrap();
    assert!(json.contains(&format!("\n{}[]\n", " ".repeat(9900))));
    assert_eq!(strip(&json), value);
}

#[test]
fn test_compact_setting() {
    let value = BTreeMap::from([("a", vec![1, 2]), ("b", vec![])]);
    const SPACED: SerializerConfig = SerializerConfig::new().compact(false);
    assert_eq!(
        SPACED.to_string(&value).unwrap(),
        r#"{"a": [1, 2], "b": []}"#
    );
    // indentation without spaces after separators
    const INDENTED: SerializerConfig = SerializerConfig::new().indent(Indent::Spaces(2));
    assert_eq!(
        INDENTED.to_string(&value).unwrap(),
        "{\n  \"a\":[\n    1,\n    2\n  ],\n  \"b\":[]\n}"
    );
    // pretty with no indentation is the default again
    const RESET: SerializerConfig = PRETTY.pretty(Indent::None);
    assert_eq!(RESET, SerializerConfig::new());
    assert_eq!(RESET.to_string(&value).unwrap(), r#"{"a":[1,2],"b":[]}"#);
    // the order matters
    const PRETTY_COMPACT: SerializerConfig = PRETTY.compact(true);
    assert_eq!(PRETTY_COMPACT, INDENTED);
}

#[test]
fn test_layout_hints() {
    #[derive(Serialize)]
    struct Hinted {
        #[deser(as = Compact)]
        point: BTreeMap<&'static str, u32>,
        #[deser(as = Compact)]
        matrix: Vec<Vec<u32>>,
        #[deser(as = Compact<Vec<Expanded>>)]
        rows: Vec<Vec<u32>>,
        #[deser(as = Expanded)]
        list: Vec<u32>,
    }
    let value = Hinted {
        point: BTreeMap::from([("x", 1), ("y", 2)]),
        matrix: vec![vec![1, 2], vec![3]],
        rows: vec![vec![1, 2]],
        list: vec![1],
    };
    // expanded containers in compact containers are compact too
    assert_eq!(
        PRETTY.to_string(&value).unwrap(),
        r#"{
  "point": {"x": 1, "y": 2},
  "matrix": [[1, 2], [3]],
  "rows": [[1, 2]],
  "list": [
    1
  ]
}"#
    );
    // without indentation hints have no effect
    assert_eq!(
        to_string(&value).unwrap(),
        r#"{"point":{"x":1,"y":2},"matrix":[[1,2],[3]],"rows":[[1,2]],"list":[1]}"#
    );
    assert_eq!(
        SerializerConfig::new()
            .compact(false)
            .to_string(&value)
            .unwrap(),
        r#"{"point": {"x": 1, "y": 2}, "matrix": [[1, 2], [3]], "rows": [[1, 2]], "list": [1]}"#
    );
}

#[test]
fn test_keys() {
    let value = BTreeMap::from([(1u32, BTreeMap::from([('x', true)]))]);
    assert_eq!(
        PRETTY.to_string(&value).unwrap(),
        "{\n  \"1\": {\n    \"x\": true\n  }\n}"
    );
    let value = BTreeMap::from([(vec![1u32], 1u32)]);
    assert!(PRETTY.to_string(&value).is_err());
}

#[test]
fn test_same_as_compact() {
    let value = (
        service(),
        vec![Some(1.5f64), None],
        BTreeMap::from([("a b", vec![vec!["c d"]]), ("\"q\" \\", vec![])]),
        "  spaced  ",
    );
    let compact = to_string(&value).unwrap();
    for config in [
        PRETTY,
        SerializerConfig::new().indent(Indent::Tab),
        SerializerConfig::new().compact(false),
        SerializerConfig::new().pretty(Indent::Spaces(3)),
    ] {
        let json = config.to_string(&value).unwrap();
        assert_eq!(strip(&json), compact, "{}", json);
    }
}
