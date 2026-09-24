use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, Recording};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Error, ErrorKind, Event, Serialize};

fn deserialize<T: Deserialize>(events: Vec<Event<'_>>) -> Result<T, Error> {
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
        events.push(event.to_static());
    }
    events
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[deser(tag = "type", rename_all = "snake_case")]
enum Shape {
    Circle {
        radius: u32,
    },
    Rect {
        width: u32,
        #[deser(rename = "h", alias = "height")]
        height: u32,
        #[deser(default)]
        label: String,
    },
    #[deser(alias = "nothing")]
    Empty,
    Polygon {
        points: Vec<(i32, i32)>,
        attrs: BTreeMap<String, Vec<String>>,
    },
}

#[test]
fn test_tag_first() {
    let shape: Shape = deserialize(vec![
        Event::MapStart,
        "type".into(),
        "circle".into(),
        "radius".into(),
        42u64.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(shape, Shape::Circle { radius: 42 });
}

#[test]
fn test_tag_last() {
    let shape: Shape = deserialize(vec![
        Event::MapStart,
        "height".into(),
        2u64.into(),
        "unknown".into(),
        Event::SeqStart,
        true.into(),
        Event::SeqEnd,
        "width".into(),
        1u64.into(),
        "type".into(),
        "rect".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        shape,
        Shape::Rect {
            width: 1,
            height: 2,
            label: "".into()
        }
    );
}

#[test]
fn test_tag_in_the_middle_with_compound_values() {
    let shape: Shape = deserialize(vec![
        Event::MapStart,
        "points".into(),
        Event::SeqStart,
        Event::SeqStart,
        1i64.into(),
        (-1i64).into(),
        Event::SeqEnd,
        Event::SeqEnd,
        "type".into(),
        "polygon".into(),
        "attrs".into(),
        Event::MapStart,
        "a".into(),
        Event::SeqStart,
        "x".into(),
        Event::SeqEnd,
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    let mut attrs = BTreeMap::new();
    attrs.insert("a".to_string(), vec!["x".to_string()]);
    assert_eq!(
        shape,
        Shape::Polygon {
            points: vec![(1, -1)],
            attrs
        }
    );
}

#[test]
fn test_unit_variant() {
    let shape: Shape = deserialize(vec![
        Event::MapStart,
        "type".into(),
        "nothing".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(shape, Shape::Empty);
}

#[test]
fn test_errors() {
    let err = deserialize::<Shape>(vec![
        Event::MapStart,
        "radius".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
    assert_eq!(err.to_string(), "MissingField: missing tag 'type'");

    let err = deserialize::<Shape>(vec![
        Event::MapStart,
        "type".into(),
        "triangle".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unknown variant 'triangle' for Shape"
    );

    let err = deserialize::<Shape>(vec![
        Event::MapStart,
        "type".into(),
        "circle".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.to_string(), "MissingField: Missing field 'radius'");

    let err = deserialize::<Shape>(vec!["circle".into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);

    // errors in buffered values are reported when they are replayed
    let err = deserialize::<Shape>(vec![
        Event::MapStart,
        "radius".into(),
        "not a number".into(),
        "type".into(),
        "circle".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
}

#[test]
fn test_serialize() {
    assert_eq!(
        serialize(&Shape::Rect {
            width: 1,
            height: 2,
            label: "x".into()
        }),
        vec![
            Event::MapStart,
            "type".into(),
            "rect".into(),
            "width".into(),
            1u64.into(),
            "h".into(),
            2u64.into(),
            "label".into(),
            "x".into(),
            Event::MapEnd,
        ]
    );
    assert_eq!(
        serialize(&Shape::Empty),
        vec![
            Event::MapStart,
            "type".into(),
            "empty".into(),
            Event::MapEnd
        ]
    );

    // roundtrip
    let shape = Shape::Circle { radius: 3 };
    assert_eq!(deserialize::<Shape>(serialize(&shape)).unwrap(), shape);
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[deser(tag = "kind", skip_serializing_optionals)]
enum WithOptionals {
    Item {
        name: String,
        comment: Option<String>,
        #[deser(skip_serializing_if = "is_zero")]
        count: u32,
    },
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[test]
fn test_serialize_skipping() {
    assert_eq!(
        serialize(&WithOptionals::Item {
            name: "x".into(),
            comment: None,
            count: 0,
        }),
        vec![
            Event::MapStart,
            "kind".into(),
            "Item".into(),
            "name".into(),
            "x".into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_recording() {
    let mut recording = Recording::new();
    assert!(recording.is_empty());
    {
        let mut driver = DeserializeDriver::from_sink(recording.recorder());
        driver.emit("hello").unwrap();
    }
    assert_eq!(recording.as_str(), Some("hello"));

    // recording again replaces the value
    {
        let mut driver = DeserializeDriver::from_sink(recording.recorder());
        driver.emit(Event::MapStart).unwrap();
        driver.emit("a").unwrap();
        driver.emit(Event::SeqStart).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    assert_eq!(recording.as_str(), None);
    assert_eq!(
        recording.events().cloned().collect::<Vec<_>>(),
        vec![
            Event::MapStart,
            "a".into(),
            Event::SeqStart,
            Event::SeqEnd,
            Event::MapEnd
        ]
    );

    // a recording can be replayed multiple times
    let mut driver_out = None::<()>;
    let driver = DeserializeDriver::new(&mut driver_out);
    for _ in 0..2 {
        let mut out = None::<BTreeMap<String, Vec<u32>>>;
        recording
            .replay(Deserialize::deserialize_into(&mut out), driver.state())
            .unwrap();
        assert_eq!(out.unwrap()["a"], Vec::<u32>::new());
    }
}
