//! Tests for tuple structs and unit structs.
use std::fmt::Debug;
use std::marker::PhantomData;

use deser::adapters::DisplayFromStr;
use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::{Describe, SerializeDriver};
use deser::{Atom, Deserialize, Error, Event, Serialize};

/// Removes the length from container starts, the tests are not about it.
fn without_len(event: Event<'static>) -> Event<'static> {
    match event {
        Event::MapStart(shape) => {
            Event::MapStart(deser::ContainerShape::new().with_order(shape.order()))
        }
        Event::SeqStart(shape) => {
            Event::SeqStart(deser::ContainerShape::new().with_order(shape.order()))
        }
        event => event,
    }
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

fn serialize(value: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(without_len(event.to_static()));
    }
    events
}

/// Checks the serialized form and that it deserializes back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
    assert_eq!(
        serialize(&value),
        events.iter().map(|x| x.to_static()).collect::<Vec<_>>()
    );
    assert_eq!(deserialize::<T>(events).unwrap(), value);
}

#[derive(Default)]
struct Names(Vec<String>);

impl Describe for Names {
    fn tuple_struct(&mut self, name: &str) {
        self.0.push(format!("tuple struct {}", name));
    }

    fn unit_struct(&mut self, name: &str) {
        self.0.push(format!("unit struct {}", name));
    }
}

fn describe(value: &dyn Serialize) -> Vec<String> {
    let mut names = Names::default();
    value.describe(&mut names);
    names.0
}

#[test]
fn test_tuple_struct() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Pair(u32, String);

    check(
        Pair(1, "two".into()),
        vec![Event::seq_start(), 1u64.into(), "two".into(), Event::SeqEnd],
    );
    assert_eq!(describe(&Pair(1, "two".into())), ["tuple struct Pair"]);

    let err =
        deserialize::<Pair>(vec![Event::seq_start(), 1u64.into(), Event::SeqEnd]).unwrap_err();
    assert_eq!(err.message(), "not enough elements in tuple");
    let err = deserialize::<Pair>(vec![
        Event::seq_start(),
        1u64.into(),
        "two".into(),
        "three".into(),
    ])
    .unwrap_err();
    assert_eq!(err.message(), "too many elements in tuple");
    let err = deserialize::<Pair>(vec![Event::map_start()]).unwrap_err();
    assert_eq!(err.message(), "unexpected map, expected tuple");
}

#[test]
fn test_tuple_struct_shape() {
    #[derive(Serialize)]
    struct Triple(u32, u32, u32);

    let mut driver = SerializeDriver::new(&Triple(1, 2, 3));
    let (event, _, _) = driver.next().unwrap().unwrap();
    assert_eq!(
        event.to_static(),
        Event::SeqStart(deser::ContainerShape::new().with_len(3))
    );
}

#[test]
fn test_tuple_struct_generic() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Pair<A, B>(A, B);

    check(
        Pair(1u32, Some(true)),
        vec![Event::seq_start(), 1u64.into(), true.into(), Event::SeqEnd],
    );
}

#[test]
fn test_tuple_struct_adapters() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Addr(#[deser(as = DisplayFromStr)] std::net::IpAddr, u16);

    check(
        Addr("127.0.0.1".parse().unwrap(), 80),
        vec![
            Event::seq_start(),
            "127.0.0.1".into(),
            80u64.into(),
            Event::SeqEnd,
        ],
    );

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Generic<T>(#[deser(as = DisplayFromStr)] T, u16)
    where
        T: std::fmt::Display + std::str::FromStr,
        T::Err: std::fmt::Display;

    check(
        Generic(42u32, 1),
        vec![Event::seq_start(), "42".into(), 1u64.into(), Event::SeqEnd],
    );
}

#[test]
fn test_tuple_struct_borrowed() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Borrowed<'a>(&'a str, u32);

    let mut out = None::<Borrowed<'_>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::seq_start()).unwrap();
        driver
            .emit_borrowed(Event::Atom(Atom::Str("borrowed".into())))
            .unwrap();
        driver.emit(1u64).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
    }
    assert_eq!(out.unwrap(), Borrowed("borrowed", 1));
}

#[test]
fn test_tuple_struct_validate() {
    fn ordered(value: &Range) -> Result<(), &'static str> {
        if value.0 > value.1 {
            Err("start is after end")
        } else {
            Ok(())
        }
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(validate = ordered)]
    struct Range(u32, u32);

    assert_eq!(
        deserialize::<Range>(vec![
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd
        ])
        .unwrap(),
        Range(1, 2)
    );
    let err = deserialize::<Range>(vec![
        Event::seq_start(),
        2u64.into(),
        1u64.into(),
        Event::SeqEnd,
    ])
    .unwrap_err();
    assert_eq!(err.message(), "invalid value: start is after end");
}

