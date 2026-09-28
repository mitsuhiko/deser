use std::collections::{BTreeMap, HashMap};

use deser::adapters::MapSkipError;
use deser::de::{DeserializeDriver, DeserializeOwned, DuplicateKeys};
use deser::{Deserialize, Error, Event};

fn deserialize<T: DeserializeOwned>(
    policy: Option<DuplicateKeys>,
    events: Vec<Event<'_>>,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        if let Some(policy) = policy {
            *driver.state_mut().get_mut::<DuplicateKeys>() = policy;
        }
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

fn map<'a>(pairs: &[(&'a str, Event<'a>)]) -> Vec<Event<'a>> {
    let mut events = vec![Event::map_start()];
    for (key, value) in pairs {
        events.push((*key).into());
        events.push(value.clone());
    }
    events.push(Event::MapEnd);
    events
}

#[derive(Debug, Deserialize, PartialEq)]
struct Item {
    #[deser(alias = "identifier")]
    id: u32,
    #[deser(default)]
    tags: Vec<String>,
    name: Option<String>,
}

fn tags<'a>(values: &[&'a str]) -> Vec<Event<'a>> {
    let mut events = vec![Event::seq_start()];
    events.extend(values.iter().map(|x| Event::from(*x)));
    events.push(Event::SeqEnd);
    events
}

fn item_events() -> Vec<Event<'static>> {
    let mut events = vec![Event::map_start()];
    events.extend(["id".into(), 1u64.into(), "name".into(), "a".into()]);
    events.push("tags".into());
    events.extend(tags(&["x"]));
    events.extend(["identifier".into(), 2u64.into(), "name".into(), "b".into()]);
    events.push("tags".into());
    events.extend(tags(&["y", "z"]));
    events.push(Event::MapEnd);
    events
}

#[test]
fn test_struct_default() {
    // duplicate fields are rejected by default
    let err = deserialize::<Item>(None, item_events()).unwrap_err();
    assert_eq!(err.message(), "duplicate field `id`");
}

#[test]
fn test_struct_last() {
    // also for values that are containers and aliases
    assert_eq!(
        deserialize::<Item>(Some(DuplicateKeys::Last), item_events()).unwrap(),
        Item {
            id: 2,
            tags: vec!["y".into(), "z".into()],
            name: Some("b".into()),
        }
    );
}

#[test]
fn test_struct_first() {
    assert_eq!(
        deserialize::<Item>(Some(DuplicateKeys::First), item_events()).unwrap(),
        Item {
            id: 1,
            tags: vec!["x".into()],
            name: Some("a".into()),
        }
    );
}

#[test]
fn test_struct_error() {
    let err = deserialize::<Item>(Some(DuplicateKeys::Error), item_events()).unwrap_err();
    assert_eq!(err.message(), "duplicate field `id`");

    let err = deserialize::<Item>(
        Some(DuplicateKeys::Error),
        map(&[
            ("id", 1u64.into()),
            ("name", Event::from("a")),
            ("name", ().into()),
        ]),
    )
    .unwrap_err();
    assert_eq!(err.message(), "duplicate field `name`");

    // unknown keys are not fields, they are ignored
    assert_eq!(
        deserialize::<Item>(
            Some(DuplicateKeys::Error),
            map(&[("id", 1u64.into()), ("x", 1u64.into()), ("x", 2u64.into())]),
        )
        .unwrap(),
        Item {
            id: 1,
            tags: vec![],
            name: None,
        }
    );
}

#[test]
fn test_flatten() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Paginate {
        limit: u32,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        q: String,
        #[deser(flatten)]
        paginate: Paginate,
    }

    let events = || {
        map(&[
            ("limit", 1u64.into()),
            ("q", "a".into()),
            ("limit", 2u64.into()),
        ])
    };
    assert_eq!(
        deserialize::<Query>(Some(DuplicateKeys::Last), events())
            .unwrap()
            .paginate
            .limit,
        2
    );
    assert_eq!(
        deserialize::<Query>(Some(DuplicateKeys::First), events())
            .unwrap()
            .paginate
            .limit,
        1
    );
    let err = deserialize::<Query>(Some(DuplicateKeys::Error), events()).unwrap_err();
    assert_eq!(err.message(), "duplicate field `limit`");
}

#[test]
fn test_buffered() {
    // the policy applies to values that are replayed
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "type")]
    enum Message {
        Ping { id: u32 },
    }

    let events = || {
        map(&[
            ("id", 1u64.into()),
            ("id", 2u64.into()),
            ("type", "Ping".into()),
        ])
    };
    assert_eq!(
        deserialize::<Message>(Some(DuplicateKeys::Last), events()).unwrap(),
        Message::Ping { id: 2 }
    );
    assert_eq!(
        deserialize::<Message>(Some(DuplicateKeys::First), events()).unwrap(),
        Message::Ping { id: 1 }
    );
    let err = deserialize::<Message>(Some(DuplicateKeys::Error), events()).unwrap_err();
    assert_eq!(err.message(), "duplicate field `id`");
}

