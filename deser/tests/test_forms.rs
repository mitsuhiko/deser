use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::{self, Debug};
use std::str::FromStr;

use deser::adapters::{
    DisplayFromStr, MapAsPairs, OneOrMany, PairsAsMap, PickFirst, PreferMany, TryFromInto,
};
use deser::de::{DeserializeDriver, DeserializeOwned, DuplicateKeys};
use deser::ser::SerializeDriver;
use deser::{Atom, ContainerShape, Deserialize, Error, ErrorKind, Event, Order, Serialize};

/// Removes the length from container starts, the tests are not about it.
fn without_len(event: Event<'static>) -> Event<'static> {
    match event {
        Event::MapStart(shape) => Event::MapStart(ContainerShape::with_order(shape.order())),
        Event::SeqStart(shape) => Event::SeqStart(ContainerShape::with_order(shape.order())),
        event => event,
    }
}

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    deserialize_with(None, events)
}

fn deserialize_with<T: DeserializeOwned>(
    policy: Option<DuplicateKeys>,
    events: Vec<Event<'_>>,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        if let Some(policy) = policy {
            policy.set(driver.state_mut());
        }
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

fn serialize<T: Serialize + ?Sized>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(without_len(event.to_static()));
    }
    // the other driver interface produces the same events
    let mut driven = Vec::new();
    SerializeDriver::new(&value)
        .drive(|event, _| {
            driven.push(without_len(event.to_static()));
            Ok(())
        })
        .unwrap();
    assert_eq!(events, driven);
    events
}

/// Checks the serialized form and that it deserializes back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
    let expected = events.iter().map(|x| x.to_static()).collect::<Vec<_>>();
    assert_eq!(serialize(&value), expected);
    assert_eq!(deserialize::<T>(events).unwrap(), value);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Rule {
    #[deser(as = OneOrMany)]
    hosts: Vec<String>,
    #[deser(as = OneOrMany<_, PreferMany>)]
    ports: Vec<u16>,
}

