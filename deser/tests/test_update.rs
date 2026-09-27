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
    // values other than structs are replaced
    let mut map_value = BTreeMap::from([("a".to_string(), 1u32)]);
    update(&mut map_value, map(vec![("b", v(2u64))])).unwrap();
    assert_eq!(map_value, BTreeMap::from([("b".to_string(), 2)]));

    let mut number = 1u32;
    update(&mut number, v(2u64)).unwrap();
    assert_eq!(number, 2);

    let mut option = Some(defaults().limits);
    update(&mut option, map(vec![("connections", v(1u64))])).unwrap();
    assert_eq!(option.unwrap().connections, 1);
}

fn positive(value: &u32) -> Result<(), &'static str> {
    if *value == 0 {
        Err("must be positive")
    } else {
        Ok(())
    }
}

fn ordered(value: &Range) -> Result<(), &'static str> {
    if value.min > value.max {
        Err("min is larger than max")
    } else {
        Ok(())
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(validate = ordered, deny_unknown_fields)]
struct Range {
    #[deser(validate = positive)]
    min: u32,
    max: u32,
}

#[test]
fn test_validation() {
    let mut range = Range { min: 1, max: 5 };
    update(&mut range, map(vec![("max", v(3u64))])).unwrap();
    assert_eq!(range, Range { min: 1, max: 3 });

    let rv = update(&mut range, map(vec![("min", v(0u64))]));
    assert_eq!(rv.unwrap_err().message(), "invalid value: must be positive");
    assert_eq!(range, Range { min: 1, max: 3 });

    // the container is validated after the update
    let rv = update(&mut range, map(vec![("min", v(4u64))]));
    assert_eq!(
        rv.unwrap_err().message(),
        "invalid value: min is larger than max"
    );

    // unknown fields
    let rv = update(&mut range, map(vec![("x", v(4u64))]));
    assert_eq!(
        rv.unwrap_err().message(),
        "unknown field `x`, expected `min` or `max`"
    );
}

#[test]
fn test_unknown_fields_policy() {
    let mut limits = defaults().limits;
    let mut driver = DeserializeDriver::update(&mut limits);
    *driver.state_mut().get_mut::<UnknownFields>() = UnknownFields::Error;
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

    // structs with flattened fields are replaced
    let mut value = WithFlatten {
        a: 1,
        limits: defaults().limits,
    };
    let rv = update(&mut value, map(vec![("a", v(2u64))]));
    assert_eq!(rv.unwrap_err().message(), "missing field `connections`");
}
