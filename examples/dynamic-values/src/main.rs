//! Working with data whose structure is not known up front with
//! `deser-value`.
//!
//! A `Value` holds anything the deser data model can express.  This shows:
//!
//! * inspecting and transforming values (indexing, walking, the `value!`
//!   macro),
//! * converting between formats without losing information: a TOML
//!   date-time stays a date-time in YAML,
//! * keeping the fields a type does not know in a flattened `Map` and
//!   writing them back unchanged,
//! * merging files into one value and deserializing a type from it, with
//!   errors that point into the file the bad value came from.
use deser::Deserialize;
use deser::Serialize;
use deser::de::Deserializer as _;
use deser_path::{Path, PathLayer};
use deser_value::{Kind, Map, Value, from_value, to_value, value};

/// Lists the paths of all values with the kind of the value.
fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
    match value.kind() {
        Kind::Map(map) => {
            for (key, value) in map {
                let key = key.as_str().unwrap_or("?");
                walk(value, &format!("{}.{}", path, key), out);
            }
        }
        Kind::Seq(seq) => {
            for (index, value) in seq.iter().enumerate() {
                walk(value, &format!("{}[{}]", path, index), out);
            }
        }
        kind => out.push(format!("{}: {}", path, kind.name())),
    }
}

/// Replaces the values of all keys named `password`.
fn redact(value: &mut Value) {
    match value.kind_mut() {
        Kind::Map(map) => {
            for (key, value) in map.iter_mut() {
                if key.as_str() == Some("password") {
                    *value = value!("***");
                } else {
                    redact(value);
                }
            }
        }
        Kind::Seq(seq) => seq.iter_mut().for_each(redact),
        _ => {}
    }
}

/// A type that knows some fields and keeps the rest.
#[derive(Debug, Serialize, Deserialize)]
pub struct Event<'a> {
    // borrowed from the value
    kind: &'a str,
    at: u64,
    #[deser(flatten)]
    extra: Map,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    server: Server,
    debug: bool,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    host: String,
    port: u16,
}

fn inspect() {
    let mut value: Value = deser_json::from_str(
        r#"{
            "user": {"name": "jane", "password": "hunter2", "roles": ["admin", "dev"]},
            "sessions": [{"id": 1, "password": "abc"}],
            "score": 4.5,
            "active": null
        }"#,
    )
    .unwrap();

    let mut paths = Vec::new();
    walk(&value, "", &mut paths);
    println!("{}\n", paths.join("\n"));
    assert!(paths.contains(&".user.roles[1]: string".to_string()));
    assert!(paths.contains(&".score: float".to_string()));

    // indexing, accessors and the `value!` macro
    assert_eq!(value["user"]["roles"][0], "admin");
    assert_eq!(value["score"].as_f64(), Some(4.5));
    assert!(value["active"].is_null());
    assert!(value.get("missing").is_none());
    value["user"]["roles"] = value!(["admin"]);
    value["user"]["login"] = value!({"count": 3, "last": null});
    value.as_map_mut().unwrap().remove("active");
    redact(&mut value);

    let json = deser_json::to_string(&value).unwrap();
    println!("{}\n", json);
    assert_eq!(
        json,
        r#"{"user":{"name":"jane","password":"***","roles":["admin"],"login":{"count":3,"last":null}},"sessions":[{"id":1,"password":"***"}],"score":4.5}"#
    );
}

fn convert() {
    let toml = r#"
title = "release"
published = 2024-05-01T10:30:00Z
day = 2024-05-01
tags = ["stable", "lts"]
"#;
    let value: Value = deser_toml::from_str(toml).unwrap();
    // the date-times keep their type
    println!("published is a {}", value["published"].kind().name());
    assert!(matches!(value["published"].kind(), Kind::Ext(_)));

    // YAML has timestamps, JSON has strings for them
    let yaml = deser_yaml::to_string(&value).unwrap();
    println!("{}", yaml);
    let json = deser_json::to_string(&value).unwrap();
    println!("{}\n", json);
    assert!(json.contains(r#""published":"2024-05-01T10:30:00Z""#));
    // and back to TOML they are date-times again, not strings
    let back = deser_toml::to_string(&value).unwrap();
    assert!(back.contains("published = 2024-05-01T10:30:00Z"));
}

fn unknown_fields() {
    let value: Value = deser_json::from_str(
        r#"{"kind": "click", "at": 1715000000, "x": 10, "y": 20, "target": {"id": "buy"}}"#,
    )
    .unwrap();
    let event: Event = from_value(&value).unwrap();
    println!("{:?}", event);
    assert_eq!(event.kind, "click");
    assert_eq!(event.extra.get("target").unwrap()["id"], "buy");

    // the fields the type does not know are written back
    assert_eq!(to_value(&event).unwrap(), value);
    println!("{}\n", deser_json::to_string(&event).unwrap());
}

fn merge() {
    let base = r#"{"server": {"host": "localhost", "port": 8080}, "debug": false}"#;
    let local =
        "{\n  \"server\": {\n    \"host\": \"example.com\",\n    \"port\": \"eighty\"\n  }\n}";

    // values remember where they came from
    let json = deser_json::DeserializerConfig::new().track_locations(true);
    let mut merged: Value = json.from_str(base).unwrap();
    deser_json::Deserializer::from_str_with_config(local, &json)
        .update(&mut merged)
        .unwrap();
    // the keys of the file are merged into the map, the values of keys that
    // exist are replaced
    assert_eq!(merged["server"]["host"], "example.com");
    assert_eq!(merged["debug"], false);

    let err = deser_value::Deserializer::new(&merged)
        .deserialize_with::<Config, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    println!("error: {}", err);
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "server.port");
    // the location is in the file the value came from
    assert_eq!((err.line(), err.column()), (Some(4), Some(13)));
    let span = merged["server"]["port"].span().unwrap();
    assert_eq!(&**span.source(), local);
    assert_eq!(span.text(), Some("\"eighty\""));

    let config: Config = from_value(&json.from_str::<Value>(base).unwrap()).unwrap();
    assert_eq!(config.server.port, 8080);
    assert_eq!(config.server.host, "localhost");
    assert!(!config.debug);
}

fn main() {
    inspect();
    convert();
    unknown_fields();
    merge();
}
