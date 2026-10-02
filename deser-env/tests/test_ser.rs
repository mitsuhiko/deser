use std::collections::BTreeMap;

use deser::adapters::Separated;
use deser::{Deserialize, Serialize};
use deser_env::{Case, SerializerConfig, from_vars, to_vars};

fn vars(value: &[(String, String)]) -> Vec<(&str, &str)> {
    value
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Config {
    name: String,
    debug: bool,
    ratio: f64,
    server: Server,
    hosts: Vec<String>,
    #[deser(as = Separated)]
    tags: Vec<String>,
    features: BTreeMap<String, bool>,
    timeout: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Server {
    port: u16,
    max_connections: u32,
}

fn config() -> Config {
    Config {
        name: "shop".into(),
        debug: true,
        ratio: 0.5,
        server: Server {
            port: 80,
            max_connections: 5,
        },
        hosts: vec!["a".into(), "b".into()],
        tags: vec!["x".into(), "y".into()],
        features: BTreeMap::from([("new_ui".into(), true)]),
        timeout: None,
    }
}

#[test]
fn test_basics() {
    let out = to_vars("APP_", &config()).unwrap();
    assert_eq!(
        vars(&out),
        [
            ("APP_NAME", "shop"),
            ("APP_DEBUG", "true"),
            ("APP_RATIO", "0.5"),
            ("APP_SERVER__PORT", "80"),
            ("APP_SERVER__MAX_CONNECTIONS", "5"),
            ("APP_HOSTS__0", "a"),
            ("APP_HOSTS__1", "b"),
            ("APP_TAGS", "x,y"),
            ("APP_FEATURES__NEW_UI", "true"),
        ]
    );

    // it reads back
    let back: Config = from_vars("APP_", vars(&out)).unwrap();
    assert_eq!(back, config());
}

#[test]
fn test_config() {
    let value = BTreeMap::from([("server", BTreeMap::from([("Port", 80)]))]);
    let out = SerializerConfig::builder()
        .separator("_")
        .case(Case::Preserve)
        .build()
        .to_vars("app.", &value)
        .unwrap();
    assert_eq!(vars(&out), [("app.server_Port", "80")]);

    // without a separator only flat values can be written
    let flat = SerializerConfig::builder().separator("").build();
    assert_eq!(
        vars(&flat.to_vars("", &BTreeMap::from([("a__b", 1)])).unwrap()),
        [("A__B", "1")]
    );
    let err = flat.to_vars("", &value).unwrap_err();
    assert_eq!(
        err.message(),
        "nested maps and sequences require a separator"
    );
}

#[test]
fn test_values() {
    // nulls in sequences keep their position, empty containers are skipped
    #[derive(Serialize)]
    struct Values {
        list: Vec<Option<u32>>,
        empty: Vec<u32>,
        nested: BTreeMap<String, Vec<u32>>,
        bytes: deser::adapters::As<Vec<u8>, deser::adapters::Base64>,
        big: u128,
    }

    let out = to_vars(
        "",
        &Values {
            list: vec![Some(1), None, Some(3)],
            empty: vec![],
            nested: BTreeMap::from([("a".into(), vec![])]),
            bytes: vec![1, 2, 3].into(),
            big: u128::MAX,
        },
    )
    .unwrap();
    assert_eq!(
        vars(&out),
        [
            ("LIST__0", "1"),
            ("LIST__1", ""),
            ("LIST__2", "3"),
            ("BYTES", "AQID"),
            ("BIG", "340282366920938463463374607431768211455"),
        ]
    );
}

#[test]
fn test_errors() {
    let err = to_vars("", &42).unwrap_err();
    assert_eq!(
        err.message(),
        "environment variables hold maps (like structs)"
    );
    assert!(to_vars("", &None::<u32>).unwrap().is_empty());

    let err = to_vars("", &BTreeMap::from([("a__b", 1)])).unwrap_err();
    assert_eq!(err.message(), "key \"a__b\" contains the separator");
    let err = to_vars("", &BTreeMap::from([("", 1)])).unwrap_err();
    assert_eq!(
        err.message(),
        "keys of environment variables must not be empty"
    );
}
