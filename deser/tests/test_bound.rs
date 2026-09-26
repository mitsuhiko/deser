use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Event, Serialize};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> T {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event).unwrap();
        }
    }
    out.unwrap()
}

fn serialize<T: Serialize>(value: &T) -> Vec<Event<'static>> {
    let mut rv = Vec::new();
    let mut driver = SerializeDriver::new(value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        rv.push(event.to_static());
    }
    rv
}

/// A type level marker that picks the value type.
pub trait Kind: 'static {
    type Value;
}

/// Does not implement `Serialize` or `Deserialize`, so the inferred bounds
/// (`K: Serialize` and `K: Deserialize`) cannot be satisfied.
#[derive(Debug, PartialEq)]
pub struct Text;

impl Kind for Text {
    type Value = String;
}

#[test]
fn test_directional_bounds() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(
        serialize_bound(K::Value: Serialize),
        deserialize_bound(K::Value: Deserialize<'de>)
    )]
    struct Holder<K: Kind> {
        value: K::Value,
    }

    let events = serialize(&Holder::<Text> { value: "x".into() });
    assert_eq!(
        events,
        vec![
            Event::map_start(),
            "value".into(),
            "x".into(),
            Event::MapEnd
        ]
    );
    assert_eq!(
        deserialize::<Holder<Text>>(events),
        Holder { value: "x".into() }
    );
}

#[test]
fn test_shared_bound() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(bound(K::Value: Serialize + DeserializeOwned))]
    struct Holder<K: Kind> {
        value: K::Value,
    }

    let events = serialize(&Holder::<Text> { value: "x".into() });
    assert_eq!(
        deserialize::<Holder<Text>>(events),
        Holder { value: "x".into() }
    );
}

#[test]
fn test_empty_bound() {
    // the where clause of the type already has all bounds that are needed
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(bound())]
    struct Holder<K: Kind>
    where
        K::Value: Serialize + DeserializeOwned,
    {
        value: K::Value,
    }

    let events = serialize(&Holder::<Text> { value: "x".into() });
    assert_eq!(
        deserialize::<Holder<Text>>(events),
        Holder { value: "x".into() }
    );
}

#[test]
fn test_newtype_bound() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(bound(K::Value: Serialize + DeserializeOwned))]
    struct Holder<K: Kind>(K::Value);

    let events = serialize(&Holder::<Text>("x".into()));
    assert_eq!(events, vec!["x".into()]);
    assert_eq!(deserialize::<Holder<Text>>(events), Holder("x".into()));
}

#[test]
fn test_enum_bound() {
    // struct variants are deserialized through helper structs which only
    // get the bounds that refer to their type parameters.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(tag = "type", bound(K::Value: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned))]
    enum Message<K: Kind, V> {
        Text { value: K::Value },
        Other { other: V },
        Empty,
    }

    for message in [
        Message::<Text, u32>::Text { value: "x".into() },
        Message::Other { other: 42 },
        Message::Empty,
    ] {
        let events = serialize(&message);
        assert_eq!(deserialize::<Message<Text, u32>>(events), message);
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(bound(K::Value: Serialize + DeserializeOwned, V: Serialize + DeserializeOwned))]
    enum External<K: Kind, V> {
        Text { value: K::Value },
        Tuple(K::Value, V),
    }

    for message in [
        External::<Text, u32>::Text { value: "x".into() },
        External::Tuple("y".into(), 23),
    ] {
        let events = serialize(&message);
        assert_eq!(deserialize::<External<Text, u32>>(events), message);
    }
}
