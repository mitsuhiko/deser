use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned, IgnoredFields, Recording, UnknownFields};
use deser::{ContainerShape, Deserialize, Error, Event};

fn deserialize<T: DeserializeOwned>(
    policy: Option<UnknownFields>,
    events: Vec<Event<'_>>,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        if let Some(policy) = policy {
            *driver.state_mut().get_mut::<UnknownFields>() = policy;
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

/// The events of a recorded map, recordings know the length of maps.
fn recorded_map<'a>(pairs: &[(&'a str, Event<'a>)]) -> Vec<Event<'a>> {
    let mut events = map(pairs);
    events[0] = Event::MapStart(ContainerShape::new().with_len(pairs.len()));
    events
}

fn message<T: std::fmt::Debug>(rv: Result<T, Error>) -> String {
    rv.unwrap_err().message().to_string()
}

#[derive(Debug, Deserialize, PartialEq)]
struct Point {
    x: u32,
    y: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct StrictPoint {
    x: u32,
    y: u32,
}

#[test]
fn test_ignored_by_default() {
    let point: Point = deserialize(
        None,
        map(&[("x", 1u64.into()), ("z", 2u64.into()), ("y", 3u64.into())]),
    )
    .unwrap();
    assert_eq!(point, Point { x: 1, y: 3 });
}

#[test]
fn test_policy_error() {
    let rv = deserialize::<Point>(
        Some(UnknownFields::Error),
        map(&[("x", 1u64.into()), ("z", 2u64.into()), ("y", 3u64.into())]),
    );
    assert_eq!(message(rv), "unknown field `z`, expected `x` or `y`");

    // compound values of unknown keys are rejected before they start
    let mut events = map(&[("x", 1u64.into()), ("y", 3u64.into())]);
    events.splice(1..1, ["z".into(), Event::seq_start(), Event::SeqEnd]);
    let rv = deserialize::<Point>(Some(UnknownFields::Error), events);
    assert_eq!(message(rv), "unknown field `z`, expected `x` or `y`");
}

#[test]
fn test_attribute() {
    let rv = deserialize::<StrictPoint>(
        None,
        map(&[("x", 1u64.into()), ("z", 2u64.into()), ("y", 3u64.into())]),
    );
    assert_eq!(message(rv), "unknown field `z`, expected `x` or `y`");

    // the attribute wins over the policy
    let ignored = IgnoredFields::new();
    let rv = deserialize::<StrictPoint>(
        Some(UnknownFields::Collect(ignored.clone())),
        map(&[("x", 1u64.into()), ("z", 2u64.into()), ("y", 3u64.into())]),
    );
    assert_eq!(message(rv), "unknown field `z`, expected `x` or `y`");
    assert!(ignored.is_empty());

    let point: StrictPoint =
        deserialize(None, map(&[("x", 1u64.into()), ("y", 3u64.into())])).unwrap();
    assert_eq!(point, StrictPoint { x: 1, y: 3 });
}

#[test]
fn test_collect() {
    let ignored = IgnoredFields::new();
    let point: Point = deserialize(
        Some(UnknownFields::Collect(ignored.clone())),
        map(&[
            ("x", 1u64.into()),
            ("z", 2u64.into()),
            ("y", 3u64.into()),
            ("w", 4u64.into()),
        ]),
    )
    .unwrap();
    assert_eq!(point, Point { x: 1, y: 3 });
    let ignored = ignored.take();
    let messages = ignored.iter().map(|x| x.message()).collect::<Vec<_>>();
    assert_eq!(
        messages,
        [
            "unknown field `z`, expected `x` or `y`",
            "unknown field `w`, expected `x` or `y`"
        ]
    );
}

#[derive(Debug, Deserialize, PartialEq)]
struct Inner {
    a: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct Outer {
    b: u32,
    #[deser(flatten)]
    inner: Inner,
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct Nested {
    c: u32,
    #[deser(flatten)]
    outer: Outer,
}

#[test]
fn test_flatten() {
    // the keys of flattened fields are known (serde#1547, serde#2384)
    let outer: Outer = deserialize(None, map(&[("a", 1u64.into()), ("b", 2u64.into())])).unwrap();
    assert_eq!(
        outer,
        Outer {
            b: 2,
            inner: Inner { a: 1 }
        }
    );
    let rv = deserialize::<Outer>(
        None,
        map(&[("a", 1u64.into()), ("x", 2u64.into()), ("b", 2u64.into())]),
    );
    // flattened fields can take any key, the fields are not listed
    assert_eq!(message(rv), "unknown field `x`");

    let nested: Nested = deserialize(
        None,
        map(&[("a", 1u64.into()), ("b", 2u64.into()), ("c", 3u64.into())]),
    )
    .unwrap();
    assert_eq!(nested.outer.inner.a, 1);
    let rv = deserialize::<Nested>(
        None,
        map(&[
            ("a", 1u64.into()),
            ("b", 2u64.into()),
            ("c", 3u64.into()),
            ("d", 4u64.into()),
        ]),
    );
    assert_eq!(message(rv), "unknown field `d`");
}

#[derive(Debug, Deserialize, PartialEq)]
struct WithExtra {
    a: u32,
    #[deser(flatten)]
    extra: BTreeMap<String, u32>,
}

#[test]
fn test_flattened_map_takes_everything() {
    let value: WithExtra = deserialize(
        Some(UnknownFields::Error),
        map(&[("a", 1u64.into()), ("b", 2u64.into())]),
    )
    .unwrap();
    assert_eq!(value.extra.get("b"), Some(&2));
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "type", deny_unknown_fields)]
enum Shape {
    Circle { radius: u32 },
    Empty,
    Wrapped(Point),
}

#[test]
fn test_internally_tagged() {
    // the tag is not an unknown field (serde#2666)
    let shape: Shape = deserialize(
        None,
        map(&[("radius", 1u64.into()), ("type", "Circle".into())]),
    )
    .unwrap();
    assert_eq!(shape, Shape::Circle { radius: 1 });
    let rv = deserialize::<Shape>(
        None,
        map(&[
            ("radius", 1u64.into()),
            ("type", "Circle".into()),
            ("x", 1u64.into()),
        ]),
    );
    assert_eq!(message(rv), "unknown field `x`, expected `radius`");

    // keys before the tag are recorded
    let rv = deserialize::<Shape>(None, map(&[("x", 1u64.into()), ("type", "Circle".into())]));
    assert_eq!(message(rv), "unknown field `x`, expected `radius`");

    // unit variants (serde#2294)
    assert_eq!(
        deserialize::<Shape>(None, map(&[("type", "Empty".into())])).unwrap(),
        Shape::Empty
    );
    let rv = deserialize::<Shape>(None, map(&[("type", "Empty".into()), ("x", 1u64.into())]));
    assert_eq!(message(rv), "unknown field `x`, there are no fields");

    // newtype variants are up to the inner type
    assert_eq!(
        deserialize::<Shape>(
            None,
            map(&[
                ("type", "Wrapped".into()),
                ("x", 1u64.into()),
                ("y", 2u64.into()),
                ("z", 3u64.into())
            ])
        )
        .unwrap(),
        Shape::Wrapped(Point { x: 1, y: 2 })
    );
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "kind")]
enum Kind {
    A { a: u32 },
    B { b: u32 },
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deny_unknown_fields)]
struct Holder {
    id: u32,
    #[deser(flatten)]
    kind: Kind,
}

#[test]
fn test_flattened_internally_tagged() {
    // serde#1358: the keys of the variant are known
    let holder: Holder = deserialize(
        None,
        map(&[
            ("a", 1u64.into()),
            ("id", 2u64.into()),
            ("kind", "A".into()),
        ]),
    )
    .unwrap();
    assert_eq!(
        holder,
        Holder {
            id: 2,
            kind: Kind::A { a: 1 }
        }
    );
    let holder: Holder = deserialize(
        None,
        map(&[
            ("kind", "B".into()),
            ("id", 2u64.into()),
            ("b", 1u64.into()),
        ]),
    )
    .unwrap();
    assert_eq!(holder.kind, Kind::B { b: 1 });

    // keys before the tag that the variant does not take
    let rv = deserialize::<Holder>(
        None,
        map(&[
            ("x", 1u64.into()),
            ("kind", "A".into()),
            ("a", 1u64.into()),
            ("id", 2u64.into()),
        ]),
    );
    assert_eq!(message(rv), "unknown field `x`");
    // and after the tag
    let rv = deserialize::<Holder>(
        None,
        map(&[
            ("kind", "A".into()),
            ("a", 1u64.into()),
            ("id", 2u64.into()),
            ("x", 1u64.into()),
        ]),
    );
    assert_eq!(message(rv), "unknown field `x`");

    // the policy applies the same way
    let ignored = IgnoredFields::new();
    #[derive(Debug, Deserialize)]
    struct LaxHolder {
        #[allow(unused)]
        id: u32,
        #[deser(flatten)]
        #[allow(unused)]
        kind: Kind,
    }
    deserialize::<LaxHolder>(
        Some(UnknownFields::Collect(ignored.clone())),
        map(&[
            ("x", 1u64.into()),
            ("kind", "A".into()),
            ("a", 1u64.into()),
            ("y", 1u64.into()),
            ("id", 2u64.into()),
        ]),
    )
    .unwrap();
    let ignored = ignored.take();
    let mut messages = ignored.iter().map(|x| x.message()).collect::<Vec<_>>();
    messages.sort();
    assert_eq!(messages, ["unknown field `x`", "unknown field `y`"]);
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "t", content = "c", deny_unknown_fields)]
enum Adjacent {
    A(u32),
    B { b: u32 },
}

