//! Tests for adapters on containers (`#[deser(as = ...)]` on structs, enums
//! and unions) and for `serialize_as` / `deserialize_as`.
use std::borrow::Cow;
use std::fmt::{self, Debug, Display};
use std::marker::PhantomData;
use std::str::FromStr;

use deser::adapters::{
    As, DeserializeAs, DisplayFromStr, FromInto, Same, SerializeAs, TryFromInto,
};
use deser::de::{DeserializeDriver, DeserializeOwned, SinkHandle};
use deser::ser::{Chunk, Describe, SerializeDriver};
use deser::{Deserialize, Error, ErrorKind, Event, Serialize, State};

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

fn serialize_drive(value: &dyn Serialize) -> Result<Vec<Event<'static>>, Error> {
    let mut events = Vec::new();
    SerializeDriver::new(value).drive(|event, _| {
        events.push(without_len(event.to_static()));
        Ok(())
    })?;
    Ok(events)
}

/// Checks the serialized form (with both driver interfaces) and that it
/// deserializes back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
    let expected = events.iter().map(|x| x.to_static()).collect::<Vec<_>>();
    assert_eq!(serialize(&value), expected);
    assert_eq!(serialize_drive(&value).unwrap(), expected);
    assert_eq!(deserialize::<T>(events).unwrap(), value);
}

/// An email address that is validated when it's deserialized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[deser(as = TryFromInto<String>)]
struct Email {
    user: String,
    domain: String,
}

impl TryFrom<String> for Email {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Email, Self::Error> {
        match value.split_once('@') {
            Some((user, domain)) => Ok(Email {
                user: user.into(),
                domain: domain.into(),
            }),
            None => Err("missing @"),
        }
    }
}

impl From<Email> for String {
    fn from(value: Email) -> String {
        format!("{}@{}", value.user, value.domain)
    }
}

#[test]
fn test_struct() {
    check(
        Email {
            user: "jane".into(),
            domain: "example.com".into(),
        },
        vec!["jane@example.com".into()],
    );
    let err = deserialize::<Email>(vec!["jane".into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
    assert!(err.to_string().contains("missing @"), "{}", err);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(as = DisplayFromStr)]
enum Level {
    Debug,
    Info,
    Custom(u8),
}

impl Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Level::Debug => f.write_str("debug"),
            Level::Info => f.write_str("info"),
            Level::Custom(level) => write!(f, "level-{}", level),
        }
    }
}

impl FromStr for Level {
    type Err = String;

    fn from_str(s: &str) -> Result<Level, String> {
        match s {
            "debug" => Ok(Level::Debug),
            "info" => Ok(Level::Info),
            other => match other.strip_prefix("level-") {
                Some(level) => level.parse().map(Level::Custom).map_err(|_| s.to_string()),
                None => Err(s.to_string()),
            },
        }
    }
}

#[test]
fn test_enum() {
    check(Level::Info, vec!["info".into()]);
    check(Level::Custom(7), vec!["level-7".into()]);
    assert!(deserialize::<Level>(vec!["trace".into()]).is_err());
}

/// Tuple structs, unit structs and unions are supported with adapters on the
/// container as the fields are not used.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[deser(as = FromInto<(u8, u8, u8)>)]
struct Rgb(u8, u8, u8);

impl From<(u8, u8, u8)> for Rgb {
    fn from((r, g, b): (u8, u8, u8)) -> Rgb {
        Rgb(r, g, b)
    }
}

impl From<Rgb> for (u8, u8, u8) {
    fn from(value: Rgb) -> (u8, u8, u8) {
        (value.0, value.1, value.2)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(as = DisplayFromStr)]
struct Marker;

impl Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("marker")
    }
}

