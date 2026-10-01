//! Adapters on enum variants (`#[deser(as = ...)]` on variants).
use std::fmt::{self, Debug, Display};
use std::str::FromStr;

use deser::adapters::{DisplayFromStr, FromInto};
use deser::de::{DeserializeOwned, Slot, default_atom};
use deser::ser::{Emit, SerializeDriver};
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize, State};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
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
        // the shapes are not compared
        events.push(match event {
            Event::MapStart(_) => Event::map_start(),
            Event::SeqStart(_) => Event::seq_start(),
            event => event.to_static(),
        });
    }
    events
}

/// Checks the serialized form and that it deserializes back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
    let expected: Vec<_> = events.iter().map(|x| x.to_static()).collect();
    assert_eq!(serialize(&value), expected);
    assert_eq!(deserialize::<T>(events).unwrap(), value);
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

/// Writes two numbers as `"a,b"`.
///
/// The fields of the variant are given to the adapter as a tuple, of
/// references when serializing.
struct Joined;

impl Serialize<(&u32, &u32)> for Joined {
    fn serialize<'a>(value: &'a (&u32, &u32), _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Str(
            format!("{},{}", value.0, value.1).into(),
        )))
    }
}

impl<'de> Deserialize<'de, (u32, u32)> for Joined {
    fn deserialize_atom(
        slot: &mut Slot<(u32, u32), Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            Atom::Str(ref s) => {
                let invalid = || Error::new(ErrorKind::Unexpected, "invalid pair");
                let (a, b) = s.split_once(',').ok_or_else(invalid)?;
                slot.set((
                    a.parse().map_err(|_| invalid())?,
                    b.parse().map_err(|_| invalid())?,
                ));
                Ok(())
            }
            other => default_atom(slot, other, state),
        }
    }
}

/// Writes `()` as `true`.
struct Marker;

impl Serialize<()> for Marker {
    fn serialize<'a>(_value: &'a (), _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Bool(true)))
    }
}

impl<'de> Deserialize<'de, ()> for Marker {
    fn deserialize_atom(
        slot: &mut Slot<(), Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            Atom::Bool(true) => {
                slot.set(());
                Ok(())
            }
            other => default_atom(slot, other, state),
        }
    }
}

/// The content of `Moved` as a struct.
#[derive(Serialize, Deserialize)]
struct Coords {
    x: u32,
    y: u32,
}

impl From<(&u32, &u32)> for Coords {
    fn from((x, y): (&u32, &u32)) -> Coords {
        Coords { x: *x, y: *y }
    }
}