#[test]
fn test_adjacently_tagged() {
    assert_eq!(
        deserialize::<Adjacent>(None, map(&[("t", "A".into()), ("c", 1u64.into())])).unwrap(),
        Adjacent::A(1)
    );
    let rv = deserialize::<Adjacent>(
        None,
        map(&[("t", "A".into()), ("x", 1u64.into()), ("c", 1u64.into())]),
    );
    assert_eq!(message(rv), "unknown field `x`, expected `t` or `c`");
    let mut events = vec![Event::map_start(), "t".into(), "B".into(), "c".into()];
    events.extend(map(&[("b", 1u64.into()), ("x", 1u64.into())]));
    events.push(Event::MapEnd);
    let rv = deserialize::<Adjacent>(None, events);
    assert_eq!(message(rv), "unknown field `x`, expected `b`");
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(untagged, deny_unknown_fields)]
enum Untagged {
    Empty {},
    Point { x: u32, y: u32 },
}

#[test]
fn test_untagged() {
    // strict variants do not match maps with other keys (serde#1560)
    assert_eq!(
        deserialize::<Untagged>(None, map(&[("x", 1u64.into()), ("y", 2u64.into())])).unwrap(),
        Untagged::Point { x: 1, y: 2 }
    );
    assert_eq!(
        deserialize::<Untagged>(None, map(&[])).unwrap(),
        Untagged::Empty {}
    );
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(untagged)]
enum LooseUntagged {
    Flag(bool),
    Point(Point),
}

