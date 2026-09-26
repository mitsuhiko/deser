use std::collections::BTreeMap;
use std::fmt::Debug;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Error, Event, Serialize};

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

/// Checks the serialized form and that it deserializes back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
    let serialized = serialize(&value);
    assert_eq!(
        serialized,
        events.iter().map(|x| x.to_static()).collect::<Vec<_>>()
    );
    assert_eq!(deserialize::<T>(events).unwrap(), value);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum External {
    Unit,
    Newtype(u32),
    Tuple(u32, String),
    Struct {
        a: u32,
        #[deser(rename = "bee")]
        b: Option<String>,
    },
}

#[test]
fn test_externally_tagged() {
    check(External::Unit, vec!["Unit".into()]);
    check(
        External::Newtype(1),
        vec![
            Event::MapStart,
            "Newtype".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        External::Tuple(1, "x".into()),
        vec![
            Event::MapStart,
            "Tuple".into(),
            Event::SeqStart,
            1u64.into(),
            "x".into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        External::Struct {
            a: 1,
            b: Some("x".into()),
        },
        vec![
            Event::MapStart,
            "Struct".into(),
            Event::MapStart,
            "a".into(),
            1u64.into(),
            "bee".into(),
            "x".into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );

    // unit variants can also be maps with null content
    assert_eq!(
        deserialize::<External>(vec![
            Event::MapStart,
            "Unit".into(),
            ().into(),
            Event::MapEnd
        ])
        .unwrap(),
        External::Unit
    );

    // errors
    let err = deserialize::<External>(vec!["Nope".into()]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unknown variant 'Nope' for External"
    );
    let err = deserialize::<External>(vec![
        Event::MapStart,
        "Newtype".into(),
        1u64.into(),
        "Unit".into(),
        ().into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: expected a map with a single key for External"
    );
    assert!(deserialize::<External>(vec![Event::MapStart, Event::MapEnd]).is_err());
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum Internal {
    Unit,
    Newtype(Inner),
    Map(BTreeMap<String, u32>),
    Struct { a: u32 },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Inner {
    x: u32,
    y: u32,
}

#[test]
fn test_internally_tagged() {
    check(
        Internal::Unit,
        vec![Event::MapStart, "type".into(), "Unit".into(), Event::MapEnd],
    );
    check(
        Internal::Newtype(Inner { x: 1, y: 2 }),
        vec![
            Event::MapStart,
            "type".into(),
            "Newtype".into(),
            "x".into(),
            1u64.into(),
            "y".into(),
            2u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        Internal::Struct { a: 1 },
        vec![
            Event::MapStart,
            "type".into(),
            "Struct".into(),
            "a".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );

    // newtype variants with maps can be deserialized, the tag is not
    // passed to the inner value
    let mut map = BTreeMap::new();
    map.insert("a".to_string(), 1);
    assert_eq!(
        deserialize::<Internal>(vec![
            Event::MapStart,
            "a".into(),
            1u64.into(),
            "type".into(),
            "Map".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Internal::Map(map.clone())
    );
    // and serialized, the keys of the map must be strings
    check(
        Internal::Map(map),
        vec![
            Event::MapStart,
            "type".into(),
            "Map".into(),
            "a".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );

    // tag last
    assert_eq!(
        deserialize::<Internal>(vec![
            Event::MapStart,
            "y".into(),
            2u64.into(),
            "x".into(),
            1u64.into(),
            "type".into(),
            "Newtype".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Internal::Newtype(Inner { x: 1, y: 2 })
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum Adjacent {
    Unit,
    Newtype(u32),
    Tuple(u32, u32),
    Struct { a: u32 },
}

#[test]
fn test_adjacently_tagged() {
    check(
        Adjacent::Unit,
        vec![Event::MapStart, "t".into(), "Unit".into(), Event::MapEnd],
    );
    check(
        Adjacent::Newtype(1),
        vec![
            Event::MapStart,
            "t".into(),
            "Newtype".into(),
            "c".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        Adjacent::Tuple(1, 2),
        vec![
            Event::MapStart,
            "t".into(),
            "Tuple".into(),
            "c".into(),
            Event::SeqStart,
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        Adjacent::Struct { a: 1 },
        vec![
            Event::MapStart,
            "t".into(),
            "Struct".into(),
            "c".into(),
            Event::MapStart,
            "a".into(),
            1u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );

    // content before the tag is buffered, unknown keys are ignored
    assert_eq!(
        deserialize::<Adjacent>(vec![
            Event::MapStart,
            "c".into(),
            Event::MapStart,
            "a".into(),
            1u64.into(),
            Event::MapEnd,
            "extra".into(),
            true.into(),
            "t".into(),
            "Struct".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Adjacent::Struct { a: 1 }
    );

    // unit variants may have null content
    assert_eq!(
        deserialize::<Adjacent>(vec![
            Event::MapStart,
            "c".into(),
            ().into(),
            "t".into(),
            "Unit".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Adjacent::Unit
    );

    // errors
    let err = deserialize::<Adjacent>(vec![
        Event::MapStart,
        "c".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.to_string(), "MissingField: missing tag 't'");
    assert!(
        deserialize::<Adjacent>(vec![
            Event::MapStart,
            "t".into(),
            "Newtype".into(),
            Event::MapEnd
        ])
        .is_err()
    );
    assert!(
        deserialize::<Adjacent>(vec![
            Event::MapStart,
            "t".into(),
            "Newtype".into(),
            "c".into(),
            1u64.into(),
            "c".into(),
            2u64.into(),
            Event::MapEnd
        ])
        .is_err()
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum Untagged {
    Unit,
    Number(u32),
    Pair(u32, u32),
    Struct { a: u32 },
    Text(String),
}

#[test]
fn test_untagged() {
    check(Untagged::Unit, vec![().into()]);
    check(Untagged::Number(1), vec![1u64.into()]);
    check(Untagged::Text("x".into()), vec!["x".into()]);
    check(
        Untagged::Pair(1, 2),
        vec![Event::SeqStart, 1u64.into(), 2u64.into(), Event::SeqEnd],
    );
    check(
        Untagged::Struct { a: 1 },
        vec![Event::MapStart, "a".into(), 1u64.into(), Event::MapEnd],
    );

    // variants are tried in order
    assert_eq!(
        deserialize::<Vec<Untagged>>(vec![
            Event::SeqStart,
            42u64.into(),
            "42".into(),
            Event::SeqEnd
        ])
        .unwrap(),
        vec![Untagged::Number(42), Untagged::Text("42".into())]
    );

    let err = deserialize::<Untagged>(vec![true.into()]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: data did not match any variant of Untagged"
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum WithOtherUnit {
    A,
    #[deser(other)]
    Unknown,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum WithOtherExternal {
    A(u32),
    #[deser(other)]
    Unknown,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum WithOtherInternal {
    A {
        a: u32,
    },
    #[deser(other)]
    Unknown,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum WithOtherAdjacent {
    A(u32),
    #[deser(other)]
    Unknown,
}

#[test]
fn test_other() {
    assert_eq!(
        deserialize::<WithOtherUnit>(vec!["B".into()]).unwrap(),
        WithOtherUnit::Unknown
    );
    assert_eq!(serialize(&WithOtherUnit::Unknown), vec!["Unknown".into()]);

    assert_eq!(
        deserialize::<WithOtherExternal>(vec!["B".into()]).unwrap(),
        WithOtherExternal::Unknown
    );
    assert_eq!(
        deserialize::<WithOtherExternal>(vec![
            Event::MapStart,
            "B".into(),
            Event::SeqStart,
            1u64.into(),
            Event::SeqEnd,
            Event::MapEnd
        ])
        .unwrap(),
        WithOtherExternal::Unknown
    );

    assert_eq!(
        deserialize::<WithOtherInternal>(vec![
            Event::MapStart,
            "b".into(),
            1u64.into(),
            "type".into(),
            "B".into(),
            Event::MapEnd
        ])
        .unwrap(),
        WithOtherInternal::Unknown
    );

    assert_eq!(
        deserialize::<WithOtherAdjacent>(vec![
            Event::MapStart,
            "c".into(),
            Event::MapStart,
            Event::MapEnd,
            "t".into(),
            "B".into(),
            Event::MapEnd
        ])
        .unwrap(),
        WithOtherAdjacent::Unknown
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "kind", content = "data", rename_all = "snake_case")]
enum Nested {
    Leaf(Untagged),
    Branch(Vec<Nested>),
    Boxed(Box<Nested>),
    Opt(Option<u32>),
}

#[test]
fn test_nesting() {
    check(
        Nested::Branch(vec![
            Nested::Leaf(Untagged::Number(1)),
            Nested::Boxed(Box::new(Nested::Opt(None))),
            Nested::Opt(Some(2)),
        ]),
        vec![
            Event::MapStart,
            "kind".into(),
            "branch".into(),
            "data".into(),
            Event::SeqStart,
            Event::MapStart,
            "kind".into(),
            "leaf".into(),
            "data".into(),
            1u64.into(),
            Event::MapEnd,
            Event::MapStart,
            "kind".into(),
            "boxed".into(),
            "data".into(),
            Event::MapStart,
            "kind".into(),
            "opt".into(),
            "data".into(),
            ().into(),
            Event::MapEnd,
            Event::MapEnd,
            Event::MapStart,
            "kind".into(),
            "opt".into(),
            "data".into(),
            2u64.into(),
            Event::MapEnd,
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Either<L, R> {
    Left(L),
    Right(R),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum GenericInternal<A, B> {
    // uses only one of the parameters
    First { value: A },
    Both { a: Vec<A>, b: Option<B> },
    Nothing,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum GenericUntagged<T> {
    One(T),
    Many(Vec<T>),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum GenericAdjacent<T>
where
    T: Clone,
{
    Value(T),
    Pair(T, T),
}

#[test]
fn test_generics() {
    check(
        Either::<u32, String>::Right("x".into()),
        vec![Event::MapStart, "Right".into(), "x".into(), Event::MapEnd],
    );
    check(
        GenericInternal::<u32, bool>::First { value: 1 },
        vec![
            Event::MapStart,
            "type".into(),
            "First".into(),
            "value".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        GenericInternal::<u32, bool>::Both {
            a: vec![1],
            b: Some(true),
        },
        vec![
            Event::MapStart,
            "type".into(),
            "Both".into(),
            "a".into(),
            Event::SeqStart,
            1u64.into(),
            Event::SeqEnd,
            "b".into(),
            true.into(),
            Event::MapEnd,
        ],
    );
    check(
        GenericInternal::<u32, bool>::Nothing,
        vec![
            Event::MapStart,
            "type".into(),
            "Nothing".into(),
            Event::MapEnd,
        ],
    );
    check(GenericUntagged::One(1u32), vec![1u64.into()]);
    check(
        GenericUntagged::Many(vec![1u32]),
        vec![Event::SeqStart, 1u64.into(), Event::SeqEnd],
    );
    check(
        GenericAdjacent::Pair(1u32, 2),
        vec![
            Event::MapStart,
            "t".into(),
            "Pair".into(),
            "c".into(),
            Event::SeqStart,
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum Tree<T> {
    Leaf(T),
    // the helper struct for this variant needs `T: 'static` to be able to
    // deserialize the nested enum
    Node { children: Vec<Box<Tree<T>>> },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t")]
enum Bounded<T, U>
where
    T: Clone + PartialEq,
    U: Default,
{
    // the where clause is carried over to the helper struct
    Only { value: T },
    Other { value: U },
}

#[test]
fn test_generic_helper_bounds() {
    check(
        Tree::Node {
            children: vec![Box::new(Tree::Leaf(1u32))],
        },
        vec![
            Event::MapStart,
            "children".into(),
            Event::SeqStart,
            1u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        Bounded::<u32, u32>::Only { value: 1 },
        vec![
            Event::MapStart,
            "t".into(),
            "Only".into(),
            "value".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
}
