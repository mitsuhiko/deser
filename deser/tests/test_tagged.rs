use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned, Recording};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Error, ErrorKind, Event, Serialize};

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
        Event::map_start(),
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
        Event::map_start(),
        "height".into(),
        2u64.into(),
        "unknown".into(),
        Event::seq_start(),
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
        Event::map_start(),
        "points".into(),
        Event::seq_start(),
        Event::seq_start(),
        1i64.into(),
        (-1i64).into(),
        Event::SeqEnd,
        Event::SeqEnd,
        "type".into(),
        "polygon".into(),
        "attrs".into(),
        Event::map_start(),
        "a".into(),
        Event::seq_start(),
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
        Event::map_start(),
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
        Event::map_start(),
        "radius".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
    assert_eq!(err.to_string(), "MissingField: missing tag 'type'");

    let err = deserialize::<Shape>(vec![
        Event::map_start(),
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
        Event::map_start(),
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
        Event::map_start(),
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
            Event::map_start(),
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
            Event::map_start(),
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
        #[deser(skip_serializing_if = is_zero)]
        count: u32,
    },
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, PartialEq, Deserialize)]
#[deser(tag = "kind")]
enum WithDefaults {
    Server {
        #[deser(default = "localhost")]
        host: String,
        #[deser(default = 8000 + 80)]
        port: u16,
    },
}

#[test]
fn test_variant_expression_defaults() {
    assert_eq!(
        deserialize::<WithDefaults>(vec![
            Event::map_start(),
            "kind".into(),
            "Server".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        WithDefaults::Server {
            host: "localhost".into(),
            port: 8080,
        }
    );
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
            Event::map_start(),
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
        driver.emit(Event::map_start()).unwrap();
        driver.emit("a").unwrap();
        driver.emit(Event::seq_start()).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    assert_eq!(recording.as_str(), None);
    assert_eq!(
        recording.events().cloned().collect::<Vec<_>>(),
        vec![
            Event::map_start(),
            "a".into(),
            Event::seq_start(),
            Event::SeqEnd,
            Event::MapEnd
        ]
    );

    // a recording can be replayed multiple times
    let mut driver_out = None::<()>;
    let mut driver = DeserializeDriver::new(&mut driver_out);
    for _ in 0..2 {
        let mut out = None::<BTreeMap<String, Vec<u32>>>;
        recording
            .replay(Deserialize::deserialize_into(&mut out), driver.state_mut())
            .unwrap();
        assert_eq!(out.unwrap()["a"], Vec::<u32>::new());
    }
}

/// Captures the state that its sink observes.
#[derive(Debug, PartialEq)]
struct Probe {
    depth: usize,
    shape: deser::ContainerShape,
}

deser::make_slot_wrapper!(ProbeSlot);

impl<'de> deser::de::Sink<'de> for ProbeSlot<Probe> {
    fn atom(&mut self, _atom: deser::Atom, state: &mut deser::State) -> Result<(), Error> {
        **self = Some(Probe {
            depth: state.depth(),
            shape: state.container_shape(),
        });
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Probe {
    fn deserialize_into(out: &mut Option<Self>) -> deser::de::SinkHandle<'_, 'de> {
        ProbeSlot::make_handle(out)
    }
}

#[derive(Debug, Deserialize)]
#[deser(tag = "type")]
enum Probed {
    Variant { value: Probe },
}

#[test]
fn test_replay_keeps_state() {
    // the tag comes first, the value is not buffered
    let direct: Vec<Probed> = deserialize(vec![
        Event::seq_start(),
        Event::map_start(),
        "type".into(),
        "Variant".into(),
        "value".into(),
        1u64.into(),
        Event::MapEnd,
        Event::SeqEnd,
    ])
    .unwrap();
    // the tag comes last, the value is recorded and replayed
    let replayed: Vec<Probed> = deserialize(vec![
        Event::seq_start(),
        Event::map_start(),
        "value".into(),
        1u64.into(),
        "type".into(),
        "Variant".into(),
        Event::MapEnd,
        Event::SeqEnd,
    ])
    .unwrap();

    let Probed::Variant { value: direct } = &direct[0];
    let Probed::Variant { value: replayed } = &replayed[0];
    assert_eq!(direct.depth, 2);
    assert_eq!(replayed, direct);
}
