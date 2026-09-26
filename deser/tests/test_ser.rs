use std::borrow::Cow;
use std::collections::BTreeSet;

use deser::ser::SerializeDriver;
use deser::{Atom, Event, Serialize};

fn capture_events(s: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(s);
    while let Some((event, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

#[test]
fn test_optional() {
    let events = capture_events(&None::<usize>);
    assert_eq!(events, vec![Event::Atom(Atom::Null)]);

    let events = capture_events(&Some(42usize));
    assert_eq!(events, vec![Event::Atom(Atom::U64(42))]);
}

#[test]
fn test_tuples() {
    let events = capture_events(&(1, 2, 3));
    assert_eq!(
        events,
        vec![
            Event::seq_start(),
            1i64.into(),
            2i64.into(),
            3i64.into(),
            Event::SeqEnd,
        ]
    );
}

#[test]
fn test_unit() {
    let events = capture_events(&());
    assert_eq!(events, vec![Event::Atom(Atom::Null)]);
}

#[test]
fn test_array() {
    let events = capture_events(&[1i64, 2, 3, 4]);
    assert_eq!(
        events,
        vec![
            Event::seq_start(),
            1i64.into(),
            2i64.into(),
            3i64.into(),
            4i64.into(),
            Event::SeqEnd
        ]
    );

    let events = capture_events(b"Hello");
    assert_eq!(
        events,
        vec![Event::Atom(Atom::Bytes(deser::Bytes::new(Cow::Borrowed(
            &b"Hello"[..]
        ))))]
    );
}

#[test]
fn test_chars() {
    let events = capture_events(&'x');
    assert_eq!(events, vec!['x'.into()]);
}

#[test]
fn test_refs() {
    let events = capture_events(&&&&42u64);
    assert_eq!(events, vec![42u64.into()]);
}

#[test]
fn test_box() {
    let events = capture_events(&Box::new(true));
    assert_eq!(events, vec![true.into()]);
}

#[test]
fn test_set() {
    let mut set = BTreeSet::new();
    set.insert("foo");
    set.insert("bar");
    let events = capture_events(&set);
    assert_eq!(
        events,
        vec![
            Event::SeqStart(deser::ContainerShape::new().with_order(deser::Order::Sorted)),
            "bar".into(),
            "foo".into(),
            Event::SeqEnd
        ]
    );
}

#[test]
fn test_shape_forwarding() {
    fn top_shape(s: &dyn Serialize) -> Option<deser::ContainerShape> {
        let mut driver = SerializeDriver::new(s);
        match driver.next().unwrap() {
            Some((Event::MapStart(shape) | Event::SeqStart(shape), _)) => Some(shape),
            _ => None,
        }
    }

    let mut map = std::collections::HashMap::new();
    map.insert(1u32, 2u32);
    let arbitrary = deser::ContainerShape::new().with_order(deser::Order::Arbitrary);
    assert_eq!(top_shape(&map), Some(arbitrary));
    assert_eq!(top_shape(&&map), Some(arbitrary));
    assert_eq!(top_shape(&Box::new(&map)), Some(arbitrary));
    assert_eq!(top_shape(&Some(&map)), Some(arbitrary));
    assert_eq!(top_shape(&None::<u32>), None);
    assert_eq!(top_shape(&vec![1u32]), Some(deser::ContainerShape::new()));

    assert!(Serialize::is_optional(&&None::<u32>));
    assert!(Serialize::is_optional(&Box::new(None::<u32>)));
    assert!(!Serialize::is_optional(&Box::new(Some(1u32))));
}

#[test]
fn test_is_map_key() {
    #[derive(Serialize)]
    struct Item {
        id: u32,
        tags: std::collections::BTreeMap<u32, (u32, u32)>,
    }

    let item = Item {
        id: 1,
        tags: [(2, (3, 4))].into_iter().collect(),
    };
    let expected = vec![
        (Event::map_start(), false),
        ("id".into(), true),
        (1u64.into(), false),
        ("tags".into(), true),
        (
            Event::MapStart(deser::ContainerShape::new().with_order(deser::Order::Sorted)),
            false,
        ),
        (2u64.into(), true),
        (Event::seq_start(), false),
        (3u64.into(), false),
        (4u64.into(), false),
        (Event::SeqEnd, false),
        (Event::MapEnd, false),
        (Event::MapEnd, false),
    ];

    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&item);
    while let Some((event, state)) = driver.next().unwrap() {
        events.push((event.to_static(), state.is_map_key()));
    }
    assert_eq!(events, expected);

    let mut events = Vec::new();
    SerializeDriver::new(&item)
        .drive(|event, state| {
            events.push((event.to_static(), state.is_map_key()));
            Ok(())
        })
        .unwrap();
    assert_eq!(events, expected);
}
