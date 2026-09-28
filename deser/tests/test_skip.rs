use std::marker::PhantomData;

use deser::de::{DeserializeDriver, DeserializeOwned, UnknownFields};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Error, Event, Serialize};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        UnknownFields::Error.set(driver.state_mut());
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

fn serialize<T: Serialize>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(match event {
            Event::MapStart(_) => Event::map_start(),
            event => event.to_static(),
        });
    }
    events
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

/// A type that is neither serializable nor deserializable.
#[derive(Debug, Default, PartialEq)]
struct Cache(u32);

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Item {
    id: u32,
    #[deser(skip)]
    cache: Cache,
    #[deser(skip, default = 42)]
    answer: u32,
    #[deser(skip_serializing)]
    secret: String,
    #[deser(skip_deserializing)]
    computed: u32,
}

#[test]
fn test_skip() {
    let item: Item = deserialize(map(&[("id", 1u64.into()), ("secret", "x".into())])).unwrap();
    assert_eq!(
        item,
        Item {
            id: 1,
            cache: Cache(0),
            answer: 42,
            secret: "x".into(),
            computed: 0,
        }
    );
    let item = Item {
        id: 1,
        cache: Cache(1),
        answer: 1,
        secret: "x".into(),
        computed: 2,
    };
    assert_eq!(
        serialize(&item),
        map(&[("id", 1u64.into()), ("computed", 2u64.into())])
    );

    // the keys of fields that are not deserialized are unknown
    let rv = deserialize::<Item>(map(&[("id", 1u64.into()), ("computed", 2u64.into())]));
    assert_eq!(
        rv.unwrap_err().message(),
        "unknown field `computed`, expected `id` or `secret`"
    );
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(default)]
struct WithDefault {
    a: u32,
    #[deser(skip)]
    b: u32,
}

impl Default for WithDefault {
    fn default() -> WithDefault {
        WithDefault { a: 1, b: 2 }
    }
}

#[test]
fn test_skip_with_container_default() {
    let value: WithDefault = deserialize(map(&[("a", 5u64.into())])).unwrap();
    assert_eq!(value, WithDefault { a: 5, b: 2 });
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Generic<T, U> {
    value: T,
    #[deser(skip)]
    marker: PhantomData<U>,
    #[deser(skip)]
    other: Option<U>,
}

#[test]
fn test_skip_generics() {
    // `U` needs neither `Serialize` nor `Deserialize`
    let value: Generic<u32, Cache> = deserialize(map(&[("value", 1u64.into())])).unwrap();
    assert_eq!(value.value, 1);
    assert_eq!(value.other, None);
    assert_eq!(serialize(&value), map(&[("value", 1u64.into())]));
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(tag = "type")]
enum Shape {
    Circle {
        radius: u32,
        #[deser(skip)]
        area: u32,
    },
}

#[test]
fn test_skip_in_variants() {
    let shape: Shape =
        deserialize(map(&[("type", "Circle".into()), ("radius", 1u64.into())])).unwrap();
    assert_eq!(shape, Shape::Circle { radius: 1, area: 0 });
    assert_eq!(
        serialize(&Shape::Circle { radius: 1, area: 3 }),
        map(&[("type", "Circle".into()), ("radius", 1u64.into())])
    );
}

/// Skipped fields of generic types need a default.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum GenericVariant<T> {
    Value {
        value: u32,
        #[deser(skip)]
        extra: T,
    },
    Empty,
}

#[test]
fn test_skip_generics_in_variants() {
    let events = vec![
        Event::map_start(),
        "Value".into(),
        Event::map_start(),
        "value".into(),
        1u64.into(),
        Event::MapEnd,
        Event::MapEnd,
    ];
    let value: GenericVariant<Vec<u32>> = deserialize(events.clone()).unwrap();
    assert_eq!(
        value,
        GenericVariant::Value {
            value: 1,
            extra: Vec::new()
        }
    );
    assert_eq!(serialize(&value), events);
}

fn try_serialize<T: Serialize>(value: &T) -> Result<(), Error> {
    let mut driver = SerializeDriver::new(value);
    while driver.next()?.is_some() {}
    Ok(())
}

/// `C` is only used in skipped variants, it needs neither `Serialize` nor
/// `Deserialize`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(tag = "type")]
enum Message<C> {
    Text {
        text: String,
    },
    #[deser(skip)]
    Local(C),
    #[deser(skip_serializing)]
    Legacy {
        body: String,
    },
    #[deser(skip_deserializing)]
    Generated {
        id: u32,
    },
}

#[test]
fn test_skip_variants() {
    type M = Message<Cache>;
    let rv = deserialize::<M>(map(&[("type", "Legacy".into()), ("body", "a".into())]));
    assert_eq!(rv.unwrap(), M::Legacy { body: "a".into() });
    // skipped variants are unknown variants
    for name in ["Local", "Generated"] {
        let rv = deserialize::<M>(map(&[("type", name.into())]));
        assert_eq!(
            rv.unwrap_err().message(),
            format!(
                "unknown variant `{}` of Message, expected `Text` or `Legacy`",
                name
            )
        );
    }
    assert_eq!(
        serialize(&M::Generated { id: 1 }),
        map(&[("type", "Generated".into()), ("id", 1u64.into())])
    );
    for (value, name) in [
        (M::Local(Cache(1)), "Local"),
        (M::Legacy { body: "a".into() }, "Legacy"),
    ] {
        assert_eq!(
            try_serialize(&value).unwrap_err().message(),
            format!("the variant `{}` of Message cannot be serialized", name)
        );
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Level {
    Low,
    #[deser(skip)]
    Unknown,
    #[deser(skip_serializing)]
    Medium,
    #[deser(skip_deserializing, rename = 3)]
    High,
}

#[test]
fn test_skip_unit_variants() {
    assert_eq!(
        deserialize::<Level>(vec!["Medium".into()]).unwrap(),
        Level::Medium
    );
    for event in [
        Event::from("Unknown"),
        Event::from("High"),
        Event::from(3u64),
    ] {
        let err = deserialize::<Level>(vec![event]).unwrap_err();
        assert!(err.message().starts_with("unknown variant"), "{}", err);
        assert!(
            err.message()
                .ends_with("of Level, expected `Low` or `Medium`"),
            "{}",
            err
        );
    }
    assert_eq!(serialize(&Level::High), vec![Event::from(3u64)]);
    assert_eq!(
        try_serialize(&Level::Unknown).unwrap_err().message(),
        "the variant `Unknown` of Level cannot be serialized"
    );
    assert_eq!(
        try_serialize(&Level::Medium).unwrap_err().message(),
        "the variant `Medium` of Level cannot be serialized"
    );
}

#[derive(Debug, Deserialize, PartialEq)]
struct Required {
    #[deser(required)]
    value: Option<u32>,
    other: Option<u32>,
}

#[test]
fn test_required() {
    let value: Required = deserialize(map(&[("value", ().into())])).unwrap();
    assert_eq!(
        value,
        Required {
            value: None,
            other: None
        }
    );
    let rv = deserialize::<Required>(map(&[("other", 1u64.into())]));
    assert_eq!(rv.unwrap_err().message(), "missing field `value`");
}