impl From<Coords> for (u32, u32) {
    fn from(value: Coords) -> (u32, u32) {
        (value.x, value.y)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum External {
    #[deser(as = Joined)]
    Pair(u32, u32),
    #[deser(as = Joined)]
    Range {
        start: u32,
        end: u32,
    },
    #[deser(as = DisplayFromStr)]
    Newtype(u32),
    #[deser(as = DisplayFromStr)]
    Named {
        value: u32,
    },
    #[deser(as = Marker)]
    Unit,
    Plain,
}

#[test]
fn test_externally_tagged() {
    check(External::Pair(1, 2), map(&[("Pair", "1,2".into())]));
    check(
        External::Range { start: 1, end: 2 },
        map(&[("Range", "1,2".into())]),
    );
    check(External::Newtype(1), map(&[("Newtype", "1".into())]));
    check(External::Named { value: 1 }, map(&[("Named", "1".into())]));
    // a unit variant with an adapter has content
    check(External::Unit, map(&[("Unit", true.into())]));
    check(External::Plain, vec!["Plain".into()]);
    // the tag alone gives null to the adapter
    assert_eq!(
        deserialize::<External>(vec!["Unit".into()])
            .unwrap_err()
            .message(),
        "unexpected null, expected Marker"
    );
    assert_eq!(
        deserialize::<External>(map(&[("Pair", "1".into())]))
            .unwrap_err()
            .message(),
        "invalid pair"
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t")]
enum Internal {
    #[deser(as = FromInto<Coords>)]
    Moved(u32, u32),
    #[deser(as = FromInto<Coords>)]
    Point {
        a: u32,
        b: u32,
    },
    Other {
        value: u32,
    },
}

#[test]
fn test_internally_tagged() {
    check(
        Internal::Moved(1, 2),
        map(&[
            ("t", "Moved".into()),
            ("x", 1u64.into()),
            ("y", 2u64.into()),
        ]),
    );
    check(
        Internal::Point { a: 1, b: 2 },
        map(&[
            ("t", "Point".into()),
            ("x", 1u64.into()),
            ("y", 2u64.into()),
        ]),
    );
    // the tag can come last
    assert_eq!(
        deserialize::<Internal>(map(&[
            ("x", 1u64.into()),
            ("y", 2u64.into()),
            ("t", "Moved".into()),
        ]))
        .unwrap(),
        Internal::Moved(1, 2)
    );
    check(
        Internal::Other { value: 1 },
        map(&[("t", "Other".into()), ("value", 1u64.into())]),
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
enum Adjacent {
    #[deser(as = Joined)]
    Pair(u32, u32),
    #[deser(as = Marker)]
    Unit,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum Untagged {
    #[deser(as = Joined)]
    Pair(u32, u32),
    #[deser(as = DisplayFromStr)]
    Number(u32),
}

#[test]
fn test_adjacently_tagged_and_untagged() {
    check(
        Adjacent::Pair(1, 2),
        map(&[("t", "Pair".into()), ("c", "1,2".into())]),
    );
    check(
        Adjacent::Unit,
        map(&[("t", "Unit".into()), ("c", true.into())]),
    );
    check(Untagged::Pair(1, 2), vec!["1,2".into()]);
    check(Untagged::Number(1), vec!["1".into()]);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Skips {
    #[deser(as = Joined)]
    Tuple(u32, #[deser(skip, default = 7)] u8, u32),
    // serialized as a struct
    #[deser(deserialize_as = DisplayFromStr)]
    Struct {
        value: u32,
        #[deser(skip)]
        cache: Vec<u32>,
        #[deser(skip_deserializing, default = "x")]
        note: String,
    },
}

#[test]
fn test_skipped_fields() {
    check(Skips::Tuple(1, 7, 2), map(&[("Tuple", "1,2".into())]));
    let value = Skips::Struct {
        value: 1,
        cache: vec![1],
        note: "y".into(),
    };
    let mut events = vec![Event::map_start(), "Struct".into()];
    events.extend(map(&[("value", 1u64.into()), ("note", "y".into())]));
    events.push(Event::MapEnd);
    assert_eq!(serialize(&value), events);
    assert_eq!(
        deserialize::<Skips>(map(&[("Struct", "1".into())])).unwrap(),
        Skips::Struct {
            value: 1,
            cache: Vec::new(),
            note: "x".into(),
        }
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t")]
enum Directional {
    // read with the adapter, written as a map with renamed fields
    #[deser(deserialize_as = Joined, rename_all = "UPPERCASE")]
    Read {
        #[deser(skip_serializing_if = is_zero)]
        a: u32,
        b: u32,
    },
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[test]
fn test_directional() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Written {
        #[deser(serialize_as = Joined)]
        Pair(#[deser(deserialize_as = DisplayFromStr)] u32, u32),
    }

    assert_eq!(
        serialize(&Written::Pair(1, 2)),
        map(&[("Pair", "1,2".into())])
    );
    assert_eq!(
        deserialize::<Written>(vec![
            Event::map_start(),
            "Pair".into(),
            Event::seq_start(),
            "1".into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ])
        .unwrap(),
        Written::Pair(1, 2)
    );

    assert_eq!(
        serialize(&Directional::Read { a: 0, b: 2 }),
        map(&[("t", "Read".into()), ("B", 2u64.into())])
    );
    let rv = deserialize::<Directional>(map(&[("t", "Read".into()), ("a", 1u64.into())]));
    // the adapter receives the map (without the tag)
    assert_eq!(rv.unwrap_err().message(), "unexpected map, expected Joined");
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "lowercase")]
enum WithOther {
    Known(u32),
    #[deser(other, as = DisplayFromStr)]
    Unknown(#[deser(tag)] String, u32),
}

#[test]
fn test_other_variant() {
    check(WithOther::Known(1), map(&[("known", 1u64.into())]));
    // the tag field is not part of the content
    check(
        WithOther::Unknown("custom".into(), 1),
        map(&[("custom", "1".into())]),
    );
}

/// Only displays and parses, does not implement `Serialize` and
/// `Deserialize`.
#[derive(Debug, PartialEq)]
struct Code(u32);

impl Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C{}", self.0)
    }
}

impl FromStr for Code {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Code, Self::Err> {
        s.strip_prefix('C')
            .and_then(|x| x.parse().ok())
            .map(Code)
            .ok_or("invalid code")
    }
}

/// `T` only appears in variants with adapters, it does not need to
/// implement `Serialize` or `Deserialize`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Generic<T, U> {
    #[deser(as = (DisplayFromStr, DisplayFromStr))]
    Pair(T, T),
    #[deser(as = DisplayFromStr)]
    One(T),
    Plain(U),
}

#[test]
fn test_generics() {
    check(
        Generic::<Code, u32>::Pair(Code(1), Code(2)),
        vec![
            Event::map_start(),
            "Pair".into(),
            Event::seq_start(),
            "C1".into(),
            "C2".into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        Generic::<Code, u32>::One(Code(1)),
        map(&[("One", "C1".into())]),
    );
    check(
        Generic::<Code, u32>::Plain(1),
        map(&[("Plain", 1u64.into())]),
    );
}
