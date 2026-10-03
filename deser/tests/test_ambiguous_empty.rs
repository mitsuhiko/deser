//! Empty containers that can be the other kind of container (see
//! `ContainerShape::set_ambiguous_empty`).
use std::collections::{BTreeMap, HashMap};

use deser::de::{DeserializeDriver, DeserializeOwned, Recording};
use deser::{ContainerShape, Deserialize, Error, ErrorKind, Event};

fn ambiguous() -> ContainerShape {
    let mut shape = ContainerShape::with_len(0);
    shape.set_ambiguous_empty(true);
    shape
}

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

fn empty_seq() -> Vec<Event<'static>> {
    vec![Event::SeqStart(ambiguous()), Event::SeqEnd]
}

fn empty_map() -> Vec<Event<'static>> {
    vec![Event::MapStart(ambiguous()), Event::MapEnd]
}

#[derive(Debug, PartialEq, Deserialize)]
struct Settings {
    #[deser(default)]
    theme: Option<String>,
    #[deser(default)]
    flags: Vec<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Required {
    name: String,
}

#[test]
fn test_seq_as_map() {
    assert_eq!(
        deserialize::<BTreeMap<String, u32>>(empty_seq()).unwrap(),
        BTreeMap::new()
    );
    assert_eq!(
        deserialize::<HashMap<u32, u32>>(empty_seq()).unwrap(),
        HashMap::new()
    );
    assert_eq!(
        deserialize::<Settings>(empty_seq()).unwrap(),
        Settings {
            theme: None,
            flags: vec![]
        }
    );
    assert_eq!(
        deserialize::<Option<Box<BTreeMap<String, u32>>>>(empty_seq()).unwrap(),
        Some(Box::default())
    );
    // the map is taken, its fields are missing
    let err = deserialize::<Required>(empty_seq()).unwrap_err();
    assert_eq!(err.message(), "missing field `name`");
    // values that take sequences receive the sequence
    assert_eq!(
        deserialize::<Vec<u32>>(empty_seq()).unwrap(),
        Vec::<u32>::new()
    );
}

#[test]
fn test_map_as_seq() {
    assert_eq!(
        deserialize::<Vec<u32>>(empty_map()).unwrap(),
        Vec::<u32>::new()
    );
    assert_eq!(
        deserialize::<Option<Vec<String>>>(empty_map()).unwrap(),
        Some(vec![])
    );
    assert_eq!(deserialize::<[u32; 0]>(empty_map()).unwrap(), []);
    assert_eq!(
        deserialize::<BTreeMap<String, u32>>(empty_map()).unwrap(),
        BTreeMap::new()
    );
}

#[test]
fn test_nested() {
    let events = vec![
        Event::map_start(),
        "a".into(),
        Event::SeqStart(ambiguous()),
        Event::SeqEnd,
        "b".into(),
        Event::MapStart(ambiguous()),
        Event::MapEnd,
        Event::MapEnd,
    ];
    #[derive(Debug, PartialEq, Deserialize)]
    struct Outer {
        a: BTreeMap<String, u32>,
        b: Vec<Vec<u8>>,
    }
    assert_eq!(
        deserialize::<Outer>(events).unwrap(),
        Outer {
            a: BTreeMap::new(),
            b: vec![]
        }
    );
    // in a sequence whose elements are built in place
    let events = vec![
        Event::seq_start(),
        Event::MapStart(ambiguous()),
        Event::MapEnd,
        Event::SeqEnd,
    ];
    assert_eq!(
        deserialize::<Vec<Vec<u8>>>(events).unwrap(),
        vec![Vec::<u8>::new()]
    );
}

#[test]
fn test_rejected() {
    // without the flag, or if the container is not empty, nothing changes
    let events = vec![Event::SeqStart(ContainerShape::with_len(0)), Event::SeqEnd];
    let err = deserialize::<BTreeMap<String, u32>>(events).unwrap_err();
    assert_eq!(err.message(), "unexpected sequence, expected BTreeMap");
    let mut shape = ContainerShape::new();
    shape.set_ambiguous_empty(true);
    assert!(!shape.is_ambiguous_empty());
    let events = vec![Event::SeqStart(shape), Event::SeqEnd];
    assert!(deserialize::<BTreeMap<String, u32>>(events).is_err());
    // if the other kind is rejected too, the error is the one of the
    // container in the input
    let err = deserialize::<u32>(empty_seq()).unwrap_err();
    assert_eq!(err.message(), "unexpected sequence, expected u32");
    let err = deserialize::<u32>(empty_map()).unwrap_err();
    assert_eq!(err.message(), "unexpected map, expected u32");
}

#[test]
fn test_items_in_empty_container() {
    let events = vec![Event::SeqStart(ambiguous()), 1u64.into(), Event::SeqEnd];
    let err = deserialize::<BTreeMap<String, u32>>(events).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidState);
    let events = vec![
        Event::SeqStart(ambiguous()),
        Event::seq_start(),
        Event::SeqEnd,
        Event::SeqEnd,
    ];
    assert!(deserialize::<BTreeMap<String, u32>>(events).is_err());
}

#[test]
fn test_untagged() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(untagged)]
    enum Data {
        Map(BTreeMap<String, u32>),
        List(Vec<u32>),
    }
    // the variants are tried in order, the map takes the empty sequence
    assert_eq!(
        deserialize::<Data>(empty_seq()).unwrap(),
        Data::Map(BTreeMap::new())
    );
}

#[test]
fn test_recording() {
    // the flag is part of the recorded shape
    let recording = deserialize::<Recording>(empty_seq()).unwrap();
    let mut out = None::<BTreeMap<String, u32>>;
    let mut driver_out = None::<()>;
    let mut driver = DeserializeDriver::new(&mut driver_out);
    recording
        .replay(
            <BTreeMap<String, u32>>::deserialize_into(&mut out, driver.state_mut()),
            driver.state_mut(),
        )
        .unwrap();
    assert_eq!(out, Some(BTreeMap::new()));
}