impl FromStr for Marker {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Marker, Self::Err> {
        if s == "marker" {
            Ok(Marker)
        } else {
            Err("not a marker")
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[deser(as = FromInto<u32>)]
union Bits {
    int: u32,
    float: f32,
}

impl From<u32> for Bits {
    fn from(int: u32) -> Bits {
        Bits { int }
    }
}

impl From<Bits> for u32 {
    fn from(value: Bits) -> u32 {
        // SAFETY: all bit patterns are valid for both fields
        unsafe { value.int }
    }
}

#[test]
fn test_shapes() {
    check(
        Rgb(1, 2, 3),
        vec![
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            3u64.into(),
            Event::SeqEnd,
        ],
    );
    check(Marker, vec!["marker".into()]);

    let bits = Bits { float: 1.0 };
    assert_eq!(serialize(&bits), vec![Event::from(0x3f80_0000u64)]);
    let bits: Bits = deserialize(vec![0x3f80_0000u64.into()]).unwrap();
    // SAFETY: all bit patterns are valid for both fields
    assert_eq!(unsafe { bits.float }, 1.0);
}

/// A configuration that is validated when it's read but written with the
/// derived implementation.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(deserialize_as = TryFromInto<RawConfig>, rename_all = "camelCase")]
struct Config {
    min_port: u16,
    max_port: u16,
}

#[derive(Deserialize)]
#[deser(rename_all = "camelCase")]
struct RawConfig {
    min_port: u16,
    max_port: u16,
}

impl TryFrom<RawConfig> for Config {
    type Error = String;

    fn try_from(value: RawConfig) -> Result<Config, String> {
        if value.min_port > value.max_port {
            return Err(format!("{} > {}", value.min_port, value.max_port));
        }
        Ok(Config {
            min_port: value.min_port,
            max_port: value.max_port,
        })
    }
}

/// Written as a string but read with the derived implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[deser(serialize_as = DisplayFromStr)]
struct Version {
    major: u32,
    minor: u32,
}

impl Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[test]
fn test_directional_container() {
    let config = Config {
        min_port: 1,
        max_port: 2,
    };
    let events = vec![
        Event::map_start(),
        "minPort".into(),
        1u64.into(),
        "maxPort".into(),
        2u64.into(),
        Event::MapEnd,
    ];
    check(config, events);
    let err = deserialize::<Config>(vec![
        Event::map_start(),
        "minPort".into(),
        3u64.into(),
        "maxPort".into(),
        2u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert!(err.to_string().contains("3 > 2"), "{}", err);

    let version = Version { major: 1, minor: 2 };
    assert_eq!(serialize(&version), vec![Event::from("1.2")]);
    let version: Version = deserialize(vec![
        Event::map_start(),
        "major".into(),
        1u64.into(),
        "minor".into(),
        2u64.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(version, Version { major: 1, minor: 2 });
}

/// Neither `T` nor the fields need to be serializable, only the adapter has
/// to support the type.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(as = FromInto<String>)]
struct Name<T> {
    name: String,
    _marker: PhantomData<T>,
}

impl<T> Clone for Name<T> {
    fn clone(&self) -> Self {
        Name {
            name: self.name.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> From<String> for Name<T> {
    fn from(name: String) -> Self {
        Name {
            name,
            _marker: PhantomData,
        }
    }
}

impl<T> From<Name<T>> for String {
    fn from(value: Name<T>) -> String {
        value.name
    }
}

#[derive(Debug, PartialEq)]
struct NotSerializable;

/// The adapter can refer to the type parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[deser(as = FromInto<Vec<T>>)]
struct Items<T> {
    items: Vec<T>,
}

impl<T> From<Vec<T>> for Items<T> {
    fn from(items: Vec<T>) -> Self {
        Items { items }
    }
}

impl<T> From<Items<T>> for Vec<T> {
    fn from(value: Items<T>) -> Vec<T> {
        value.items
    }
}

/// Recursive types can refer to themselves in the adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[deser(as = FromInto<Vec<Tree>>)]
struct Tree {
    children: Vec<Tree>,
}

impl From<Vec<Tree>> for Tree {
    fn from(children: Vec<Tree>) -> Self {
        Tree { children }
    }
}

impl From<Tree> for Vec<Tree> {
    fn from(value: Tree) -> Vec<Tree> {
        value.children
    }
}

#[test]
fn test_recursive() {
    check(
        Tree {
            children: vec![Tree { children: vec![] }],
        },
        vec![
            Event::seq_start(),
            Event::seq_start(),
            Event::SeqEnd,
            Event::SeqEnd,
        ],
    );
}

#[test]
fn test_generics() {
    check(
        Name::<NotSerializable>::from("x".to_string()),
        vec!["x".into()],
    );
    check(
        Items {
            items: vec![1u32, 2],
        },
        vec![Event::seq_start(), 1u64.into(), 2u64.into(), Event::SeqEnd],
    );
}

/// A name that is optional as the adapter makes missing values `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[deser(as = FromInto<Option<String>>)]
struct Nickname(Option<String>);

impl From<Option<String>> for Nickname {
    fn from(value: Option<String>) -> Self {
        Nickname(value)
    }
}

impl From<Nickname> for Option<String> {
    fn from(value: Nickname) -> Option<String> {
        value.0
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(skip_serializing_optionals)]
struct Person {
    nickname: Nickname,
}

#[test]
fn test_missing_and_optional() {
    // the initial value comes from the adapter
    let person: Person = deserialize(vec![Event::map_start(), Event::MapEnd]).unwrap();
    assert_eq!(person.nickname, Nickname(None));
    // and so does optionality
    assert_eq!(serialize(&person), vec![Event::map_start(), Event::MapEnd]);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Fields {
    // written as a string, read as a number
    #[deser(serialize_as = DisplayFromStr)]
    written: u32,
    // written as a number, read from a string
    #[deser(deserialize_as = DisplayFromStr)]
    read: u32,
    #[deser(serialize_as = Option<DisplayFromStr>, deserialize_as = Option<_>)]
    optional: Option<u32>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct NewtypeField(#[deser(deserialize_as = DisplayFromStr)] u32);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Variants {
    Tuple(#[deser(serialize_as = DisplayFromStr)] u32, u32),
    Struct {
        #[deser(deserialize_as = DisplayFromStr)]
        value: u32,
    },
    #[deser(other)]
    Other(#[deser(tag, serialize_as = FromInto<String>)] Tag),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Tag(String);

impl From<Tag> for String {
    fn from(value: Tag) -> String {
        value.0
    }
}

/// Only the direction with the adapter needs the adapter bound, the other
/// one needs the regular bounds.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Generic<T> {
    #[deser(deserialize_as = DisplayFromStr)]
    value: T,
}

#[test]
fn test_directional_fields() {
    let value = Fields {
        written: 1,
        read: 2,
        optional: Some(3),
    };
    assert_eq!(
        serialize(&value),
        vec![
            Event::map_start(),
            "written".into(),
            "1".into(),
            "read".into(),
            2u64.into(),
            "optional".into(),
            "3".into(),
            Event::MapEnd,
        ]
    );
    let value: Fields = deserialize(vec![
        Event::map_start(),
        "written".into(),
        1u64.into(),
        "read".into(),
        "2".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        value,
        Fields {
            written: 1,
            read: 2,
            optional: None,
        }
    );

    assert_eq!(serialize(&NewtypeField(1)), vec![Event::from(1u64)]);
    assert_eq!(
        deserialize::<NewtypeField>(vec!["1".into()]).unwrap(),
        NewtypeField(1)
    );

    assert_eq!(
        serialize(&Variants::Tuple(1, 2)),
        vec![
            Event::map_start(),
            "Tuple".into(),
            Event::seq_start(),
            "1".into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ]
    );
    assert_eq!(
        deserialize::<Variants>(vec![
            Event::map_start(),
            "Tuple".into(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ])
        .unwrap(),
        Variants::Tuple(1, 2)
    );
    assert_eq!(
        deserialize::<Variants>(vec![
            Event::map_start(),
            "Struct".into(),
            Event::map_start(),
            "value".into(),
            "1".into(),
            Event::MapEnd,
            Event::MapEnd,
        ])
        .unwrap(),
        Variants::Struct { value: 1 }
    );
    assert_eq!(
        serialize(&Variants::Struct { value: 1 }),
        vec![
            Event::map_start(),
            "Struct".into(),
            Event::map_start(),
            "value".into(),
            1u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ]
    );
    let other = Variants::Other(Tag("x".into()));
    assert_eq!(serialize(&other), vec![Event::from("x")]);
    assert_eq!(deserialize::<Variants>(vec!["x".into()]).unwrap(), other);

    let value = Generic { value: 1u32 };
    assert_eq!(
        serialize(&value),
        vec![
            Event::map_start(),
            "value".into(),
            1u64.into(),
            Event::MapEnd
        ]
    );
    assert_eq!(
        deserialize::<Generic<u32>>(vec![
            Event::map_start(),
            "value".into(),
            "1".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        value
    );
}

/// A section that is validated and flattened into another struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[deser(as = TryFromInto<RawRange>)]
struct Range {
    start: u32,
    end: u32,
}

#[derive(Serialize, Deserialize)]
struct RawRange {
    start: u32,
    end: u32,
}

impl TryFrom<RawRange> for Range {
    type Error = &'static str;

    fn try_from(value: RawRange) -> Result<Range, Self::Error> {
        if value.start > value.end {
            return Err("start after end");
        }
        Ok(Range {
            start: value.start,
            end: value.end,
        })
    }
}

impl From<Range> for RawRange {
    fn from(value: Range) -> RawRange {
        RawRange {
            start: value.start,
            end: value.end,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Selection {
    name: String,
    #[deser(flatten)]
    range: Range,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Flattened {
    name: String,
    #[deser(flatten)]
    marker: Marker,
}

#[test]
fn test_flatten() {
    let events = vec![
        Event::map_start(),
        "name".into(),
        "x".into(),
        "start".into(),
        1u64.into(),
        "end".into(),
        2u64.into(),
        Event::MapEnd,
    ];
    check(
        Selection {
            name: "x".into(),
            range: Range { start: 1, end: 2 },
        },
        events,
    );
    let err = deserialize::<Selection>(vec![
        Event::map_start(),
        "name".into(),
        "x".into(),
        "start".into(),
        2u64.into(),
        "end".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert!(err.to_string().contains("start after end"), "{}", err);

    // values which do not serialize as structs cannot be flattened
    let value = Flattened {
        name: "x".into(),
        marker: Marker,
    };
    let err = serialize_drive(&value).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unable to flatten on struct into struct"
    );
    let err = deserialize::<Flattened>(vec![
        Event::map_start(),
        "name".into(),
        "x".into(),
        "marker".into(),
        "marker".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: Failed to deserialize flattened field 'marker'"
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type")]
enum Command {
    Select(Range),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum Bound {
    Range(Range),
    Name(String),
}

#[test]
fn test_enums_with_adapted_content() {
    // internally tagged newtype variants follow the forwarding
    check(
        Command::Select(Range { start: 1, end: 2 }),
        vec![
            Event::map_start(),
            "type".into(),
            "Select".into(),
            "start".into(),
            1u64.into(),
            "end".into(),
            2u64.into(),
            Event::MapEnd,
        ],
    );
    // the tag does not need to come first, the value is replayed
    assert_eq!(
        deserialize::<Command>(vec![
            Event::map_start(),
            "start".into(),
            1u64.into(),
            "end".into(),
            2u64.into(),
            "type".into(),
            "Select".into(),
            Event::MapEnd,
        ])
        .unwrap(),
        Command::Select(Range { start: 1, end: 2 })
    );

    // untagged enums try the next variant if a conversion fails
    let range = vec![
        Event::map_start(),
        "start".into(),
        1u64.into(),
        "end".into(),
        2u64.into(),
        Event::MapEnd,
    ];
    assert_eq!(
        deserialize::<Bound>(range).unwrap(),
        Bound::Range(Range { start: 1, end: 2 })
    );
    let err = deserialize::<Bound>(vec![
        Event::map_start(),
        "start".into(),
        2u64.into(),
        "end".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
    assert_eq!(
        deserialize::<Bound>(vec!["x".into()]).unwrap(),
        Bound::Name("x".into())
    );
}

#[derive(Default)]
struct Names(Vec<String>);

impl Describe for Names {
    fn structure(&mut self, name: &str) {
        self.0.push(format!("structure {}", name));
    }

    fn newtype(&mut self, name: &str) {
        self.0.push(format!("newtype {}", name));
    }

    fn some(&mut self) {
        self.0.push("some".into());
    }
}

#[derive(Clone, Serialize)]
#[deser(as = FromInto<Option<u32>>, rename = "Renamed")]
struct Described(Option<u32>);

impl From<Described> for Option<u32> {
    fn from(value: Described) -> Option<u32> {
        value.0
    }
}

#[test]
fn test_describe() {
    let mut names = Names::default();
    Email {
        user: "jane".into(),
        domain: "example.com".into(),
    }
    .describe(&mut names);
    assert_eq!(names.0, ["newtype Email"]);

    // the name can be changed with rename and the adapter describes itself
    // (FromInto does not describe the value it converts to)
    let mut names = Names::default();
    Described(Some(1)).describe(&mut names);
    assert_eq!(names.0, ["newtype Renamed"]);

    let mut names = Names::default();
    Version { major: 1, minor: 0 }.describe(&mut names);
    assert_eq!(names.0, ["newtype Version"]);
}

/// A byte which forwards all of its implementation to `u8`, including the
/// hidden methods that make vectors of bytes serialize as bytes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[deser(as = ViaU8)]
#[repr(transparent)]
struct Byte(u8);

struct ViaU8;

impl SerializeAs<Byte> for ViaU8 {
    fn serialize_as<'a>(value: &'a Byte, state: &mut State) -> Result<Chunk<'a>, Error> {
        value.0.serialize(state)
    }

    fn __private_slice_as_bytes_as(val: &[Byte]) -> Option<Cow<'_, [u8]>> {
        // SAFETY: `Byte` is transparent over `u8`
        Some(Cow::Borrowed(unsafe {
            &*(val as *const [Byte] as *const [u8])
        }))
    }
}

impl<'de> DeserializeAs<'de, Byte> for ViaU8 {
    fn deserialize_into_as(out: &mut Option<Byte>) -> SinkHandle<'_, 'de> {
        <FromInto<u8> as DeserializeAs<'de, Byte>>::deserialize_into_as(out)
    }

    fn __private_is_bytes_as() -> bool {
        true
    }

    fn __private_vec_from_bytes_as(bytes: Vec<u8>) -> Option<Vec<Byte>> {
        Some(bytes.into_iter().map(Byte).collect())
    }

    fn __private_array_from_bytes_as<const N: usize>(bytes: &[u8]) -> Option<[Byte; N]> {
        <[u8; N]>::try_from(bytes).ok().map(|x| x.map(Byte))
    }
}

impl From<u8> for Byte {
    fn from(value: u8) -> Byte {
        Byte(value)
    }
}

#[test]
fn test_bytes_hooks() {
    // the derive forwards the hidden methods to the adapter
    let bytes = vec![Byte(1), Byte(2)];
    assert_eq!(serialize(&bytes), vec![Event::from(&b"\x01\x02"[..])]);
    assert_eq!(
        deserialize::<Vec<Byte>>(vec![Event::from(&b"\x01\x02"[..])]).unwrap(),
        bytes
    );
    assert_eq!(
        deserialize::<[Byte; 2]>(vec![Event::from(&b"\x01\x02"[..])]).unwrap(),
        [Byte(1), Byte(2)]
    );

    // and so does `As`
    let bytes: Vec<As<u8, Same>> = vec![As::new(1), As::new(2)];
    assert_eq!(serialize(&bytes), vec![Event::from(&b"\x01\x02"[..])]);
    let bytes: Vec<As<u8, Same>> = deserialize(vec![Event::from(&b"\x01\x02"[..])]).unwrap();
    assert_eq!(bytes.iter().map(|x| **x).collect::<Vec<_>>(), [1, 2]);
    let bytes: [As<u8, Same>; 2] = deserialize(vec![Event::from(&b"\x01\x02"[..])]).unwrap();
    assert_eq!(bytes.map(|x| x.into_inner()), [1, 2]);
}
