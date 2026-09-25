use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{self, Debug, Display};
use std::net::IpAddr;
use std::str::FromStr;

use deser::adapters::{
    As, DefaultOnError, DeserializeAs, DisplayFromStr, FromInto, MapSkipError, SerializeAs,
    TryFromInto, VecSkipError,
};
use deser::de::{DeserializeDriver, Recording, SinkHandle};
use deser::ser::{Chunk, SerializeDriver, SerializeHandle};
use deser::{make_slot_wrapper, Atom, Deserialize, Error, ErrorKind, Event, Serialize, State};

fn deserialize<T: Deserialize>(events: Vec<Event<'_>>) -> Result<T, Error> {
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

fn serialize_drive(value: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    SerializeDriver::new(value)
        .drive(|event, _, _| {
            events.push(event.to_static());
            Ok(())
        })
        .unwrap();
    events
}

/// Checks the serialized form (with both driver interfaces) and that it
/// deserializes back.
fn check<T: Serialize + Deserialize + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
    let expected = events.iter().map(|x| x.to_static()).collect::<Vec<_>>();
    assert_eq!(serialize(&value), expected);
    assert_eq!(serialize_drive(&value), expected);
    assert_eq!(deserialize::<T>(events).unwrap(), value);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Server {
    #[deser(as = DisplayFromStr)]
    addr: IpAddr,
    #[deser(as = Option<DisplayFromStr>)]
    fallback: Option<IpAddr>,
    #[deser(as = BTreeMap<_, Vec<DisplayFromStr>>)]
    aliases: BTreeMap<String, Vec<u16>>,
}

#[test]
fn test_display_from_str() {
    let mut aliases = BTreeMap::new();
    aliases.insert("a".to_string(), vec![1, 2]);
    check(
        Server {
            addr: "127.0.0.1".parse().unwrap(),
            fallback: Some("::1".parse().unwrap()),
            aliases,
        },
        vec![
            Event::MapStart,
            "addr".into(),
            "127.0.0.1".into(),
            "fallback".into(),
            "::1".into(),
            "aliases".into(),
            Event::MapStart,
            "a".into(),
            Event::SeqStart,
            "1".into(),
            "2".into(),
            Event::SeqEnd,
            Event::MapEnd,
            Event::MapEnd,
        ],
    );

    // optional adapters make missing fields optional
    let server: Server = deserialize(vec![
        Event::MapStart,
        "addr".into(),
        "127.0.0.1".into(),
        "fallback".into(),
        ().into(),
        "aliases".into(),
        Event::MapStart,
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(server.fallback, None);
    let server: Server = deserialize(vec![
        Event::MapStart,
        "addr".into(),
        "127.0.0.1".into(),
        "aliases".into(),
        Event::MapStart,
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(server.fallback, None);

    // required fields are still required
    let err = deserialize::<Server>(vec![
        Event::MapStart,
        "aliases".into(),
        Event::MapStart,
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);

    // only strings are accepted
    let err = deserialize::<Server>(vec![
        Event::MapStart,
        "addr".into(),
        42u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected unsigned integer, expected string"
    );
    let err = deserialize::<Server>(vec![
        Event::MapStart,
        "addr".into(),
        "nope".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: invalid value: invalid IP address syntax"
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Containers {
    #[deser(as = (DisplayFromStr, _))]
    tuple: (u32, u32),
    #[deser(as = [DisplayFromStr; 2])]
    array: [u32; 2],
    #[deser(as = Box<DisplayFromStr>)]
    boxed: Box<u32>,
    #[deser(as = BTreeSet<DisplayFromStr>)]
    set: BTreeSet<u32>,
    #[deser(as = HashMap<DisplayFromStr, _>)]
    map: HashMap<u32, bool>,
    #[deser(as = Option<Vec<_>>)]
    bytes: Option<Vec<u8>>,
    #[deser(as = [_; 2])]
    byte_array: [u8; 2],
}

#[test]
fn test_containers() {
    let mut map = HashMap::new();
    map.insert(1, true);
    check(
        Containers {
            tuple: (1, 2),
            array: [3, 4],
            boxed: Box::new(5),
            set: [6].into_iter().collect(),
            map,
            bytes: Some(vec![1, 2, 3]),
            byte_array: [4, 5],
        },
        vec![
            Event::MapStart,
            "tuple".into(),
            Event::SeqStart,
            "1".into(),
            2u64.into(),
            Event::SeqEnd,
            "array".into(),
            Event::SeqStart,
            "3".into(),
            "4".into(),
            Event::SeqEnd,
            "boxed".into(),
            "5".into(),
            "set".into(),
            Event::SeqStart,
            "6".into(),
            Event::SeqEnd,
            "map".into(),
            Event::MapStart,
            "1".into(),
            true.into(),
            Event::MapEnd,
            "bytes".into(),
            (&b"\x01\x02\x03"[..]).into(),
            "byte_array".into(),
            (&b"\x04\x05"[..]).into(),
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, Clone, PartialEq)]
struct Rgb(u8, u8, u8);

impl From<(u8, u8, u8)> for Rgb {
    fn from(value: (u8, u8, u8)) -> Rgb {
        Rgb(value.0, value.1, value.2)
    }
}

impl From<Rgb> for (u8, u8, u8) {
    fn from(value: Rgb) -> (u8, u8, u8) {
        (value.0, value.1, value.2)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Percent(u8);

impl TryFrom<u64> for Percent {
    type Error = &'static str;

    fn try_from(value: u64) -> Result<Percent, Self::Error> {
        if value <= 100 {
            Ok(Percent(value as u8))
        } else {
            Err("out of range")
        }
    }
}

impl TryFrom<Percent> for u64 {
    type Error = &'static str;

    fn try_from(value: Percent) -> Result<u64, Self::Error> {
        if value.0 <= 100 {
            Ok(value.0 as u64)
        } else {
            Err("bad percent")
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Conversions {
    #[deser(as = FromInto<(u8, u8, u8)>)]
    color: Rgb,
    #[deser(as = Vec<FromInto<(u8, u8, u8)>>)]
    colors: Vec<Rgb>,
    #[deser(as = TryFromInto<u64>)]
    done: Percent,
}

#[test]
fn test_conversions() {
    check(
        Conversions {
            color: Rgb(1, 2, 3),
            colors: vec![Rgb(4, 5, 6)],
            done: Percent(50),
        },
        vec![
            Event::MapStart,
            "color".into(),
            Event::SeqStart,
            1u64.into(),
            2u64.into(),
            3u64.into(),
            Event::SeqEnd,
            "colors".into(),
            Event::SeqStart,
            Event::SeqStart,
            4u64.into(),
            5u64.into(),
            6u64.into(),
            Event::SeqEnd,
            Event::SeqEnd,
            "done".into(),
            50u64.into(),
            Event::MapEnd,
        ],
    );

    let err = deserialize::<Conversions>(vec![
        Event::MapStart,
        "done".into(),
        101u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.to_string(), "Unexpected: invalid value: out of range");

    let value = Conversions {
        color: Rgb(1, 2, 3),
        colors: vec![],
        done: Percent(101),
    };
    let err = SerializeDriver::new(&value)
        .drive(|_, _, _| Ok(()))
        .unwrap_err();
    assert_eq!(err.to_string(), "Unexpected: invalid value: bad percent");
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Kind {
    A,
    B,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
struct Point {
    x: u32,
    y: u32,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Lenient {
    #[deser(as = DefaultOnError)]
    kind: Option<Kind>,
    #[deser(as = DefaultOnError)]
    point: Point,
    #[deser(as = VecSkipError)]
    kinds: Vec<Kind>,
    #[deser(as = VecSkipError)]
    points: Vec<Point>,
    #[deser(as = MapSkipError)]
    weights: BTreeMap<Kind, u32>,
    #[deser(as = MapSkipError<_, DisplayFromStr>)]
    named: HashMap<String, u32>,
    #[deser(as = MapSkipError)]
    keyed: BTreeMap<(u32, u32), u32>,
}

#[test]
fn test_error_recovery() {
    let value: Lenient = deserialize(vec![
        Event::MapStart,
        "kind".into(),
        "C".into(),
        "point".into(),
        Event::MapStart,
        "x".into(),
        "wrong".into(),
        Event::MapEnd,
        "kinds".into(),
        Event::SeqStart,
        "A".into(),
        "C".into(),
        "B".into(),
        Event::SeqEnd,
        "points".into(),
        Event::SeqStart,
        Event::MapStart,
        "x".into(),
        1u64.into(),
        "y".into(),
        2u64.into(),
        Event::MapEnd,
        Event::MapStart,
        "x".into(),
        1u64.into(),
        Event::MapEnd,
        Event::SeqStart,
        Event::SeqEnd,
        Event::MapStart,
        "x".into(),
        3u64.into(),
        "y".into(),
        4u64.into(),
        Event::MapEnd,
        Event::SeqEnd,
        "weights".into(),
        Event::MapStart,
        "A".into(),
        1u64.into(),
        "C".into(),
        2u64.into(),
        "B".into(),
        "x".into(),
        "B".into(),
        Event::MapStart,
        Event::MapEnd,
        Event::MapEnd,
        "named".into(),
        Event::MapStart,
        "a".into(),
        "1".into(),
        "b".into(),
        "x".into(),
        Event::MapEnd,
        "keyed".into(),
        Event::MapStart,
        Event::SeqStart,
        1u64.into(),
        2u64.into(),
        Event::SeqEnd,
        3u64.into(),
        Event::SeqStart,
        1u64.into(),
        Event::SeqEnd,
        4u64.into(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.kind, None);
    assert_eq!(value.point, Point::default());
    assert_eq!(value.kinds, vec![Kind::A, Kind::B]);
    assert_eq!(
        value.points,
        vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]
    );
    assert_eq!(
        value.weights.into_iter().collect::<Vec<_>>(),
        [(Kind::A, 1)]
    );
    assert_eq!(
        value.named.into_iter().collect::<Vec<_>>(),
        [("a".into(), 1)]
    );
    assert_eq!(value.keyed.into_iter().collect::<Vec<_>>(), [((1, 2), 3)]);

    let value: Lenient = deserialize(vec![
        Event::MapStart,
        "kind".into(),
        "B".into(),
        "point".into(),
        Event::MapStart,
        "x".into(),
        1u64.into(),
        "y".into(),
        2u64.into(),
        Event::MapEnd,
        "kinds".into(),
        Event::SeqStart,
        Event::SeqEnd,
        "points".into(),
        Event::SeqStart,
        Event::SeqEnd,
        "weights".into(),
        Event::MapStart,
        Event::MapEnd,
        "named".into(),
        Event::MapStart,
        Event::MapEnd,
        "keyed".into(),
        Event::MapStart,
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.kind, Some(Kind::B));
    assert_eq!(value.point, Point { x: 1, y: 2 });
}

/// A custom adapter that represents bytes as hex strings.
struct Hex;

impl SerializeAs<Vec<u8>> for Hex {
    fn serialize_as<'a>(value: &'a Vec<u8>, _state: &mut State) -> Result<Chunk<'a>, Error> {
        let hex: String = value.iter().map(|x| format!("{:02x}", x)).collect();
        Ok(Chunk::Atom(Atom::Str(hex.into())))
    }
}

make_slot_wrapper!(HexSlot);

impl deser::de::Sink for HexSlot<Vec<u8>> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref s) if s.len() % 2 == 0 => {
                let bytes = (0..s.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| Error::new(ErrorKind::Unexpected, "invalid hex"))?;
                **self = Some(bytes);
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

impl DeserializeAs<Vec<u8>> for Hex {
    fn deserialize_into_as(out: &mut Option<Vec<u8>>) -> SinkHandle<'_> {
        HexSlot::make_handle(out)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    #[deser(as = Hex)]
    data: Vec<u8>,
    #[deser(as = BTreeMap<_, Hex>)]
    parts: BTreeMap<String, Vec<u8>>,
}

#[test]
fn test_custom_adapter() {
    let mut parts = BTreeMap::new();
    parts.insert("a".to_string(), vec![0xff]);
    check(
        Blob {
            data: vec![1, 2, 0xab],
            parts,
        },
        vec![
            Event::MapStart,
            "data".into(),
            "0102ab".into(),
            "parts".into(),
            Event::MapStart,
            "a".into(),
            "ff".into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Port(#[deser(as = DisplayFromStr)] u16);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OptionalPort(#[deser(as = Option<DisplayFromStr>)] Option<u16>);

#[test]
fn test_newtype_struct() {
    check(Port(80), vec!["80".into()]);
    check(OptionalPort(Some(80)), vec!["80".into()]);
    check(OptionalPort(None), vec![().into()]);
    check(
        vec![Port(1), Port(2)],
        vec![Event::SeqStart, "1".into(), "2".into(), Event::SeqEnd],
    );
}

/// Only displays and parses, does not implement `Serialize` and `Deserialize`.
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

/// `T` only appears in fields with adapters, so it does not need to
/// implement `Serialize` or `Deserialize`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Codes<T, U> {
    #[deser(as = Vec<DisplayFromStr>)]
    codes: Vec<T>,
    other: U,
}

#[test]
fn test_generics() {
    check(
        Codes {
            codes: vec![Code(1), Code(2)],
            other: 42u32,
        },
        vec![
            Event::MapStart,
            "codes".into(),
            Event::SeqStart,
            "C1".into(),
            "C2".into(),
            Event::SeqEnd,
            "other".into(),
            42u64.into(),
            Event::MapEnd,
        ],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(skip_serializing_optionals)]
struct SkipOptionals {
    #[deser(as = Option<DisplayFromStr>)]
    a: Option<u32>,
    #[deser(as = Option<DisplayFromStr>)]
    b: Option<u32>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(skip_serializing_optionals)]
struct SkipOptionalsFlatten {
    #[deser(as = Option<DisplayFromStr>)]
    a: Option<u32>,
    #[deser(flatten)]
    inner: SkipOptionals,
}

#[test]
fn test_skip_serializing_optionals() {
    check(
        SkipOptionals {
            a: Some(1),
            b: None,
        },
        vec![Event::MapStart, "a".into(), "1".into(), Event::MapEnd],
    );
    check(
        SkipOptionalsFlatten {
            a: None,
            inner: SkipOptionals {
                a: None,
                b: Some(2),
            },
        },
        vec![Event::MapStart, "b".into(), "2".into(), Event::MapEnd],
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum ExternalAdapters {
    Newtype(#[deser(as = DisplayFromStr)] u32),
    Tuple(#[deser(as = DisplayFromStr)] u32, u32),
    Struct {
        #[deser(as = Option<DisplayFromStr>)]
        a: Option<u32>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "t")]
enum InternalAdapters {
    Newtype(#[deser(as = FromInto<BTreeMap<String, u32>>)] Wrapped),
    Struct {
        #[deser(as = DisplayFromStr)]
        a: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Wrapped(BTreeMap<String, u32>);

impl From<BTreeMap<String, u32>> for Wrapped {
    fn from(value: BTreeMap<String, u32>) -> Wrapped {
        Wrapped(value)
    }
}

impl From<Wrapped> for BTreeMap<String, u32> {
    fn from(value: Wrapped) -> BTreeMap<String, u32> {
        value.0
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(untagged)]
enum UntaggedAdapters {
    Number(u32),
    Text(#[deser(as = DisplayFromStr)] IpAddr),
}

#[test]
fn test_enum_variants() {
    check(
        ExternalAdapters::Newtype(1),
        vec![Event::MapStart, "Newtype".into(), "1".into(), Event::MapEnd],
    );
    check(
        ExternalAdapters::Tuple(1, 2),
        vec![
            Event::MapStart,
            "Tuple".into(),
            Event::SeqStart,
            "1".into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        ExternalAdapters::Struct { a: Some(1) },
        vec![
            Event::MapStart,
            "Struct".into(),
            Event::MapStart,
            "a".into(),
            "1".into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
    assert_eq!(
        deserialize::<ExternalAdapters>(vec![
            Event::MapStart,
            "Struct".into(),
            Event::MapStart,
            Event::MapEnd,
            Event::MapEnd,
        ])
        .unwrap(),
        ExternalAdapters::Struct { a: None }
    );

    let mut map = BTreeMap::new();
    map.insert("x".to_string(), 1);
    check(
        InternalAdapters::Newtype(Wrapped(map)),
        vec![
            Event::MapStart,
            "t".into(),
            "Newtype".into(),
            "x".into(),
            1u64.into(),
            Event::MapEnd,
        ],
    );
    check(
        InternalAdapters::Struct { a: 1 },
        vec![
            Event::MapStart,
            "t".into(),
            "Struct".into(),
            "a".into(),
            "1".into(),
            Event::MapEnd,
        ],
    );

    check(UntaggedAdapters::Number(1), vec![1u64.into()]);
    check(
        UntaggedAdapters::Text("127.0.0.1".parse().unwrap()),
        vec!["127.0.0.1".into()],
    );
}

#[test]
fn test_as_wrapper() {
    check(
        vec![As::<u32, DisplayFromStr>::new(1), As::new(2)],
        vec![Event::SeqStart, "1".into(), "2".into(), Event::SeqEnd],
    );
    let values: Vec<As<Option<u32>, Option<DisplayFromStr>>> =
        deserialize(vec![Event::SeqStart, "1".into(), ().into(), Event::SeqEnd]).unwrap();
    assert_eq!(
        values.into_iter().map(As::into_inner).collect::<Vec<_>>(),
        [Some(1), None]
    );
}

/// A value that serializes by forwarding to another value.
struct Forwarding<'a> {
    inner: &'a dyn Serialize,
    log: &'a std::cell::RefCell<Vec<&'static str>>,
}

impl<'a> Serialize for Forwarding<'a> {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Forward(SerializeHandle::Borrowed(self.inner)))
    }

    fn finish(&self, _state: &mut State) -> Result<(), Error> {
        self.log.borrow_mut().push("outer");
        Ok(())
    }
}

struct Logged<'a, T> {
    value: T,
    log: &'a std::cell::RefCell<Vec<&'static str>>,
}

impl<'a, T: Serialize> Serialize for Logged<'a, T> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.value.serialize(state)
    }

    fn finish(&self, _state: &mut State) -> Result<(), Error> {
        self.log.borrow_mut().push("inner");
        Ok(())
    }
}

#[test]
fn test_forward() {
    let log = std::cell::RefCell::new(Vec::new());
    for value in [
        &1u32 as &dyn Serialize,
        &vec![1u32, 2] as &dyn Serialize,
        &(1u32, "x") as &dyn Serialize,
    ] {
        let inner = Logged { value, log: &log };
        let forwarding = Forwarding {
            inner: &inner,
            log: &log,
        };
        // forwarding twice
        let outer = Forwarding {
            inner: &forwarding,
            log: &log,
        };
        let expected = serialize(value);
        assert_eq!(serialize(&outer), expected);
        assert_eq!(&log.borrow()[..], ["inner", "outer", "outer"]);
        log.borrow_mut().clear();
        assert_eq!(serialize_drive(&outer), expected);
        assert_eq!(&log.borrow()[..], ["inner", "outer", "outer"]);
        log.borrow_mut().clear();

        // within containers
        let values = vec![&outer as &dyn Serialize, &outer];
        let mut expected_seq = vec![Event::SeqStart];
        expected_seq.extend(expected.iter().cloned());
        expected_seq.extend(expected.iter().cloned());
        expected_seq.push(Event::SeqEnd);
        assert_eq!(serialize(&values), expected_seq);
        assert_eq!(serialize_drive(&values), expected_seq);
        log.borrow_mut().clear();
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Tag(u64);

#[test]
fn test_recording_raw_value() {
    let events = vec![
        Event::MapStart,
        "a".into(),
        Event::SeqStart,
        1u64.into(),
        Event::MapStart,
        Event::MapEnd,
        Event::SeqEnd,
        "b".into(),
        ().into(),
        Event::MapEnd,
    ];
    let recording: Recording = deserialize(events.clone()).unwrap();
    assert_eq!(serialize(&recording), events);
    assert_eq!(serialize_drive(&recording), events);

    let recording: Recording = deserialize(vec!["x".into()]).unwrap();
    assert_eq!(serialize(&recording), vec![Event::from("x")]);

    // event data survives a round trip
    let mut out = None::<Recording>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::SeqStart).unwrap();
        driver
            .emit_with(1u64, |state| state.event_mut::<Tag>().0 = 42)
            .unwrap();
        driver.emit(2u64).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
    }
    let recording = out.unwrap();
    let mut tags = Vec::new();
    SerializeDriver::new(&recording)
        .drive(|event, _, state| {
            tags.push((event.to_static(), state.event::<Tag>().cloned()));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        tags,
        vec![
            (Event::SeqStart, None),
            (1u64.into(), Some(Tag(42))),
            (2u64.into(), None),
            (Event::SeqEnd, None),
        ]
    );
}
