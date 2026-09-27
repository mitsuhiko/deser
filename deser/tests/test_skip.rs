use std::marker::PhantomData;

use deser::de::{DeserializeDriver, DeserializeOwned, UnknownFields};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Error, Event, Serialize};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        *driver.state_mut().get_mut::<UnknownFields>() = UnknownFields::Error;
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
