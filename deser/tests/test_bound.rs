use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Event, Serialize};

/// Removes the length from container starts, the tests are not about it.
fn without_len(event: deser::Event<'static>) -> deser::Event<'static> {
    match event {
        deser::Event::MapStart(shape) => {
            deser::Event::MapStart(deser::ContainerShape::new().with_order(shape.order()))
        }
        deser::Event::SeqStart(shape) => {
            deser::Event::SeqStart(deser::ContainerShape::new().with_order(shape.order()))
        }
        event => event,
    }
}

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
        rv.push(without_len(event.to_static()));
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

#[test]
fn test_field_bound() {
    // the bound of the field replaces the bounds inferred from it, `K` is
    // still bounded because of `other`
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder<K: Kind, V> {
        #[deser(
            serialize_bound(K::Value: Serialize),
            deserialize_bound(K::Value: Deserialize<'de>)
        )]
        value: K::Value,
        other: V,
    }

    let value = Holder::<Text, u32> {
        value: "x".into(),
        other: 1,
    };
    let events = serialize(&value);
    assert_eq!(deserialize::<Holder<Text, u32>>(events), value);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Tuple<K: Kind>(
        #[deser(bound(K::Value: Serialize + DeserializeOwned))] K::Value,
        u32,
    );

    let value = Tuple::<Text>("x".into(), 1);
    let events = serialize(&value);
    assert_eq!(deserialize::<Tuple<Text>>(events), value);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(tag = "type")]
    enum Message<K: Kind> {
        Text {
            #[deser(bound(K::Value: Serialize + DeserializeOwned))]
            value: K::Value,
        },
        Pair(
            #[deser(bound(K::Value: Serialize + DeserializeOwned))] K::Value,
            #[deser(skip)] u32,
        ),
    }

    let message = Message::<Text>::Text { value: "x".into() };
    let events = serialize(&message);
    assert_eq!(deserialize::<Message<Text>>(events), message);
}

#[test]
fn test_variant_bound() {
    // the bounds of a variant replace the bounds inferred from its fields,
    // `V` is still bounded because of `Other`
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(tag = "type", content = "content")]
    enum Message<K: Kind, V> {
        #[deser(
            serialize_bound(K::Value: Serialize),
            deserialize_bound(K::Value: Deserialize<'de>)
        )]
        Text {
            value: K::Value,
            #[deser(skip)]
            marker: std::marker::PhantomData<K>,
        },
        #[deser(
            serialize_bound(K::Value: Serialize),
            deserialize_bound(K::Value: Deserialize<'de>)
        )]
        Pair(K::Value, K::Value),
        Other {
            other: V,
        },
        Empty,
    }

    for message in [
        Message::<Text, u32>::Text {
            value: "x".into(),
            marker: std::marker::PhantomData,
        },
        Message::Pair("a".into(), "b".into()),
        Message::Other { other: 42 },
        Message::Empty,
    ] {
        let events = serialize(&message);
        assert_eq!(deserialize::<Message<Text, u32>>(events), message);
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum External<K: Kind> {
        #[deser(bound(K::Value: Serialize + DeserializeOwned))]
        Newtype(K::Value),
        #[deser(bound(K::Value: Serialize + DeserializeOwned))]
        Struct { value: K::Value },
    }

    for message in [
        External::<Text>::Newtype("x".into()),
        External::Struct { value: "y".into() },
    ] {
        let events = serialize(&message);
        assert_eq!(deserialize::<External<Text>>(events), message);
    }
}

#[test]
fn test_variant_bound_with_adapter() {
    use deser::adapters::DisplayFromStr;

    // the adapter would require `K::Value: Display` and `FromStr` for the
    // tuple of the fields without the bound
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Message<K: Kind> {
        #[deser(
            as = (DisplayFromStr, DisplayFromStr),
            bound(
                K::Value: std::fmt::Display + std::str::FromStr + Send + Sync,
                <K::Value as std::str::FromStr>::Err: std::fmt::Display,
            )
        )]
        Pair(K::Value, K::Value),
    }

    let message = Message::<Text>::Pair("a".into(), "b".into());
    let events = serialize(&message);
    assert_eq!(deserialize::<Message<Text>>(events), message);
}
