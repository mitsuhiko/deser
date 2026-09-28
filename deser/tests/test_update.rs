use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, UnknownFields};
use deser::{Deserialize, Error, Event};

fn update<T: for<'de> Deserialize<'de>>(
    value: &mut T,
    events: Vec<Event<'_>>,
) -> Result<(), Error> {
    let mut driver = DeserializeDriver::update(value);
    for event in events {
        driver.emit(event)?;
    }
    Ok(())
}

fn map<'a>(pairs: Vec<(&'a str, Vec<Event<'a>>)>) -> Vec<Event<'a>> {
    let mut events = vec![Event::map_start()];
    for (key, value) in pairs {
        events.push(key.into());
        events.extend(value);
    }
    events.push(Event::MapEnd);
    events
}

fn v<'a>(event: impl Into<Event<'a>>) -> Vec<Event<'a>> {
    vec![event.into()]
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Tls {
    cert: String,
    key: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Server {
    host: String,
    port: u16,
    tags: Vec<String>,
    tls: Option<Tls>,
    limits: Limits,
    #[deser(skip)]
    runtime: u32,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Limits {
    connections: u32,
    timeout: u32,
}

fn defaults() -> Server {
    Server {
        host: "localhost".into(),
        port: 80,
        tags: vec!["a".into()],
        tls: Some(Tls {
            cert: "cert.pem".into(),
            key: "key.pem".into(),
        }),
        limits: Limits {
            connections: 10,
            timeout: 30,
        },
        runtime: 7,
    }
}

#[test]
fn test_merge() {
    let mut server = defaults();
    update(
        &mut server,
        map(vec![
            ("port", v(8080u64)),
            ("limits", map(vec![("timeout", v(5u64))])),
            ("tls", map(vec![("key", v("other.pem"))])),
            ("tags", vec![Event::seq_start(), "b".into(), Event::SeqEnd]),
        ]),
    )
    .unwrap();
    let mut expected = defaults();
    expected.port = 8080;
    expected.limits.timeout = 5;
    expected.tls.as_mut().unwrap().key = "other.pem".into();
    // sequences are replaced
    expected.tags = vec!["b".into()];
    assert_eq!(server, expected);
}

#[test]
fn test_options() {
    // null clears options
    let mut server = defaults();
    update(&mut server, map(vec![("tls", v(()))])).unwrap();
    assert_eq!(server.tls, None);

    // options that are not set are deserialized, the value has to be
    // complete
    let rv = update(&mut server, map(vec![("tls", map(vec![("key", v("k"))]))]));
    assert_eq!(rv.unwrap_err().message(), "missing field `cert`");
    update(
        &mut server,
        map(vec![("tls", map(vec![("key", v("k")), ("cert", v("c"))]))]),
    )
    .unwrap();
    assert_eq!(
        server.tls,
        Some(Tls {
            cert: "c".into(),
            key: "k".into()
        })
    );

    // failed updates keep the (partially updated) value
    let rv = update(
        &mut server,
        map(vec![("tls", map(vec![("key", v("x")), ("cert", v(1u64))]))]),
    );
    assert_eq!(
        rv.unwrap_err().message(),
        "unexpected unsigned integer, expected string"
    );
    assert_eq!(server.tls.as_ref().unwrap().key, "x");
}

#[test]
fn test_top_level_values() {
    // maps are merged
    let mut map_value = BTreeMap::from([("a".to_string(), 1u32), ("b".to_string(), 1)]);
    update(&mut map_value, map(vec![("b", v(2u64)), ("c", v(3u64))])).unwrap();
    assert_eq!(
        map_value,
        BTreeMap::from([
            ("a".to_string(), 1),
            ("b".to_string(), 2),
            ("c".to_string(), 3)
        ])
    );

    let mut number = 1u32;
    update(&mut number, v(2u64)).unwrap();
    assert_eq!(number, 2);

    let mut option = Some(defaults().limits);
    update(&mut option, map(vec![("connections", v(1u64))])).unwrap();
    assert_eq!(option.unwrap().connections, 1);
}

#[test]
fn test_unknown_fields_policy() {
    let mut limits = defaults().limits;
    let mut driver = DeserializeDriver::update(&mut limits);
    UnknownFields::Error.set(driver.state_mut());
    driver.emit(Event::map_start()).unwrap();
    driver.emit("nope").unwrap();
    let err = driver.emit(1u64).unwrap_err();
    assert_eq!(
        err.message(),
        "unknown field `nope`, expected `connections` or `timeout`"
    );
}

#[derive(Debug, Deserialize, PartialEq)]
struct Wrapper(Limits);

#[derive(Debug, Deserialize, PartialEq)]
struct WithFlatten {
    a: u32,
    #[deser(flatten)]
    limits: Limits,
}

#[test]
fn test_newtypes_and_flatten() {
    // newtype structs update their field
    let mut value = Wrapper(defaults().limits);
    update(&mut value, map(vec![("timeout", v(1u64))])).unwrap();
    assert_eq!(
        value.0,
        Limits {
            connections: 10,
            timeout: 1
        }
    );

    // boxes update their value
    let mut value = Box::new(defaults().limits);
    update(&mut value, map(vec![("timeout", v(1u64))])).unwrap();
    assert_eq!(value.connections, 10);
    assert_eq!(value.timeout, 1);

    // flattened fields are updated with the keys they take, and kept if
    // they take none
    let mut value = WithFlatten {
        a: 1,
        limits: defaults().limits,
    };
    update(&mut value, map(vec![("a", v(2u64))])).unwrap();
    assert_eq!(
        value,
        WithFlatten {
            a: 2,
            limits: defaults().limits
        }
    );
    update(&mut value, map(vec![("timeout", v(1u64))])).unwrap();
    assert_eq!(
        value,
        WithFlatten {
            a: 2,
            limits: Limits {
                connections: 10,
                timeout: 1
            }
        }
    );
}

#[test]
fn test_maps() {
    use std::collections::HashMap;

    // the values of maps are replaced, not updated
    let mut limits = BTreeMap::from([("a".to_string(), defaults().limits)]);
    let rv = update(
        &mut limits,
        map(vec![("a", map(vec![("timeout", v(1u64))]))]),
    );
    assert_eq!(rv.unwrap_err().message(), "missing field `connections`");
    assert_eq!(limits["a"], defaults().limits);

    // keys of the map are not duplicates, keys in the data are
    let mut hash_map = HashMap::from([("a".to_string(), 1u32), ("b".to_string(), 1)]);
    update(&mut hash_map, map(vec![("a", v(2u64))])).unwrap();
    assert_eq!(
        hash_map,
        HashMap::from([("a".to_string(), 2), ("b".to_string(), 1)])
    );
    let rv = update(&mut hash_map, map(vec![("a", v(3u64)), ("a", v(4u64))]));
    assert_eq!(rv.unwrap_err().message(), "duplicate key in map");
    assert_eq!(hash_map["a"], 2);

    // larger updates of hash maps are merged the other way around
    let mut hash_map = HashMap::from([("a".to_string(), 1u32), ("z".to_string(), 1)]);
    update(
        &mut hash_map,
        map(vec![("a", v(2u64)), ("b", v(2u64)), ("c", v(2u64))]),
    )
    .unwrap();
    assert_eq!(
        hash_map,
        HashMap::from([
            ("a".to_string(), 2),
            ("b".to_string(), 2),
            ("c".to_string(), 2),
            ("z".to_string(), 1),
        ])
    );
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Settings {
    name: String,
    #[deser(flatten)]
    server: Flattened,
    #[deser(flatten)]
    tls: Option<Tls>,
    #[deser(flatten)]
    extra: BTreeMap<String, u32>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct Flattened {
    port: u16,
    #[deser(flatten)]
    limits: Limits,
}

fn settings() -> Settings {
    Settings {
        name: "a".into(),
        server: Flattened {
            port: 80,
            limits: defaults().limits,
        },
        tls: None,
        extra: BTreeMap::from([("x".to_string(), 1)]),
    }
}

#[test]
fn test_flatten() {
    let mut value = settings();
    update(
        &mut value,
        map(vec![
            ("timeout", v(1u64)),
            ("y", v(2u64)),
            ("port", v(81u64)),
            ("key", v("k")),
            ("cert", v("c")),
        ]),
    )
    .unwrap();
    let mut expected = settings();
    expected.server.port = 81;
    expected.server.limits.timeout = 1;
    expected.tls = Some(Tls {
        cert: "c".into(),
        key: "k".into(),
    });
    expected.extra.insert("y".into(), 2);
    assert_eq!(value, expected);

    // the options are updated too
    update(&mut value, map(vec![("key", v("k2"))])).unwrap();
    expected.tls.as_mut().unwrap().key = "k2".into();
    assert_eq!(value, expected);

    // duplicate keys are detected in the flattened fields
    let mut value = settings();
    let rv = update(&mut value, map(vec![("port", v(1u64)), ("port", v(2u64))]));
    assert_eq!(rv.unwrap_err().message(), "duplicate field `port`");
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct Strict {
    a: u32,
    #[deser(flatten)]
    limits: Limits,
}

#[test]
fn test_flatten_unknown_fields() {
    let mut value = Strict {
        a: 1,
        limits: defaults().limits,
    };
    let rv = update(&mut value, map(vec![("timeout", v(1u64)), ("b", v(1u64))]));
    assert_eq!(rv.unwrap_err().message(), "unknown field `b`");
    update(&mut value, map(vec![("timeout", v(1u64))])).unwrap();
    assert_eq!(value.limits.timeout, 1);
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "type")]
enum Backend {
    File { path: String },
    Memory { size: u32 },
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct Storage {
    name: String,
    #[deser(flatten)]
    backend: Backend,
}

#[test]
fn test_flatten_internally_tagged() {
    let mut value = Storage {
        name: "a".into(),
        backend: Backend::Memory { size: 1 },
    };
    // enums are replaced
    update(
        &mut value,
        map(vec![("path", v("/tmp")), ("type", v("File"))]),
    )
    .unwrap();
    assert_eq!(
        value.backend,
        Backend::File {
            path: "/tmp".into()
        }
    );
    update(&mut value, map(vec![("name", v("b"))])).unwrap();
    assert_eq!(value.name, "b");
    assert_eq!(
        value.backend,
        Backend::File {
            path: "/tmp".into()
        }
    );
    // keys the variant does not take are unknown keys of the struct
    let rv = update(
        &mut value,
        map(vec![
            ("type", v("Memory")),
            ("size", v(1u64)),
            ("path", v("x")),
        ]),
    );
    assert_eq!(rv.unwrap_err().message(), "unknown field `path`");
}
