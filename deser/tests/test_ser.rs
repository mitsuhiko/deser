use std::borrow::Cow;
use std::collections::BTreeSet;

use deser::ser::SerializeDriver;
use deser::{Atom, Event, Serialize};

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

fn capture_events(s: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(s);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(without_len(event.to_static()));
    }
    events
}

#[test]
fn test_floats() {
    assert_eq!(capture_events(&0.1f32), vec![Event::Atom(Atom::F32(0.1))]);
    assert_eq!(capture_events(&0.1f64), vec![Event::Atom(Atom::F64(0.1))]);
    assert_eq!(Event::from(0.5f32), Event::Atom(Atom::F32(0.5)));
    assert_eq!(
        capture_events(&[1.5f32, 2.5]),
        vec![
            Event::seq_start(),
            Event::Atom(Atom::F32(1.5)),
            Event::Atom(Atom::F32(2.5)),
            Event::SeqEnd,
        ]
    );
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
            Some((Event::MapStart(shape) | Event::SeqStart(shape), _, _)) => Some(shape),
            _ => None,
        }
    }

    let mut map = std::collections::HashMap::new();
    map.insert(1u32, 2u32);
    let arbitrary = deser::ContainerShape::new()
        .with_order(deser::Order::Arbitrary)
        .with_len(1);
    assert_eq!(top_shape(&map), Some(arbitrary));
    assert_eq!(top_shape(&&map), Some(arbitrary));
    assert_eq!(top_shape(&Box::new(&map)), Some(arbitrary));
    assert_eq!(top_shape(&Some(&map)), Some(arbitrary));
    assert_eq!(top_shape(&None::<u32>), None);
    let len = |len| Some(deser::ContainerShape::new().with_len(len));
    assert_eq!(top_shape(&vec![1u32]), len(1));
    assert_eq!(top_shape(&[1u32, 2, 3]), len(3));
    assert_eq!(top_shape(&(1, "x")), len(2));

    #[derive(Serialize)]
    struct Point {
        x: u32,
        y: u32,
    }
    #[derive(Serialize)]
    #[deser(skip_serializing_optionals)]
    struct MaybePoint {
        x: Option<u32>,
    }
    assert_eq!(top_shape(&Point { x: 1, y: 2 }), len(2));
    // fields can be skipped, the length is unknown
    assert_eq!(
        top_shape(&MaybePoint { x: None }),
        Some(deser::ContainerShape::new())
    );

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
    while let Some((event, _, state)) = driver.next().unwrap() {
        events.push((without_len(event.to_static()), state.is_map_key()));
    }
    assert_eq!(events, expected);

    let mut events = Vec::new();
    SerializeDriver::new(&item)
        .drive(|event, state| {
            events.push((without_len(event.to_static()), state.is_map_key()));
            Ok(())
        })
        .unwrap();
    assert_eq!(events, expected);
}

#[test]
fn test_describe_through_layers() {
    use deser::ser::{Describe, Layer, Next};

    struct Passthrough;

    impl Layer for Passthrough {
        fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), deser::Error> {
            next.emit(event)
        }
    }

    #[derive(Default)]
    struct Names(Vec<String>);

    impl Describe for Names {
        fn structure(&mut self, name: &str) {
            self.0.push(name.into());
        }

        fn some(&mut self) {
            self.0.push("Some".into());
        }
    }

    #[derive(deser::Serialize)]
    struct Point {
        x: Option<u32>,
    }

    let mut names = Names::default();
    let mut driver = SerializeDriver::new(&Point { x: Some(1) });
    driver.push_layer(Passthrough);
    driver
        .drive_described(|_event, value, state| {
            if !state.is_map_key() {
                value.describe(&mut names);
            }
            Ok(())
        })
        .unwrap();
    // the map start (and end) describe the struct, the value the option
    assert_eq!(names.0, ["Point", "Some", "Point"]);

    // without values nothing is described
    let mut driver = SerializeDriver::new(&Point { x: Some(1) });
    let mut count = 0;
    driver
        .drive(|_event, _state| {
            count += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(count, 4);
}

/// `drive` emits sequences of plain values (atoms and sequences of atoms)
/// on a fast path, the events and the state have to be the same as with
/// `next`.
#[test]
fn test_drive_like_next() {
    use std::collections::{BTreeMap, VecDeque};

    fn check(value: &dyn Serialize) {
        type Seen = (Event<'static>, bool, usize);
        let mut expected: Vec<Seen> = Vec::new();
        let mut driver = SerializeDriver::new(value);
        while let Some((event, _, state)) = driver.next().unwrap() {
            expected.push((event.to_static(), state.is_map_key(), state.depth()));
        }
        let mut events: Vec<Seen> = Vec::new();
        SerializeDriver::new(value)
            .drive(|event, state| {
                events.push((event.to_static(), state.is_map_key(), state.depth()));
                Ok(())
            })
            .unwrap();
        assert_eq!(events, expected);
    }

    check(&vec![1u32, 2, 3]);
    check(&vec![(1u8, -2i64), (3, 4)]);
    check(&vec![vec![0.5f32, 1.5], vec![]]);
    check(&[Some("a".to_string()), None]);
    check(&vec![vec![1u8, 2], vec![3]]);
    check(&vec![[1u8, 2]]);
    check(&(1u32, "x".to_string(), [true, false], ('c', ())));
    check(&VecDeque::from(vec![(1u64, 2.5f64)]));
    check(&vec![u128::MAX, 1]);
    // not plain: maps and strings by reference
    check(&vec![BTreeMap::from([(1u32, vec![1u32])])]);
    check(&vec![(vec!["a"], 1u32)]);
    // sequences as map keys
    check(&BTreeMap::from([
        ((1u32, 2u32), vec![3u32]),
        ((4, 5), vec![]),
    ]));
    check(&vec![std::collections::HashMap::from([(
        "a".to_string(),
        1u8,
    )])]);
    check(&std::collections::BTreeSet::from([(1u8, 'x')]));
    check(&std::collections::HashSet::from([1u64]));

    // structs emit runs of plain fields
    fn is_zero(value: &u32) -> bool {
        *value == 0
    }

    #[derive(deser::Serialize)]
    struct Inner {
        a: u32,
        b: Vec<(f32, f32)>,
    }

    #[derive(deser::Serialize)]
    struct Outer {
        id: u64,
        name: String,
        #[deser(skip_serializing_if = is_zero)]
        skipped: u32,
        inner: Inner,
        inners: Vec<Inner>,
        #[deser(as = deser::adapters::bytes::BytesFallback<deser::adapters::bytes::Hex>)]
        bytes: Vec<u8>,
        tags: BTreeMap<String, Vec<u32>>,
        last: Option<i8>,
    }

    #[derive(deser::Serialize)]
    #[deser(skip_serializing_optionals)]
    struct Optionals {
        a: Option<u32>,
        b: Option<u32>,
        c: (u32, Option<()>),
    }

    let inner = || Inner {
        a: 1,
        b: vec![(0.5, 1.5)],
    };
    for skipped in [0, 1] {
        check(&Outer {
            id: 1,
            name: "x".into(),
            skipped,
            inner: inner(),
            inners: vec![inner(), inner()],
            bytes: vec![1, 2],
            tags: BTreeMap::from([("t".into(), vec![1])]),
            last: None,
        });
    }
    check(&vec![
        Optionals {
            a: None,
            b: Some(1),
            c: (1, None),
        },
        Optionals {
            a: Some(2),
            b: None,
            c: (2, Some(())),
        },
    ]);
}