#[test]
fn test_one_or_many() {
    // a single element
    let rule: Rule = deserialize(vec![
        Event::map_start(),
        "hosts".into(),
        "a".into(),
        "ports".into(),
        80u64.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        rule,
        Rule {
            hosts: vec!["a".into()],
            ports: vec![80],
        }
    );

    // one element is written as the element, unless many are preferred
    check(
        rule,
        vec![
            Event::map_start(),
            "hosts".into(),
            "a".into(),
            "ports".into(),
            Event::seq_start(),
            80u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );

    // sequences, also of one or no elements
    check(
        Rule {
            hosts: vec!["a".into(), "b".into()],
            ports: vec![],
        },
        vec![
            Event::map_start(),
            "hosts".into(),
            Event::seq_start(),
            "a".into(),
            "b".into(),
            Event::SeqEnd,
            "ports".into(),
            Event::seq_start(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    let rule: Rule = deserialize(vec![
        Event::map_start(),
        "hosts".into(),
        Event::seq_start(),
        "a".into(),
        Event::SeqEnd,
        "ports".into(),
        Event::seq_start(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(rule.hosts, ["a"]);

    // the elements are checked in both forms
    let err = deserialize::<Rule>(vec![
        Event::map_start(),
        "hosts".into(),
        Event::seq_start(),
        "a".into(),
        1u64.into(),
        Event::SeqEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
    let err =
        deserialize::<Rule>(vec![Event::map_start(), "hosts".into(), 1u64.into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Point {
    x: u32,
    y: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Shapes {
    // elements that are maps
    #[deser(as = OneOrMany)]
    points: Vec<Point>,
    // elements that are sequences, a sequence is always the collection
    #[deser(as = OneOrMany)]
    pairs: Vec<(u32, u32)>,
    #[deser(as = Option<OneOrMany<DisplayFromStr>>)]
    numbers: Option<VecDeque<u32>>,
}

#[test]
fn test_one_or_many_compound() {
    let shapes: Shapes = deserialize(vec![
        Event::map_start(),
        "points".into(),
        Event::map_start(),
        "x".into(),
        1u64.into(),
        "y".into(),
        2u64.into(),
        Event::MapEnd,
        "pairs".into(),
        Event::seq_start(),
        Event::seq_start(),
        1u64.into(),
        2u64.into(),
        Event::SeqEnd,
        Event::seq_start(),
        3u64.into(),
        4u64.into(),
        Event::SeqEnd,
        Event::SeqEnd,
        "numbers".into(),
        "42".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        shapes,
        Shapes {
            points: vec![Point { x: 1, y: 2 }],
            pairs: vec![(1, 2), (3, 4)],
            numbers: Some(VecDeque::from([42])),
        }
    );

    check(
        shapes,
        vec![
            Event::map_start(),
            "points".into(),
            Event::map_start(),
            "x".into(),
            1u64.into(),
            "y".into(),
            2u64.into(),
            Event::MapEnd,
            "pairs".into(),
            Event::seq_start(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::seq_start(),
            3u64.into(),
            4u64.into(),
            Event::SeqEnd,
            Event::SeqEnd,
            "numbers".into(),
            "42".into(),
            Event::MapEnd,
        ],
    );

    // the optional is missing
    let shapes: Shapes = deserialize(vec![
        Event::map_start(),
        "points".into(),
        Event::seq_start(),
        Event::SeqEnd,
        "pairs".into(),
        Event::seq_start(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(shapes.numbers, None);
}

#[test]
fn test_one_or_many_sets() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sets {
        #[deser(as = OneOrMany)]
        sorted: BTreeSet<u32>,
        #[deser(as = OneOrMany)]
        hashed: HashSet<u32>,
    }

    check(
        Sets {
            sorted: BTreeSet::from([1]),
            hashed: HashSet::from([2]),
        },
        vec![
            Event::map_start(),
            "sorted".into(),
            1u64.into(),
            "hashed".into(),
            2u64.into(),
            Event::MapEnd,
        ],
    );
    let sets: Sets = deserialize(vec![
        Event::map_start(),
        "sorted".into(),
        Event::seq_start(),
        2u64.into(),
        1u64.into(),
        Event::SeqEnd,
        "hashed".into(),
        Event::seq_start(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(sets.sorted, BTreeSet::from([1, 2]));
    assert!(sets.hashed.is_empty());
}

#[test]
fn test_one_or_many_bytes() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Blob {
        #[deser(as = OneOrMany)]
        data: Vec<u8>,
    }

    // bytes are the collection, a number is one element
    check(
        Blob {
            data: vec![1, 2, 3],
        },
        vec![
            Event::map_start(),
            "data".into(),
            Event::Atom(Atom::Bytes(vec![1, 2, 3].into())),
            Event::MapEnd,
        ],
    );
    let blob: Blob = deserialize(vec![
        Event::map_start(),
        "data".into(),
        7u64.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(blob.data, [7]);
}

#[test]
fn test_one_or_many_multimap() {
    let mut shape = ContainerShape::new();
    shape.set_multimap(true);
    let multimap = |pairs: &[(&'static str, &'static str)]| {
        let mut events = vec![Event::MapStart(shape)];
        for &(key, value) in pairs {
            events.push(key.into());
            events.push(Event::Atom(Atom::Lexical(value.into())));
        }
        events.push(Event::MapEnd);
        events
    };

    // repeated keys are collected
    let rule: Rule = deserialize(multimap(&[
        ("hosts", "a"),
        ("ports", "1"),
        ("hosts", "b"),
        ("ports", "2"),
    ]))
    .unwrap();
    assert_eq!(rule.hosts, ["a", "b"]);
    assert_eq!(rule.ports, [1, 2]);

    // missing keys are empty collections
    let rule: Rule = deserialize(multimap(&[("hosts", "a")])).unwrap();
    assert_eq!(rule.hosts, ["a"]);
    assert!(rule.ports.is_empty());
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Item {
    #[deser(as = PickFirst<(_, DisplayFromStr)>)]
    id: u64,
    #[deser(as = Option<PickFirst<(_, DisplayFromStr)>>)]
    parent: Option<u64>,
}

#[test]
fn test_pick_first() {
    let item = Item {
        id: 42,
        parent: Some(1),
    };
    check(
        item,
        vec![
            Event::map_start(),
            "id".into(),
            42u64.into(),
            "parent".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );

    // the second adapter accepts strings, the optional can be missing
    let item: Item = deserialize(vec![
        Event::map_start(),
        "id".into(),
        "42".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        item,
        Item {
            id: 42,
            parent: None
        }
    );

    // no adapter accepts the value, the error is the one of the first
    let err = deserialize::<Item>(vec![Event::map_start(), "id".into(), true.into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
    assert_eq!(err.message(), "unexpected bool, expected u64");
    let err = deserialize::<Item>(vec![
        Event::map_start(),
        "id".into(),
        Event::seq_start(),
        Event::SeqEnd,
    ])
    .unwrap_err();
    assert_eq!(err.message(), "unexpected sequence, expected u64");
}

/// A point that can also be written as `x,y`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Pos {
    x: i32,
    y: i32,
}

impl FromStr for Pos {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Pos, Self::Err> {
        let (x, y) = s.split_once(',').ok_or("missing comma")?;
        Ok(Pos {
            x: x.parse().map_err(|_| "invalid x")?,
            y: y.parse().map_err(|_| "invalid y")?,
        })
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.x, self.y)
    }
}

impl From<(i32, i32)> for Pos {
    fn from((x, y): (i32, i32)) -> Pos {
        Pos { x, y }
    }
}

impl From<Pos> for (i32, i32) {
    fn from(pos: Pos) -> (i32, i32) {
        (pos.x, pos.y)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Marker {
    #[deser(as = PickFirst<(_, DisplayFromStr, TryFromInto<(i32, i32)>)>)]
    pos: Pos,
    #[deser(as = PickFirst<(DisplayFromStr, _)>)]
    other: Pos,
}

#[test]
fn test_pick_first_recorded() {
    // maps and sequences are recorded and replayed for every adapter
    let marker: Marker = deserialize(vec![
        Event::map_start(),
        "pos".into(),
        Event::seq_start(),
        1i64.into(),
        (-2i64).into(),
        Event::SeqEnd,
        "other".into(),
        Event::map_start(),
        "x".into(),
        3i64.into(),
        "y".into(),
        4i64.into(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        marker,
        Marker {
            pos: Pos { x: 1, y: -2 },
            other: Pos { x: 3, y: 4 },
        }
    );

    // serialization uses the first adapter
    check(
        marker,
        vec![
            Event::map_start(),
            "pos".into(),
            Event::map_start(),
            "x".into(),
            1i64.into(),
            "y".into(),
            (-2i64).into(),
            Event::MapEnd,
            "other".into(),
            "3,4".into(),
            Event::MapEnd,
        ],
    );

    // the error of a recorded value is the one of the first adapter
    let err = deserialize::<Marker>(vec![
        Event::map_start(),
        "pos".into(),
        Event::map_start(),
        "x".into(),
        1i64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Grid {
    #[deser(as = MapAsPairs)]
    cells: BTreeMap<(u32, u32), String>,
    #[deser(as = MapAsPairs<_, DisplayFromStr>)]
    weights: HashMap<String, u32>,
}

#[test]
fn test_map_as_pairs() {
    let mut cells = BTreeMap::new();
    cells.insert((0, 1), "a".to_string());
    cells.insert((1, 0), "b".to_string());
    check(
        Grid {
            cells,
            weights: HashMap::from([("x".to_string(), 1)]),
        },
        vec![
            Event::map_start(),
            "cells".into(),
            Event::SeqStart(ContainerShape::with_order(Order::Sorted)),
            Event::seq_start(),
            Event::seq_start(),
            0u64.into(),
            1u64.into(),
            Event::SeqEnd,
            "a".into(),
            Event::SeqEnd,
            Event::seq_start(),
            Event::seq_start(),
            1u64.into(),
            0u64.into(),
            Event::SeqEnd,
            "b".into(),
            Event::SeqEnd,
            Event::SeqEnd,
            "weights".into(),
            Event::SeqStart(ContainerShape::with_order(Order::Arbitrary)),
            Event::seq_start(),
            "x".into(),
            "1".into(),
            Event::SeqEnd,
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );

    // maps are accepted too
    let grid: Grid = deserialize(vec![
        Event::map_start(),
        "cells".into(),
        Event::seq_start(),
        Event::SeqEnd,
        "weights".into(),
        Event::map_start(),
        "y".into(),
        "2".into(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert!(grid.cells.is_empty());
    assert_eq!(grid.weights["y"], 2);
}

#[test]
fn test_map_as_pairs_duplicates() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Weights {
        #[deser(as = MapAsPairs)]
        weights: BTreeMap<String, u32>,
    }

    let events = || {
        vec![
            Event::map_start(),
            "weights".into(),
            Event::seq_start(),
            Event::seq_start(),
            "a".into(),
            1u64.into(),
            Event::SeqEnd,
            Event::seq_start(),
            "a".into(),
            2u64.into(),
            Event::SeqEnd,
            Event::SeqEnd,
            Event::MapEnd,
        ]
    };

    let err = deserialize::<Weights>(events()).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DuplicateKey);
    assert_eq!(err.message(), "duplicate key in map");
    let weights: Weights = deserialize_with(Some(DuplicateKeys::Last), events()).unwrap();
    assert_eq!(weights.weights["a"], 2);
    let weights: Weights = deserialize_with(Some(DuplicateKeys::First), events()).unwrap();
    assert_eq!(weights.weights["a"], 1);

    // pairs that are not pairs
    let err = deserialize::<Weights>(vec![
        Event::map_start(),
        "weights".into(),
        Event::seq_start(),
        "a".into(),
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
    let err =
        deserialize::<Weights>(vec![Event::map_start(), "weights".into(), "a".into()]).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected BTreeMap");
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Headers {
    #[deser(as = PairsAsMap)]
    headers: Vec<(String, String)>,
    #[deser(as = PairsAsMap<DisplayFromStr, _>)]
    codes: Vec<(u32, bool)>,
}

#[test]
fn test_pairs_as_map() {
    let headers = Headers {
        headers: vec![
            ("b".into(), "1".into()),
            ("a".into(), "2".into()),
            ("b".into(), "3".into()),
        ],
        codes: vec![(200, true)],
    };
    let significant = || Event::MapStart(ContainerShape::with_order(Order::Significant));
    // the order and duplicate keys are kept
    check(
        headers,
        vec![
            Event::map_start(),
            "headers".into(),
            significant(),
            "b".into(),
            "1".into(),
            "a".into(),
            "2".into(),
            "b".into(),
            "3".into(),
            Event::MapEnd,
            "codes".into(),
            significant(),
            "200".into(),
            true.into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );

    // sequences of pairs are accepted too
    let headers: Headers = deserialize(vec![
        Event::map_start(),
        "headers".into(),
        Event::seq_start(),
        Event::seq_start(),
        "a".into(),
        "1".into(),
        Event::SeqEnd,
        Event::SeqEnd,
        "codes".into(),
        Event::seq_start(),
        Event::seq_start(),
        "404".into(),
        false.into(),
        Event::SeqEnd,
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(headers.headers, [("a".to_string(), "1".to_string())]);
    assert_eq!(headers.codes, [(404, false)]);

    let err = deserialize::<Headers>(vec![
        Event::map_start(),
        "headers".into(),
        Event::map_start(),
        "a".into(),
        1u64.into(),
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
}

/// The entries of a map in order, as type that can be flattened.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(transparent)]
struct Extra(#[deser(as = PairsAsMap<_, DisplayFromStr>)] Vec<(String, u32)>);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Record {
    id: u32,
    #[deser(flatten)]
    extra: Extra,
}

#[test]
fn test_pairs_as_map_flattened() {
    // the keys the struct does not take, in order
    check(
        Record {
            id: 1,
            extra: Extra(vec![("z".into(), 2), ("a".into(), 3)]),
        },
        vec![
            Event::map_start(),
            "id".into(),
            1u64.into(),
            "z".into(),
            "2".into(),
            "a".into(),
            "3".into(),
            Event::MapEnd,
        ],
    );

    // no other keys
    check(
        Record {
            id: 1,
            extra: Extra(vec![]),
        },
        vec![Event::map_start(), "id".into(), 1u64.into(), Event::MapEnd],
    );
}