#[test]
fn test_untagged_collect() {
    // the errors of failed variants are thrown away, collected ones are kept
    let ignored = IgnoredFields::new();
    let value: LooseUntagged = deserialize(
        Some(UnknownFields::Collect(ignored.clone())),
        map(&[("x", 1u64.into()), ("z", 2u64.into()), ("y", 3u64.into())]),
    )
    .unwrap();
    assert_eq!(value, LooseUntagged::Point(Point { x: 1, y: 3 }));
    let ignored = ignored.take();
    let messages = ignored.iter().map(|x| x.message()).collect::<Vec<_>>();
    assert_eq!(messages, ["unknown field `z`, expected `x` or `y`"]);
}

#[derive(Debug, Deserialize)]
#[deser(tag = "type")]
enum Passthrough {
    #[allow(unused)]
    Known { a: u32 },
    #[deser(other)]
    #[allow(unused)]
    Unknown(#[deser(tag)] String, Recording),
}

#[test]
fn test_other_variants() {
    // other variants with a recording take everything
    let value: Passthrough = deserialize(
        Some(UnknownFields::Error),
        map(&[("type", "X".into()), ("b", 1u64.into())]),
    )
    .unwrap();
    assert!(matches!(value, Passthrough::Unknown(ref tag, _) if tag == "X"));
}

#[derive(Debug, Deserialize)]
#[deser(deny_unknown_fields)]
struct PassthroughHolder {
    id: u32,
    #[deser(flatten)]
    kind: Passthrough,
}

#[derive(Debug, Deserialize)]
#[deser(deny_unknown_fields)]
struct WithRest {
    id: u32,
    #[deser(flatten)]
    rest: Recording,
}

#[test]
fn test_flattened_recordings() {
    // recordings take all keys when flattened, before and after the tag
    let value: PassthroughHolder = deserialize(
        None,
        map(&[
            ("x", 1u64.into()),
            ("type", "X".into()),
            ("y", 2u64.into()),
            ("id", 3u64.into()),
        ]),
    )
    .unwrap();
    assert_eq!(value.id, 3);
    let Passthrough::Unknown(tag, content) = value.kind else {
        panic!("expected the other variant");
    };
    assert_eq!(tag, "X");
    assert_eq!(
        content.events().cloned().collect::<Vec<_>>(),
        recorded_map(&[("x", 1u64.into()), ("y", 2u64.into())])
    );

    let value: WithRest = deserialize(
        None,
        map(&[("x", 1u64.into()), ("id", 3u64.into()), ("y", 2u64.into())]),
    )
    .unwrap();
    assert_eq!(value.id, 3);
    assert_eq!(
        value.rest.events().cloned().collect::<Vec<_>>(),
        recorded_map(&[("x", 1u64.into()), ("y", 2u64.into())])
    );

    // without keys the recording is an empty map
    let value: WithRest = deserialize(None, map(&[("id", 3u64.into())])).unwrap();
    assert_eq!(
        value.rest.events().cloned().collect::<Vec<_>>(),
        recorded_map(&[])
    );
}

#[test]
fn test_variant_deny_unknown_fields() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "type")]
    enum Message {
        #[deser(deny_unknown_fields)]
        Strict {
            a: u32,
        },
        Lenient {
            a: u32,
        },
        #[deser(deny_unknown_fields)]
        Empty,
    }

    let rv = deserialize::<Message>(
        None,
        map(&[
            ("type", "Strict".into()),
            ("a", 1u64.into()),
            ("b", 1u64.into()),
        ]),
    );
    assert_eq!(message(rv), "unknown field `b`, expected `a`");
    assert_eq!(
        deserialize::<Message>(
            None,
            map(&[
                ("type", "Lenient".into()),
                ("a", 1u64.into()),
                ("b", 1u64.into()),
            ]),
        )
        .unwrap(),
        Message::Lenient { a: 1 }
    );
    let rv = deserialize::<Message>(None, map(&[("type", "Empty".into()), ("x", 1u64.into())]));
    assert_eq!(message(rv), "unknown field `x`, there are no fields");

    #[derive(Debug, Deserialize, PartialEq)]
    enum External {
        #[deser(deny_unknown_fields)]
        Strict { a: u32 },
    }

    let rv = deserialize::<External>(
        None,
        vec![
            Event::map_start(),
            "Strict".into(),
            Event::map_start(),
            "c".into(),
            1u64.into(),
        ],
    );
    assert_eq!(message(rv), "unknown field `c`, expected `a`");
}
