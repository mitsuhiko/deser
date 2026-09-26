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
            driver.state_mut().set_duplicate_keys(policy);
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
fn test_struct_last() {
    // the default, also for values that are containers and aliases
    assert_eq!(
        deserialize::<Item>(None, item_events()).unwrap(),
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
    assert_eq!(err.message(), "duplicate field 'id'");

    let err = deserialize::<Item>(
        Some(DuplicateKeys::Error),
        map(&[
            ("id", 1u64.into()),
            ("name", Event::from("a")),
            ("name", ().into()),
        ]),
    )
    .unwrap_err();
    assert_eq!(err.message(), "duplicate field 'name'");

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
        deserialize::<Query>(None, events()).unwrap().paginate.limit,
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
    assert_eq!(err.message(), "duplicate field 'limit'");
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
        deserialize::<Message>(None, events()).unwrap(),
        Message::Ping { id: 2 }
    );
    assert_eq!(
        deserialize::<Message>(Some(DuplicateKeys::First), events()).unwrap(),
        Message::Ping { id: 1 }
    );
    let err = deserialize::<Message>(Some(DuplicateKeys::Error), events()).unwrap_err();
    assert_eq!(err.message(), "duplicate field 'id'");
}

#[test]
fn test_maps() {
    let events = || map(&[("a", 1u64.into()), ("b", 2u64.into()), ("a", 3u64.into())]);

    let rv = deserialize::<BTreeMap<String, u32>>(None, events()).unwrap();
    assert_eq!(rv, BTreeMap::from([("a".into(), 3), ("b".into(), 2)]));
    let rv = deserialize::<HashMap<String, u32>>(None, events()).unwrap();
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