#[test]
fn test_empty_tuple_struct() {
    // without fields tuple structs are like unit structs
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Empty();

    check(Empty(), vec![Atom::Null.into()]);
    assert_eq!(describe(&Empty()), ["unit struct Empty"]);
}

#[test]
fn test_unit_struct() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Unit;

    check(Unit, vec![Atom::Null.into()]);
    assert_eq!(describe(&Unit), ["unit struct Unit"]);
    // text of unknown type which is empty is null (like for `()`)
    assert_eq!(
        deserialize::<Unit>(vec![Atom::Lexical("".into()).into()]).unwrap(),
        Unit
    );
    let err = deserialize::<Unit>(vec![42u64.into()]).unwrap_err();
    assert_eq!(err.message(), "unexpected unsigned integer, expected Unit");
    let err = deserialize::<Unit>(vec![Event::seq_start()]).unwrap_err();
    assert_eq!(err.message(), "unexpected sequence, expected Unit");

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(rename = "Nothing")]
    struct Renamed;
    let err = deserialize::<Renamed>(vec![true.into()]).unwrap_err();
    assert_eq!(err.message(), "unexpected bool, expected Nothing");
}

#[test]
fn test_unit_struct_in_struct() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Marker;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Outer {
        marker: Marker,
        pair: (u32, Marker),
    }

    check(
        Outer {
            marker: Marker,
            pair: (1, Marker),
        },
        vec![
            Event::map_start(),
            "marker".into(),
            Atom::Null.into(),
            "pair".into(),
            Event::seq_start(),
            1u64.into(),
            Atom::Null.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
}

#[test]
fn test_unit_struct_const_generic() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Version<const N: u32>;

    check(Version::<2>, vec![Atom::Null.into()]);
}

#[test]
fn test_tuple_struct_phantom() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Tagged<T>(u32, PhantomData<T>);

    // `PhantomData` is null
    check(
        Tagged::<String>(1, PhantomData),
        vec![
            Event::seq_start(),
            1u64.into(),
            Atom::Null.into(),
            Event::SeqEnd,
        ],
    );
}

#[test]
fn test_tuple_struct_borrowed_validate() {
    fn not_empty(value: &Named<'_>) -> Result<(), &'static str> {
        if value.0.is_empty() {
            Err("empty name")
        } else {
            Ok(())
        }
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(validate = not_empty)]
    struct Named<'a>(&'a str, u32);

    let input = String::from("");
    let mut out = None::<Named<'_>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::seq_start()).unwrap();
    driver.emit_borrowed(input.as_str()).unwrap();
    driver.emit(1u64).unwrap();
    let err = driver.emit(Event::SeqEnd).unwrap_err();
    assert_eq!(err.message(), "invalid value: empty name");
}

#[test]
fn test_skipped_fields() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Pair(u32, #[deser(skip)] String, u32);

    check(
        Pair(1, String::new(), 2),
        vec![Event::seq_start(), 1u64.into(), 2u64.into(), Event::SeqEnd],
    );
    assert_eq!(
        serialize(&Pair(1, "x".into(), 2)),
        [Event::seq_start(), 1u64.into(), 2u64.into(), Event::SeqEnd]
    );

    // with one field that is not skipped, the struct is the value of that
    // field (like a newtype struct)
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Meters<U>(f64, #[deser(skip)] PhantomData<U>);

    check(Meters::<()>(1.5, PhantomData), vec![1.5f64.into()]);
    assert!(describe(&Meters::<()>(1.5, PhantomData)).is_empty());

    // explicit defaults
    #[derive(Debug, PartialEq, Deserialize)]
    struct Defaults(
        u32,
        #[deser(skip_deserializing, default = 42)] u32,
        #[deser(skip, default = "x")] String,
    );

    assert_eq!(
        deserialize::<Defaults>(vec![7u64.into()]).unwrap(),
        Defaults(7, 42, "x".into())
    );

    // skipped in one direction
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct OneWay(u32, #[deser(skip_serializing)] u32);

    assert_eq!(serialize(&OneWay(1, 2)), [1u64.into()]);
    assert_eq!(
        deserialize::<OneWay>(vec![
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd
        ])
        .unwrap(),
        OneWay(1, 2)
    );

    // skipped fields of generic types need a default, not `Deserialize`
    struct NotDeserializable;

    #[derive(Deserialize)]
    struct Generic<T>(u32, #[deser(skip, default = None)] Option<T>);

    let value = deserialize::<Generic<NotDeserializable>>(vec![1u64.into()]).unwrap();
    assert_eq!(value.0, 1);
    assert!(value.1.is_none());

    // all fields skipped
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Nothing(#[deser(skip)] u32);

    check(Nothing(0), vec![Atom::Null.into()]);
}
