use std::collections::BTreeMap;
use std::fmt::Debug;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Atom, Deserialize, Error, Event, Serialize};

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

fn serialize<T: Serialize + ?std::marker::Sized>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&value);
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
            Event::map_start(),
            "Newtype".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        External::Tuple(1, "x".into()),
        vec![
            Event::map_start(),
            "Tuple".into(),
            Event::seq_start(),
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
            Event::map_start(),
            "Struct".into(),
            Event::map_start(),
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
            Event::map_start(),
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
        "UnknownVariant: unknown variant `Nope` of External, expected one of `Unit`, `Newtype`, `Tuple`, `Struct`"
    );
    let err = deserialize::<External>(vec![
        Event::map_start(),
        "Newtype".into(),
        1u64.into(),
        "Unit".into(),
        ().into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidType: expected a map with a single key for External"
    );
    assert!(deserialize::<External>(vec![Event::map_start(), Event::MapEnd]).is_err());
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
        vec![
            Event::map_start(),
            "type".into(),
            "Unit".into(),
            Event::MapEnd,
        ],
    );
    check(
        Internal::Newtype(Inner { x: 1, y: 2 }),
        vec![
            Event::map_start(),
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
            Event::map_start(),
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
            Event::map_start(),
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
            Event::map_start(),
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
            Event::map_start(),
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
        vec![Event::map_start(), "t".into(), "Unit".into(), Event::MapEnd],
    );
    check(
        Adjacent::Newtype(1),
        vec![
            Event::map_start(),
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
            Event::map_start(),
            "t".into(),
            "Tuple".into(),
            "c".into(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        Adjacent::Struct { a: 1 },
        vec![
            Event::map_start(),
            "t".into(),
            "Struct".into(),
            "c".into(),
            Event::map_start(),
            "a".into(),
            1u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );

    // content before the tag is buffered, unknown keys are ignored
    assert_eq!(
        deserialize::<Adjacent>(vec![
            Event::map_start(),
            "c".into(),
            Event::map_start(),
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
            Event::map_start(),
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
        Event::map_start(),
        "c".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.to_string(), "MissingField: missing tag `t`");
    assert!(
        deserialize::<Adjacent>(vec![
            Event::map_start(),
            "t".into(),
            "Newtype".into(),
            Event::MapEnd
        ])
        .is_err()
    );
    assert!(
        deserialize::<Adjacent>(vec![
            Event::map_start(),
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
        vec![Event::seq_start(), 1u64.into(), 2u64.into(), Event::SeqEnd],
    );
    check(
        Untagged::Struct { a: 1 },
        vec![Event::map_start(), "a".into(), 1u64.into(), Event::MapEnd],
    );

    // variants are tried in order
    assert_eq!(
        deserialize::<Vec<Untagged>>(vec![
            Event::seq_start(),
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
        "UnknownVariant: data did not match any variant of Untagged"
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
            Event::map_start(),
            "B".into(),
            Event::seq_start(),
            1u64.into(),
            Event::SeqEnd,
            Event::MapEnd
        ])
        .unwrap(),
        WithOtherExternal::Unknown
    );

    assert_eq!(
        deserialize::<WithOtherInternal>(vec![
            Event::map_start(),
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
            Event::map_start(),
            "c".into(),
            Event::map_start(),
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
            Event::map_start(),
            "kind".into(),
            "branch".into(),
            "data".into(),
            Event::seq_start(),
            Event::map_start(),
            "kind".into(),
            "leaf".into(),
            "data".into(),
            1u64.into(),
            Event::MapEnd,
            Event::map_start(),
            "kind".into(),
            "boxed".into(),
            "data".into(),
            Event::map_start(),
            "kind".into(),
            "opt".into(),
            "data".into(),
            ().into(),
            Event::MapEnd,
            Event::MapEnd,
            Event::map_start(),
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
        vec![
            Event::map_start(),
            "Right".into(),
            "x".into(),
            Event::MapEnd,
        ],
    );
    check(
        GenericInternal::<u32, bool>::First { value: 1 },
        vec![
            Event::map_start(),
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
            Event::map_start(),
            "type".into(),
            "Both".into(),
            "a".into(),
            Event::seq_start(),
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
            Event::map_start(),
            "type".into(),
            "Nothing".into(),
            Event::MapEnd,
        ],
    );
    check(GenericUntagged::One(1u32), vec![1u64.into()]);
    check(
        GenericUntagged::Many(vec![1u32]),
        vec![Event::seq_start(), 1u64.into(), Event::SeqEnd],
    );
    check(
        GenericAdjacent::Pair(1u32, 2),
        vec![
            Event::map_start(),
            "t".into(),
            "Pair".into(),
            "c".into(),
            Event::seq_start(),
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
    // the helper struct for this variant deserializes the nested enum
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
            Event::map_start(),
            "children".into(),
            Event::seq_start(),
            1u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        Bounded::<u32, u32>::Only { value: 1 },
        vec![
            Event::map_start(),
            "t".into(),
            "Only".into(),
            "value".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t")]
enum Sized<const N: usize> {
    Array { values: [u32; N] },
    Empty,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ConstNewtype<const N: usize> {
    Array([u8; N]),
    Unit,
}

#[test]
fn test_const_generics() {
    check(
        Sized::<2>::Array { values: [1, 2] },
        vec![
            Event::map_start(),
            "t".into(),
            "Array".into(),
            "values".into(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        Sized::<2>::Empty,
        vec![
            Event::map_start(),
            "t".into(),
            "Empty".into(),
            Event::MapEnd,
        ],
    );
    check(ConstNewtype::<1>::Unit, vec!["Unit".into()]);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type", content = "data")]
enum Mixed<'a, T, const N: usize>
where
    T: Clone + 'a,
{
    Borrowed {
        text: &'a str,
        value: T,
    },
    Owned([T; N]),
    #[deser(other)]
    Other(#[deser(tag)] String),
}

#[test]
fn test_lifetimes_types_and_consts() {
    let input = String::from("text");
    let mut out = None::<Mixed<'_, u32, 1>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in [
            Event::map_start(),
            "type".into(),
            "Borrowed".into(),
            "data".into(),
            Event::map_start(),
            "text".into(),
            input.as_str().into(),
            "value".into(),
            1u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ] {
            driver.emit_borrowed(event).unwrap();
        }
    }
    assert_eq!(
        out.unwrap(),
        Mixed::Borrowed {
            text: "text",
            value: 1
        }
    );
    // types with lifetimes are not `DeserializeOwned`
    fn check_borrowed(value: Mixed<'_, u32, 1>, events: Vec<Event<'_>>) {
        assert_eq!(
            serialize(&value),
            events.iter().map(|x| x.to_static()).collect::<Vec<_>>()
        );
        let mut out = None::<Mixed<'_, u32, 1>>;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            for event in events {
                driver.emit(event).unwrap();
            }
        }
        assert_eq!(out.unwrap(), value);
    }
    check_borrowed(
        Mixed::Owned([1]),
        vec![
            Event::map_start(),
            "type".into(),
            "Owned".into(),
            "data".into(),
            Event::seq_start(),
            1u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check_borrowed(
        Mixed::Other("x".into()),
        vec![Event::map_start(), "type".into(), "x".into(), Event::MapEnd],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Common {
    id: u32,
    #[deser(default)]
    note: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum FlattenExternal {
    A {
        #[deser(flatten)]
        common: Common,
        a: u32,
    },
    B {
        name: String,
        #[deser(flatten)]
        extra: BTreeMap<String, u32>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type", skip_serializing_optionals)]
enum FlattenInternal {
    A {
        #[deser(flatten)]
        common: Common,
        a: Option<u32>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c", deny_unknown_fields)]
enum FlattenAdjacent {
    A {
        #[deser(flatten)]
        common: Common,
        #[deser(flatten, skip_serializing_if = Option::is_none)]
        more: Option<Common>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum FlattenUntagged {
    A {
        #[deser(flatten)]
        common: Common,
        a: u32,
    },
}

#[test]
fn test_flatten_in_variants() {
    check(
        FlattenExternal::A {
            common: Common { id: 1, note: None },
            a: 2,
        },
        vec![
            Event::map_start(),
            "A".into(),
            Event::map_start(),
            "id".into(),
            1u64.into(),
            "note".into(),
            Atom::Null.into(),
            "a".into(),
            2u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
    check(
        FlattenExternal::B {
            name: "x".into(),
            extra: BTreeMap::from([("k".into(), 1)]),
        },
        vec![
            Event::map_start(),
            "B".into(),
            Event::map_start(),
            "name".into(),
            "x".into(),
            "k".into(),
            1u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
    // optional values of flattened fields are skipped as well
    check(
        FlattenInternal::A {
            common: Common { id: 1, note: None },
            a: None,
        },
        vec![
            Event::map_start(),
            "type".into(),
            "A".into(),
            "id".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    // the fields can come before the tag
    assert_eq!(
        deserialize::<FlattenInternal>(vec![
            Event::map_start(),
            "id".into(),
            1u64.into(),
            "note".into(),
            "n".into(),
            "type".into(),
            "A".into(),
            "a".into(),
            2u64.into(),
            Event::MapEnd,
        ])
        .unwrap(),
        FlattenInternal::A {
            common: Common {
                id: 1,
                note: Some("n".into())
            },
            a: Some(2),
        }
    );
    check(
        FlattenAdjacent::A {
            common: Common { id: 1, note: None },
            more: None,
        },
        vec![
            Event::map_start(),
            "t".into(),
            "A".into(),
            "c".into(),
            Event::map_start(),
            "id".into(),
            1u64.into(),
            "note".into(),
            Atom::Null.into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
    let err = deserialize::<FlattenAdjacent>(vec![
        Event::map_start(),
        "t".into(),
        "A".into(),
        "c".into(),
        Event::map_start(),
        "id".into(),
        1u64.into(),
        "other".into(),
        1u64.into(),
    ])
    .unwrap_err();
    assert_eq!(err.message(), "unknown field `other`");
    check(
        FlattenUntagged::A {
            common: Common {
                id: 1,
                note: Some("n".into()),
            },
            a: 2,
        },
        vec![
            Event::map_start(),
            "id".into(),
            1u64.into(),
            "note".into(),
            "n".into(),
            "a".into(),
            2u64.into(),
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum SkipTuple<T> {
    // the skipped field is not part of the content, which is a newtype
    Newtype(u32, #[deser(skip)] Option<T>),
    Tuple(u32, #[deser(skip, default = 7)] u32, String),
    // all fields skipped, the variant is a unit variant
    Unit(#[deser(skip)] u32),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum SkipInternal {
    Unit(#[deser(skip, default = 1)] u32),
    Unitish((), #[deser(skip)] u32),
}

#[test]
fn test_skipped_tuple_fields() {
    check(
        SkipTuple::<String>::Newtype(1, None),
        vec![
            Event::map_start(),
            "Newtype".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        SkipTuple::<String>::Tuple(1, 7, "x".into()),
        vec![
            Event::map_start(),
            "Tuple".into(),
            Event::seq_start(),
            1u64.into(),
            "x".into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(SkipTuple::<String>::Unit(0), vec!["Unit".into()]);
    assert_eq!(
        serialize(&SkipTuple::<String>::Newtype(1, Some("x".into()))),
        [
            Event::map_start(),
            "Newtype".into(),
            1u64.into(),
            Event::MapEnd
        ]
    );

    check(
        SkipInternal::Unit(1),
        vec![
            Event::map_start(),
            "type".into(),
            "Unit".into(),
            Event::MapEnd,
        ],
    );
    check(
        SkipInternal::Unitish((), 0),
        vec![
            Event::map_start(),
            "type".into(),
            "Unitish".into(),
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum WithFallback {
    Circle {
        radius: u32,
    },
    Empty,
    #[deser(untagged)]
    Raw(BTreeMap<String, u32>),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ExternalWithFallback {
    Number(u32),
    Unit,
    #[deser(untagged)]
    Text(String),
    #[deser(untagged)]
    Nothing,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum AdjacentWithFallback {
    A(u32),
    #[deser(untagged)]
    Other {
        value: String,
    },
}

#[test]
fn test_untagged_variants() {
    check(
        WithFallback::Circle { radius: 1 },
        vec![
            Event::map_start(),
            "type".into(),
            "Circle".into(),
            "radius".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        WithFallback::Empty,
        vec![
            Event::map_start(),
            "type".into(),
            "Empty".into(),
            Event::MapEnd,
        ],
    );
    // unknown tags and maps without tag go to the untagged variant
    check(
        WithFallback::Raw(BTreeMap::from([("x".into(), 1)])),
        vec![Event::map_start(), "x".into(), 1u64.into(), Event::MapEnd],
    );
    assert_eq!(
        deserialize::<WithFallback>(vec![
            Event::map_start(),
            "type".into(),
            "Square".into(),
            Event::MapEnd,
        ])
        .unwrap_err()
        .message(),
        "unknown variant `Square` of WithFallback, expected `Circle` or `Empty`"
    );

    check(
        ExternalWithFallback::Number(1),
        vec![
            Event::map_start(),
            "Number".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(ExternalWithFallback::Unit, vec!["Unit".into()]);
    check(ExternalWithFallback::Text("x".into()), vec!["x".into()]);
    check(ExternalWithFallback::Nothing, vec![Atom::Null.into()]);

    check(
        AdjacentWithFallback::A(1),
        vec![
            Event::map_start(),
            "t".into(),
            "A".into(),
            "c".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        AdjacentWithFallback::Other { value: "x".into() },
        vec![
            Event::map_start(),
            "value".into(),
            "x".into(),
            Event::MapEnd,
        ],
    );
    // the error of the tagged representation if nothing matches
    assert_eq!(
        deserialize::<AdjacentWithFallback>(vec![true.into()])
            .unwrap_err()
            .message(),
        "unexpected bool, expected AdjacentWithFallback"
    );
}

#[test]
fn test_untagged_variant_shapes() {
    let text = ExternalWithFallback::Text("x".into());
    let mut driver = SerializeDriver::new(&text);
    let (event, _, _) = driver.next().unwrap().unwrap();
    assert_eq!(event.to_static(), Event::from("x"));
    let mut driver = SerializeDriver::new(&ExternalWithFallback::Number(1));
    let (event, _, _) = driver.next().unwrap().unwrap();
    assert_eq!(
        event.to_static(),
        Event::MapStart(deser::ContainerShape::new().with_len(1))
    );
}