#[test]
fn test_maps() {
    let events = || map(&[("a", 1u64.into()), ("b", 2u64.into()), ("a", 3u64.into())]);

    let err = deserialize::<BTreeMap<String, u32>>(None, events()).unwrap_err();
    assert_eq!(err.message(), "duplicate key in map");

    let rv = deserialize::<BTreeMap<String, u32>>(Some(DuplicateKeys::Last), events()).unwrap();
    assert_eq!(rv, BTreeMap::from([("a".into(), 3), ("b".into(), 2)]));
    let rv = deserialize::<HashMap<String, u32>>(Some(DuplicateKeys::Last), events()).unwrap();
    assert_eq!(rv, HashMap::from([("a".into(), 3), ("b".into(), 2)]));

    let rv = deserialize::<BTreeMap<String, u32>>(Some(DuplicateKeys::First), events()).unwrap();
    assert_eq!(rv, BTreeMap::from([("a".into(), 1), ("b".into(), 2)]));
    let rv = deserialize::<HashMap<String, u32>>(Some(DuplicateKeys::First), events()).unwrap();
    assert_eq!(rv, HashMap::from([("a".into(), 1), ("b".into(), 2)]));

    let err =
        deserialize::<BTreeMap<String, u32>>(Some(DuplicateKeys::Error), events()).unwrap_err();
    assert_eq!(err.message(), "duplicate key in map");
    let err =
        deserialize::<HashMap<String, u32>>(Some(DuplicateKeys::Error), events()).unwrap_err();
    assert_eq!(err.message(), "duplicate key in map");
}

#[test]
fn test_map_skip_error() {
    // duplicate entries fail with `Error`, which skips them
    #[derive(Debug, Deserialize)]
    struct Weights {
        #[deser(as = MapSkipError)]
        weights: BTreeMap<String, u32>,
    }

    let events = || {
        let mut events = vec![Event::map_start(), "weights".into()];
        events.extend(map(&[
            ("a", 1u64.into()),
            ("a", 2u64.into()),
            ("b", "x".into()),
        ]));
        events.push(Event::MapEnd);
        events
    };
    for (policy, expected) in [
        (DuplicateKeys::Last, 2),
        (DuplicateKeys::First, 1),
        (DuplicateKeys::Error, 1),
    ] {
        let rv = deserialize::<Weights>(Some(policy), events()).unwrap();
        assert_eq!(rv.weights, BTreeMap::from([("a".into(), expected)]));
    }
}

/// A struct with more fields than are tracked inline.
#[derive(Debug, Deserialize, PartialEq)]
struct Wide {
    f0: u32,
    f1: u32,
    f2: u32,
    f3: u32,
    f4: u32,
    f5: u32,
    f6: u32,
    f7: u32,
    f8: u32,
    f9: u32,
    f10: u32,
    f11: u32,
    f12: u32,
    f13: u32,
    f14: u32,
    f15: u32,
    f16: u32,
    f17: u32,
    f18: u32,
    f19: u32,
    f20: u32,
    f21: u32,
    f22: u32,
    f23: u32,
    f24: u32,
    f25: u32,
    f26: u32,
    f27: u32,
    f28: u32,
    f29: u32,
    f30: u32,
    f31: u32,
    f32: u32,
    f33: u32,
    f34: u32,
    f35: u32,
    f36: u32,
    f37: u32,
    f38: u32,
    f39: u32,
    f40: u32,
    f41: u32,
    f42: u32,
    f43: u32,
    f44: u32,
    f45: u32,
    f46: u32,
    f47: u32,
    f48: u32,
    f49: u32,
    f50: u32,
    f51: u32,
    f52: u32,
    f53: u32,
    f54: u32,
    f55: u32,
    f56: u32,
    f57: u32,
    f58: u32,
    f59: u32,
    f60: u32,
    f61: u32,
    f62: u32,
    f63: u32,
    f64: u32,
    f65: u32,
    f66: u32,
    f67: u32,
    f68: u32,
    f69: u32,
}

#[test]
fn test_wide_struct() {
    let names = (0..70).map(|i| format!("f{i}")).collect::<Vec<_>>();
    let pairs = names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), Event::from(i as u64)))
        .collect::<Vec<_>>();
    let value = deserialize::<Wide>(None, map(&pairs)).unwrap();
    assert_eq!((value.f0, value.f63, value.f64, value.f69), (0, 63, 64, 69));

    // duplicates of the fields beyond the first 64 are detected too
    let mut duplicated = pairs.clone();
    duplicated.push(("f65", Event::from(100u64)));
    let err = deserialize::<Wide>(None, map(&duplicated)).unwrap_err();
    assert_eq!(err.message(), "duplicate field `f65`");
    let value = deserialize::<Wide>(Some(DuplicateKeys::Last), map(&duplicated)).unwrap();
    assert_eq!(value.f65, 100);
    let value = deserialize::<Wide>(Some(DuplicateKeys::First), map(&duplicated)).unwrap();
    assert_eq!(value.f65, 65);

    // missing fields beyond the first 64 are reported
    let err = deserialize::<Wide>(None, map(&pairs[..68])).unwrap_err();
    assert_eq!(err.message(), "missing field `f68`");
}
