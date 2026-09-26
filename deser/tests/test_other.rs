use std::collections::BTreeMap;
use std::fmt::Debug;

use deser::adapters::DisplayFromStr;
use deser::de::{DeserializeDriver, DeserializeOwned, Recording};
use deser::ser::SerializeDriver;
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize};

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
    let serialized = serialize(&value);
    assert_eq!(
        serialized,
        events.iter().map(|x| x.to_static()).collect::<Vec<_>>()
    );
    assert_eq!(deserialize::<T>(events).unwrap(), value);
}

fn recorded(events: Vec<Event<'_>>) -> Vec<Event<'static>> {
    events.iter().map(|x| x.to_static()).collect()
}

fn events_of(recording: &Recording) -> Vec<Event<'static>> {
    recording.events().cloned().collect()
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum SimpleWithOther {
    A,
    #[deser(other)]
    Unknown,
}

#[test]
fn test_simple_enum_non_string_tags() {
    assert_eq!(
        deserialize::<SimpleWithOther>(vec![42u64.into()]).unwrap(),
        SimpleWithOther::Unknown
    );
    assert_eq!(
        deserialize::<SimpleWithOther>(vec!["A".into()]).unwrap(),
        SimpleWithOther::A
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ExternalTag {
    Unit,
    Newtype(Option<u32>),
    #[deser(other)]
    Other(#[deser(tag)] String),
}

#[test]
fn test_external_tag_only() {
    check(ExternalTag::Unit, vec!["Unit".into()]);
    check(ExternalTag::Other("foo".into()), vec!["foo".into()]);
    // the content is ignored
    assert_eq!(
        deserialize::<ExternalTag>(vec![
            Event::map_start(),
            "foo".into(),
            Event::seq_start(),
            1u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ])
        .unwrap(),
        ExternalTag::Other("foo".into())
    );
    // the name of the other variant is an unknown tag too
    assert_eq!(
        deserialize::<ExternalTag>(vec!["Other".into()]).unwrap(),
        ExternalTag::Other("Other".into())
    );
    // known variants with content receive null as content if they are
    // represented as string
    assert_eq!(
        deserialize::<ExternalTag>(vec!["Newtype".into()]).unwrap(),
        ExternalTag::Newtype(None)
    );
    // the tag field only accepts strings
    let err = deserialize::<ExternalTag>(vec![42u64.into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ExternalCapture {
    Known(u32),
    #[deser(other)]
    Other(#[deser(tag)] String, Recording),
}

#[test]
fn test_external_capture() {
    check(
        ExternalCapture::Known(1),
        vec![
            Event::map_start(),
            "Known".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );

    let value = deserialize::<ExternalCapture>(vec![
        Event::map_start(),
        "foo".into(),
        Event::seq_start(),
        1u64.into(),
        2u64.into(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    match value {
        ExternalCapture::Other(ref tag, ref content) => {
            assert_eq!(tag, "foo");
            assert_eq!(
                events_of(content),
                recorded(vec![
                    Event::seq_start(),
                    1u64.into(),
                    2u64.into(),
                    Event::SeqEnd
                ])
            );
        }
        _ => panic!("expected other"),
    }
    // round trips
    assert_eq!(
        serialize(&value),
        vec![
            Event::map_start(),
            "foo".into(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ]
    );

    // the string form receives null as content
    let value = deserialize::<ExternalCapture>(vec!["bar".into()]).unwrap();
    match value {
        ExternalCapture::Other(ref tag, ref content) => {
            assert_eq!(tag, "bar");
            assert_eq!(events_of(content), recorded(vec![().into()]));
        }
        _ => panic!("expected other"),
    }

    // known tags with bad content are errors
    let err = deserialize::<ExternalCapture>(vec![
        Event::map_start(),
        "Known".into(),
        "x".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum NumericTags {
    #[deser(rename = "1")]
    One,
    #[deser(other)]
    Other(#[deser(tag)] u64),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum NumericTagsWithContent {
    #[deser(other)]
    Other {
        #[deser(tag, as = DisplayFromStr)]
        code: u32,
        value: String,
    },
}

#[test]
fn test_non_string_tags() {
    check(NumericTags::One, vec!["1".into()]);
    check(NumericTags::Other(2), vec![2u64.into()]);
    // lexical keys (like in JSON) are accepted as numbers
    assert_eq!(
        deserialize::<NumericTags>(vec![
            Event::map_start(),
            Atom::Lexical("3".into()).into(),
            ().into(),
            Event::MapEnd
        ])
        .unwrap(),
        NumericTags::Other(3)
    );
    assert_eq!(
        deserialize::<NumericTags>(vec![
            Event::map_start(),
            4u64.into(),
            ().into(),
            Event::MapEnd
        ])
        .unwrap(),
        NumericTags::Other(4)
    );

    check(
        NumericTagsWithContent::Other {
            code: 42,
            value: "x".into(),
        },
        vec![
            Event::map_start(),
            "42".into(),
            Event::map_start(),
            "value".into(),
            "x".into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type", rename_all = "snake_case")]
enum Event_ {
    Click {
        x: u32,
    },
    #[deser(other)]
    Unknown {
        #[deser(tag)]
        kind: String,
        x: Option<u32>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum InternalCapture {
    Known {
        a: u32,
    },
    #[deser(other)]
    Other(#[deser(tag)] String, BTreeMap<String, u32>),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum InternalRaw {
    #[deser(other)]
    Other(#[deser(tag)] String, Recording),
}

#[test]
fn test_internally_tagged() {
    check(
        Event_::Click { x: 1 },
        vec![
            Event::map_start(),
            "type".into(),
            "click".into(),
            "x".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        Event_::Unknown {
            kind: "scroll".into(),
            x: Some(1),
        },
        vec![
            Event::map_start(),
            "type".into(),
            "scroll".into(),
            "x".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    // the tag can come last
    assert_eq!(
        deserialize::<Event_>(vec![
            Event::map_start(),
            "y".into(),
            1u64.into(),
            "type".into(),
            "scroll".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Event_::Unknown {
            kind: "scroll".into(),
            x: None,
        }
    );

    let mut map = BTreeMap::new();
    map.insert("a".to_string(), 1);
    map.insert("b".to_string(), 2);
    check(
        InternalCapture::Other("foo".into(), map),
        vec![
            Event::map_start(),
            "type".into(),
            "foo".into(),
            "a".into(),
            1u64.into(),
            "b".into(),
            2u64.into(),
            Event::MapEnd,
        ],
    );

    let events = vec![
        Event::map_start(),
        "type".into(),
        "foo".into(),
        "a".into(),
        Event::seq_start(),
        true.into(),
        Event::SeqEnd,
        Event::MapEnd,
    ];
    let value = deserialize::<InternalRaw>(events.clone()).unwrap();
    assert_eq!(serialize(&value), recorded(events));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type", rename_all = "snake_case")]
enum Bind {
    #[deser(default)]
    Http {
        address: String,
    },
    Tls {
        address: String,
        cert: String,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum OtherOrDefault {
    A,
    #[deser(other, default)]
    Unknown(#[deser(tag)] Option<String>),
}

#[test]
fn test_default_variant() {
    assert_eq!(
        deserialize::<Bind>(vec![
            Event::map_start(),
            "address".into(),
            "0.0.0.0:80".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Bind::Http {
            address: "0.0.0.0:80".into()
        }
    );
    check(
        Bind::Tls {
            address: "0.0.0.0:443".into(),
            cert: "cert.pem".into(),
        },
        vec![
            Event::map_start(),
            "type".into(),
            "tls".into(),
            "address".into(),
            "0.0.0.0:443".into(),
            "cert".into(),
            "cert.pem".into(),
            Event::MapEnd,
        ],
    );
    // unknown tags are still errors
    let err = deserialize::<Bind>(vec![
        Event::map_start(),
        "type".into(),
        "quic".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unknown variant 'quic' for Bind"
    );

    assert_eq!(
        deserialize::<OtherOrDefault>(vec![Event::map_start(), Event::MapEnd]).unwrap(),
        OtherOrDefault::Unknown(None)
    );
    check(
        OtherOrDefault::Unknown(Some("b".into())),
        vec![Event::map_start(), "type".into(), "b".into(), Event::MapEnd],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum Adjacent {
    Known(u32),
    #[deser(default)]
    Nothing,
    #[deser(other)]
    Other(#[deser(tag)] String, Option<Recording>),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum AdjacentDefaultContent {
    #[deser(default)]
    Value(u32),
}

#[test]
fn test_adjacently_tagged() {
    check(
        Adjacent::Known(1),
        vec![
            Event::map_start(),
            "t".into(),
            "Known".into(),
            "c".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    assert_eq!(
        deserialize::<Adjacent>(vec![Event::map_start(), Event::MapEnd]).unwrap(),
        Adjacent::Nothing
    );
    check(
        Adjacent::Other("foo".into(), None),
        vec![
            Event::map_start(),
            "t".into(),
            "foo".into(),
            "c".into(),
            ().into(),
            Event::MapEnd,
        ],
    );
    let value = deserialize::<Adjacent>(vec![
        Event::map_start(),
        "c".into(),
        "x".into(),
        "t".into(),
        "foo".into(),
        Event::MapEnd,
    ])
    .unwrap();
    match value {
        Adjacent::Other(ref tag, Some(ref content)) => {
            assert_eq!(tag, "foo");
            assert_eq!(events_of(content), recorded(vec!["x".into()]));
        }
        _ => panic!("expected other"),
    }
    // missing content
    assert_eq!(
        deserialize::<Adjacent>(vec![
            Event::map_start(),
            "t".into(),
            "foo".into(),
            Event::MapEnd
        ])
        .unwrap(),
        Adjacent::Other("foo".into(), None)
    );

    assert_eq!(
        deserialize::<AdjacentDefaultContent>(vec![
            Event::map_start(),
            "c".into(),
            42u64.into(),
            Event::MapEnd
        ])
        .unwrap(),
        AdjacentDefaultContent::Value(42)
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "kebab-case")]
enum Program {
    Bash(BTreeMap<String, String>),
    #[deser(other)]
    Other(#[deser(tag)] String, BTreeMap<String, String>),
}

#[test]
fn test_program_config() {
    let mut config = BTreeMap::new();
    config.insert("theme".to_string(), "dark".to_string());
    check(
        vec![
            Program::Bash(BTreeMap::new()),
            Program::Other("vim".into(), config),
        ],
        vec![
            Event::seq_start(),
            Event::map_start(),
            "bash".into(),
            Event::MapStart(deser::ContainerShape::new().with_order(deser::Order::Sorted)),
            Event::MapEnd,
            Event::MapEnd,
            Event::map_start(),
            "vim".into(),
            Event::MapStart(deser::ContainerShape::new().with_order(deser::Order::Sorted)),
            "theme".into(),
            "dark".into(),
            Event::MapEnd,
            Event::MapEnd,
            Event::SeqEnd,
        ],
    );
}
