use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{self, Debug, Display};
use std::net::IpAddr;
use std::str::FromStr;

use deser::adapters::{
    As, DefaultOnError, DisplayFromStr, FromInto, MapSkipError, TryFromInto, VecSkipError,
};
use deser::de::{DeserializeDriver, DeserializeOwned, Recording, Slot, default_atom};
use deser::ser::{Emit, SerializeDriver, SerializeHandle, SerializeRef};
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize, State};

/// Removes the length from container starts, the tests are not about it.
fn without_len(event: deser::Event<'static>) -> deser::Event<'static> {
    match event {
        deser::Event::MapStart(shape) => {
            deser::Event::MapStart(deser::ContainerShape::with_order(shape.order()))
        }
        deser::Event::SeqStart(shape) => {
            deser::Event::SeqStart(deser::ContainerShape::with_order(shape.order()))
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

/// Deserializes with the lexical rules of formats where everything is text
/// (like query strings).
fn deserialize_lenient<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        deser::de::LexicalRules::LENIENT.set(driver.state_mut());
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
    events
}

fn serialize_drive<T: Serialize + ?Sized>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    SerializeDriver::new(&value)
        .drive(|event, _| {
            events.push(without_len(event.to_static()));
            Ok(())
        })
        .unwrap();
    events
}

/// Checks the serialized form (with both driver interfaces) and that it
/// deserializes back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, events: Vec<Event<'_>>) {
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
            Event::map_start(),
            "addr".into(),
            "127.0.0.1".into(),
            "fallback".into(),
            "::1".into(),
            "aliases".into(),
            Event::MapStart(deser::ContainerShape::with_order(deser::Order::Sorted)),
            "a".into(),
            Event::seq_start(),
            "1".into(),
            "2".into(),
            Event::SeqEnd,
            Event::MapEnd,
            Event::MapEnd,
        ],
    );

    // optional adapters make missing fields optional
    let server: Server = deserialize(vec![
        Event::map_start(),
        "addr".into(),
        "127.0.0.1".into(),
        "fallback".into(),
        ().into(),
        "aliases".into(),
        Event::map_start(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(server.fallback, None);
    let server: Server = deserialize(vec![
        Event::map_start(),
        "addr".into(),
        "127.0.0.1".into(),
        "aliases".into(),
        Event::map_start(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(server.fallback, None);

    // required fields are still required
    let err = deserialize::<Server>(vec![
        Event::map_start(),
        "aliases".into(),
        Event::map_start(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);

    // only strings are accepted
    let err = deserialize::<Server>(vec![
        Event::map_start(),
        "addr".into(),
        42u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidType: unexpected unsigned integer, expected string"
    );
    let err = deserialize::<Server>(vec![
        Event::map_start(),
        "addr".into(),
        "nope".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidValue: invalid value: invalid IP address syntax"
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
            Event::map_start(),
            "tuple".into(),
            Event::seq_start(),
            "1".into(),
            2u64.into(),
            Event::SeqEnd,
            "array".into(),
            Event::seq_start(),
            "3".into(),
            "4".into(),
            Event::SeqEnd,
            "boxed".into(),
            "5".into(),
            "set".into(),
            Event::SeqStart(deser::ContainerShape::with_order(deser::Order::Sorted)),
            "6".into(),
            Event::SeqEnd,
            "map".into(),
            Event::MapStart(deser::ContainerShape::with_order(deser::Order::Arbitrary)),
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
            Event::map_start(),
            "color".into(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            3u64.into(),
            Event::SeqEnd,
            "colors".into(),
            Event::seq_start(),
            Event::seq_start(),
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
        Event::map_start(),
        "done".into(),
        101u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.to_string(), "InvalidValue: invalid value: out of range");

    let value = Conversions {
        color: Rgb(1, 2, 3),
        colors: vec![],
        done: Percent(101),
    };
    let err = SerializeDriver::new(&value)
        .drive(|_, _| Ok(()))
        .unwrap_err();
    assert_eq!(err.to_string(), "InvalidValue: invalid value: bad percent");
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
        Event::map_start(),
        "kind".into(),
        "C".into(),
        "point".into(),
        Event::map_start(),
        "x".into(),
        "wrong".into(),
        Event::MapEnd,
        "kinds".into(),
        Event::seq_start(),
        "A".into(),
        "C".into(),
        "B".into(),
        Event::SeqEnd,
        "points".into(),
        Event::seq_start(),
        Event::map_start(),
        "x".into(),
        1u64.into(),
        "y".into(),
        2u64.into(),
        Event::MapEnd,
        Event::map_start(),
        "x".into(),
        1u64.into(),
        Event::MapEnd,
        Event::seq_start(),
        Event::SeqEnd,
        Event::map_start(),
        "x".into(),
        3u64.into(),
        "y".into(),
        4u64.into(),
        Event::MapEnd,
        Event::SeqEnd,
        "weights".into(),
        Event::map_start(),
        "A".into(),
        1u64.into(),
        "C".into(),
        2u64.into(),
        "B".into(),
        "x".into(),
        "B".into(),
        Event::map_start(),
        Event::MapEnd,
        Event::MapEnd,
        "named".into(),
        Event::map_start(),
        "a".into(),
        "1".into(),
        "b".into(),
        "x".into(),
        Event::MapEnd,
        "keyed".into(),
        Event::map_start(),
        Event::seq_start(),
        1u64.into(),
        2u64.into(),
        Event::SeqEnd,
        3u64.into(),
        Event::seq_start(),
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
        Event::map_start(),
        "kind".into(),
        "B".into(),
        "point".into(),
        Event::map_start(),
        "x".into(),
        1u64.into(),
        "y".into(),
        2u64.into(),
        Event::MapEnd,
        "kinds".into(),
        Event::seq_start(),
        Event::SeqEnd,
        "points".into(),
        Event::seq_start(),
        Event::SeqEnd,
        "weights".into(),
        Event::map_start(),
        Event::MapEnd,
        "named".into(),
        Event::map_start(),
        Event::MapEnd,
        "keyed".into(),
        Event::map_start(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.kind, Some(Kind::B));
    assert_eq!(value.point, Point { x: 1, y: 2 });
}

/// A custom adapter that represents bytes as hex strings.
struct Hex;

impl Serialize<Vec<u8>> for Hex {
    fn serialize<'a>(value: &'a Vec<u8>, _state: &mut State) -> Result<Emit<'a>, Error> {
        let hex: String = value.iter().map(|x| format!("{:02x}", x)).collect();
        Ok(Emit::Atom(Atom::Str(hex.into())))
    }
}

impl<'de> Deserialize<'de, Vec<u8>> for Hex {
    fn deserialize_atom(
        slot: &mut Slot<Vec<u8>, Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            Atom::Str(ref s) if s.len() % 2 == 0 => {
                let bytes = (0..s.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| Error::new(ErrorKind::InvalidValue, "invalid hex"))?;
                slot.set(bytes);
                Ok(())
            }
            other => default_atom(slot, other, state),
        }
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
            Event::map_start(),
            "data".into(),
            "0102ab".into(),
            "parts".into(),
            Event::MapStart(deser::ContainerShape::with_order(deser::Order::Sorted)),
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
        vec![Event::seq_start(), "1".into(), "2".into(), Event::SeqEnd],
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
            Event::map_start(),
            "codes".into(),
            Event::seq_start(),
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
        vec![Event::map_start(), "a".into(), "1".into(), Event::MapEnd],
    );
    check(
        SkipOptionalsFlatten {
            a: None,
            inner: SkipOptionals {
                a: None,
                b: Some(2),
            },
        },
        vec![Event::map_start(), "b".into(), "2".into(), Event::MapEnd],
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
        vec![
            Event::map_start(),
            "Newtype".into(),
            "1".into(),
            Event::MapEnd,
        ],
    );
    check(
        ExternalAdapters::Tuple(1, 2),
        vec![
            Event::map_start(),
            "Tuple".into(),
            Event::seq_start(),
            "1".into(),
            2u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ],
    );
    check(
        ExternalAdapters::Struct { a: Some(1) },
        vec![
            Event::map_start(),
            "Struct".into(),
            Event::map_start(),
            "a".into(),
            "1".into(),
            Event::MapEnd,
            Event::MapEnd,
        ],
    );
    assert_eq!(
        deserialize::<ExternalAdapters>(vec![
            Event::map_start(),
            "Struct".into(),
            Event::map_start(),
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
            Event::map_start(),
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
            Event::map_start(),
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
        vec![Event::seq_start(), "1".into(), "2".into(), Event::SeqEnd],
    );
    let values: Vec<As<Option<u32>, Option<DisplayFromStr>>> = deserialize(vec![
        Event::seq_start(),
        "1".into(),
        ().into(),
        Event::SeqEnd,
    ])
    .unwrap();
    assert_eq!(
        values.into_iter().map(As::into_inner).collect::<Vec<_>>(),
        [Some(1), None]
    );
}

/// A value that serializes by forwarding to another value.
struct Forwarding<'a> {
    inner: SerializeRef<'a>,
    log: &'a std::sync::Mutex<Vec<&'static str>>,
}

impl<'a> Serialize for Forwarding<'a> {
    fn serialize<'b>(value: &'b Self, _state: &mut State) -> Result<Emit<'b>, Error> {
        Ok(Emit::Forward(SerializeHandle::from(value.inner)))
    }

    fn finish(value: &Self, _state: &mut State) -> Result<(), Error> {
        value.log.lock().unwrap().push("outer");
        Ok(())
    }
}

struct Logged<'a, T> {
    value: T,
    log: &'a std::sync::Mutex<Vec<&'static str>>,
}

impl<'a, T: Serialize> Serialize for Logged<'a, T> {
    fn serialize<'b>(this: &'b Self, state: &mut State) -> Result<Emit<'b>, Error> {
        T::serialize(&this.value, state)
    }

    fn finish(value: &Self, _state: &mut State) -> Result<(), Error> {
        value.log.lock().unwrap().push("inner");
        Ok(())
    }
}

#[test]
fn test_forward() {
    let log = std::sync::Mutex::new(Vec::new());
    for value in [
        SerializeRef::new(&1u32),
        SerializeRef::new(&vec![1u32, 2]),
        SerializeRef::new(&(1u32, "x")),
    ] {
        let inner = Logged { value, log: &log };
        let forwarding = Forwarding {
            inner: SerializeRef::new(&inner),
            log: &log,
        };
        // forwarding twice
        let outer = Forwarding {
            inner: SerializeRef::new(&forwarding),
            log: &log,
        };
        let expected = serialize(&value);
        assert_eq!(serialize(&outer), expected);
        assert_eq!(&log.lock().unwrap()[..], ["inner", "outer", "outer"]);
        log.lock().unwrap().clear();
        assert_eq!(serialize_drive(&outer), expected);
        assert_eq!(&log.lock().unwrap()[..], ["inner", "outer", "outer"]);
        log.lock().unwrap().clear();

        // within containers
        let values = vec![SerializeRef::new(&outer), SerializeRef::new(&outer)];
        let mut expected_seq = vec![Event::seq_start()];
        expected_seq.extend(expected.iter().cloned());
        expected_seq.extend(expected.iter().cloned());
        expected_seq.push(Event::SeqEnd);
        assert_eq!(serialize(&values), expected_seq);
        assert_eq!(serialize_drive(&values), expected_seq);
        log.lock().unwrap().clear();
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Tag(u64);

#[test]
fn test_recording_raw_value() {
    let events = vec![
        Event::map_start(),
        "a".into(),
        Event::seq_start(),
        1u64.into(),
        Event::map_start(),
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
        driver.emit(Event::seq_start()).unwrap();
        driver.state_mut().event_mut::<Tag>().0 = 42;
        driver.emit(1u64).unwrap();
        driver.emit(2u64).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
    }
    let recording = out.unwrap();
    let mut tags = Vec::new();
    SerializeDriver::new(&recording)
        .drive(|event, state| {
            tags.push((
                without_len(event.to_static()),
                state.event::<Tag>().cloned(),
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        tags,
        vec![
            (Event::seq_start(), None),
            (1u64.into(), Some(Tag(42))),
            (2u64.into(), None),
            (Event::SeqEnd, None),
        ]
    );
}

#[test]
fn test_flag() {
    use deser::adapters::Flag;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Tree {
        #[deser(as = Flag, skip_serializing_if = std::ops::Not::not)]
        recursive: bool,
        #[deser(as = Option<Flag>)]
        verbose: Option<bool>,
    }

    let with = |value: Option<Event<'static>>| {
        let mut out = None::<Tree>;
        {
            let mut driver = deser::de::DeserializeDriver::new(&mut out);
            driver.emit(Event::map_start())?;
            if let Some(value) = value {
                driver.emit("recursive")?;
                driver.emit(value)?;
            }
            driver.emit(Event::MapEnd)?;
        }
        Ok::<_, deser::Error>(out.unwrap())
    };
    let recursive = |value| with(value).map(|tree| tree.recursive);

    // missing is false, given without value is true
    assert!(!recursive(None).unwrap());
    assert!(recursive(Some(Atom::Lexical("".into()).into())).unwrap());
    assert!(recursive(Some(Event::from(""))).unwrap());
    assert!(recursive(Some(Event::from(()))).unwrap());
    // values are booleans
    assert!(recursive(Some(Event::from(true))).unwrap());
    assert!(!recursive(Some(Event::from(false))).unwrap());
    assert!(recursive(Some(Atom::Lexical("yes".into()).into())).unwrap());
    assert!(!recursive(Some(Atom::Lexical("0".into()).into())).unwrap());
    assert!(!recursive(Some(Event::from("off"))).unwrap());
    let err = recursive(Some(Atom::Lexical("maybe".into()).into())).unwrap_err();
    assert_eq!(
        err.message(),
        "invalid value \"maybe\", expected bool (true, yes, on, 1, false, no, off or 0)"
    );
    assert!(recursive(Some(Event::from(1u64))).is_err());
    assert_eq!(with(None).unwrap().verbose, None);

    let tree = Tree {
        recursive: true,
        verbose: Some(false),
    };
    assert_eq!(
        serialize(&tree),
        vec![
            Event::map_start(),
            "recursive".into(),
            true.into(),
            "verbose".into(),
            false.into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_separated() {
    use deser::adapters::{Separated, TrimWhitespace};
    use std::collections::{HashSet, VecDeque};

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Config {
        #[deser(as = Separated)]
        hosts: Vec<String>,
        #[deser(as = Separated<':'>)]
        path: VecDeque<String>,
        #[deser(as = Separated<',', TrimWhitespace>)]
        ports: BTreeSet<u16>,
        #[deser(as = Option<Separated<',', DisplayFromStr>>, default)]
        addrs: Option<Vec<IpAddr>>,
    }

    let config = |hosts: Event<'static>, path: Event<'static>, ports: Event<'static>| {
        deserialize::<Config>(vec![
            Event::map_start(),
            "hosts".into(),
            hosts,
            "path".into(),
            path,
            "ports".into(),
            ports,
            Event::MapEnd,
        ])
    };
    let lexical = |text: &'static str| Event::from(Atom::Lexical(text.into()));

    // strings and lexical atoms are split, the pieces parse
    let value = config(
        lexical("a,b"),
        Event::from("/usr/bin:/bin"),
        lexical("80, 443 ,80"),
    )
    .unwrap();
    assert_eq!(value.hosts, ["a", "b"]);
    assert_eq!(value.path, ["/usr/bin", "/bin"]);
    assert_eq!(value.ports, BTreeSet::from([80, 443]));
    assert_eq!(value.addrs, None);

    // the empty string is empty, empty pieces are kept
    let value = config(lexical(""), lexical("a::b"), lexical("")).unwrap();
    assert!(value.hosts.is_empty());
    assert_eq!(value.path, ["a", "", "b"]);
    assert!(value.ports.is_empty());

    // sequences are accepted as they are, their elements are not split
    let value = deserialize::<Config>(vec![
        Event::map_start(),
        "hosts".into(),
        Event::seq_start(),
        "a,b".into(),
        "c".into(),
        Event::SeqEnd,
        "path".into(),
        Event::seq_start(),
        Event::SeqEnd,
        "ports".into(),
        Event::seq_start(),
        80u64.into(),
        lexical(" 443 "),
        Event::SeqEnd,
        "addrs".into(),
        lexical("127.0.0.1,::1"),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.hosts, ["a,b", "c"]);
    assert!(value.path.is_empty());
    assert_eq!(value.ports, BTreeSet::from([80, 443]));
    assert_eq!(
        value.addrs,
        Some(vec![
            "127.0.0.1".parse::<IpAddr>().unwrap(),
            "::1".parse().unwrap()
        ])
    );

    // pieces are not trimmed without the adapter
    let err = deserialize::<As<Vec<u16>, Separated>>(vec![lexical("1, 2")]).unwrap_err();
    assert_eq!(err.message(), "invalid value \" 2\", expected u16");
    let err = deserialize::<As<Vec<u16>, Separated>>(vec![Event::from(true)]).unwrap_err();
    assert_eq!(err.message(), "unexpected bool, expected vec");

    // elements are joined when serializing
    let value = Config {
        hosts: vec!["a".into(), "b".into()],
        path: VecDeque::from(["/bin".to_string()]),
        ports: BTreeSet::from([80, 443]),
        addrs: None,
    };
    check(
        value,
        vec![
            Event::map_start(),
            "hosts".into(),
            "a,b".into(),
            "path".into(),
            "/bin".into(),
            "ports".into(),
            "80,443".into(),
            "addrs".into(),
            ().into(),
            Event::MapEnd,
        ],
    );
    let joined = |value: SerializeRef<'_>| serialize(&value);
    assert_eq!(
        joined(SerializeRef::new(&As::<_, Separated<';'>>::new(vec![
            1.5f64, 2.0
        ]))),
        [Event::from("1.5;2")]
    );
    assert_eq!(
        joined(SerializeRef::new(&As::<HashSet<bool>, Separated>::new(
            HashSet::from([true])
        ))),
        [Event::from("true")]
    );
    assert_eq!(
        joined(SerializeRef::new(&As::<Vec<String>, Separated>::new(
            vec![]
        ))),
        [Event::from("")]
    );

    // values that would not read back are errors
    let fails = |value: SerializeRef<'_>| SerializeDriver::from_ref(value).drive(|_, _| Ok(()));
    let err = fails(SerializeRef::new(&As::<_, Separated>::new(vec![
        "a", "b,c",
    ])))
    .unwrap_err();
    assert_eq!(
        err.message(),
        "cannot join \"b,c\", it contains the separator ','"
    );
    let err = fails(SerializeRef::new(&As::<_, Separated>::new(vec![""]))).unwrap_err();
    assert_eq!(
        err.message(),
        "cannot join a single empty string, it would read back as no elements"
    );
    assert!(fails(SerializeRef::new(&As::<_, Separated>::new(vec!["", ""]))).is_ok());
    let err = fails(SerializeRef::new(&As::<_, Separated>::new(vec![vec![
        1u32,
    ]])))
    .unwrap_err();
    assert_eq!(
        err.message(),
        "cannot join sequence, elements must be strings, numbers, booleans or chars"
    );
    let err = fails(SerializeRef::new(&As::<_, Separated>::new(vec![
        None::<u32>,
    ])))
    .unwrap_err();
    assert_eq!(
        err.message(),
        "cannot join null, elements must be strings, numbers, booleans or chars"
    );
}

#[test]
fn test_separated_borrowed() {
    use deser::adapters::{Separated, TrimWhitespace};

    #[derive(Debug, Deserialize)]
    struct Hosts<'a> {
        #[deser(as = Separated<',', TrimWhitespace>)]
        hosts: Vec<&'a str>,
    }

    let input = String::from("a, b ,c");
    let mut out = None::<Hosts<'_>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::map_start()).unwrap();
        driver.emit("hosts").unwrap();
        driver
            .emit_borrowed(Atom::Lexical(input.as_str().into()))
            .unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    assert_eq!(out.unwrap().hosts, ["a", "b", "c"]);

    // owned text cannot be borrowed
    let mut out = None::<Hosts<'_>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::map_start()).unwrap();
    driver.emit("hosts").unwrap();
    assert!(driver.emit(Atom::Lexical("a,b".into())).is_err());
}

#[test]
fn test_trim_whitespace() {
    use deser::adapters::TrimWhitespace;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Login {
        #[deser(as = TrimWhitespace)]
        username: String,
        // trimmed first, so blank values are `None`
        #[deser(as = TrimWhitespace<Option<_>>)]
        port: Option<u16>,
        #[deser(as = TrimWhitespace, default)]
        tags: Vec<String>,
        #[deser(as = Option<TrimWhitespace>, default)]
        timeout: Option<u32>,
    }

    // like in a query string, blank values are missing values
    let login = |username: Event<'static>, port: Event<'static>| {
        deserialize_lenient::<Login>(vec![
            Event::map_start(),
            "username".into(),
            username,
            "port".into(),
            port,
            Event::MapEnd,
        ])
    };
    let lexical = |text: &'static str| Event::from(Atom::Lexical(text.into()));

    let value = login(Event::from("  jane\t"), lexical(" 8080 ")).unwrap();
    assert_eq!(value.username, "jane");
    assert_eq!(value.port, Some(8080));

    // a string stays a string, only lexical atoms parse as numbers
    let err = login(Event::from("jane"), Event::from(" 8080 ")).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected u16");
    // blank values are empty which is `None` for numbers
    assert_eq!(login(lexical(" "), lexical("  ")).unwrap().port, None);
    assert_eq!(login(lexical(" "), lexical("  ")).unwrap().username, "");
    // with the option outside, only values that are empty before trimming
    // are `None`
    let timeout = |value: Event<'static>| {
        deserialize_lenient::<Login>(vec![
            Event::map_start(),
            "username".into(),
            "x".into(),
            "timeout".into(),
            value,
            Event::MapEnd,
        ])
        .map(|login| login.timeout)
    };
    assert_eq!(timeout(lexical(" 5 ")).unwrap(), Some(5));
    assert_eq!(timeout(lexical("")).unwrap(), None);
    assert_eq!(
        timeout(lexical(" ")).unwrap_err().message(),
        "invalid value \"\", expected u32"
    );
    // other values are passed on
    assert_eq!(
        login(lexical("x"), Event::from(1u64)).unwrap().port,
        Some(1)
    );

    // compound values are passed on (elements are not trimmed)
    let value = deserialize::<Login>(vec![
        Event::map_start(),
        "username".into(),
        "x".into(),
        "tags".into(),
        Event::seq_start(),
        " a ".into(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.tags, [" a "]);
    assert_eq!(value.port, None);

    // serialization is not affected
    assert_eq!(
        serialize(&Login {
            username: " jane ".into(),
            port: None,
            tags: vec![],
            timeout: Some(1),
        }),
        vec![
            Event::map_start(),
            "username".into(),
            " jane ".into(),
            "port".into(),
            ().into(),
            "tags".into(),
            Event::seq_start(),
            Event::SeqEnd,
            "timeout".into(),
            1u64.into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_skip_blank() {
    use deser::ContainerShape;
    use deser::adapters::{Separated, SkipBlank, TrimWhitespace};

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Config {
        #[deser(as = Separated<',', SkipBlank<TrimWhitespace>>, default)]
        hosts: Vec<String>,
        #[deser(as = Vec<SkipBlank>, default)]
        tags: Vec<String>,
        #[deser(as = Vec<SkipBlank<Option<_>>>, default)]
        ports: Vec<Option<u16>>,
        #[deser(as = SkipBlank<Option<_>>)]
        name: Option<String>,
        #[deser(as = SkipBlank, default)]
        level: u32,
    }

    let lexical = |text: &'static str| Event::from(Atom::Lexical(text.into()));
    let config = |shape: ContainerShape, entries: Vec<Event<'static>>| {
        let mut events = vec![Event::MapStart(shape)];
        events.extend(entries);
        events.push(Event::MapEnd);
        deserialize_lenient::<Config>(events)
    };

    // elements of sequences
    let value = config(
        ContainerShape::new(),
        vec![
            "hosts".into(),
            lexical("a, ,b,"),
            "tags".into(),
            Event::seq_start(),
            "x".into(),
            " ".into(),
            "".into(),
            " y ".into(),
            Event::SeqEnd,
            "ports".into(),
            Event::seq_start(),
            lexical("1"),
            lexical(" "),
            ().into(),
            Event::SeqEnd,
        ],
    )
    .unwrap();
    assert_eq!(value.hosts, ["a", "b"]);
    assert_eq!(value.tags, ["x", " y "]);
    // null is not blank
    assert_eq!(value.ports, [Some(1), None]);
    assert_eq!(value.name, None);
    assert_eq!(value.level, 0);

    // repeated keys of multimaps
    let value = config(
        {
            let mut shape = ContainerShape::new();
            shape.set_multimap(true);
            shape
        },
        vec![
            "tags".into(),
            lexical(""),
            "tags".into(),
            lexical("a"),
            "tags".into(),
            lexical("\n "),
            "ports".into(),
            lexical(" "),
        ],
    )
    .unwrap();
    assert_eq!(value.tags, ["a"]);
    assert_eq!(value.ports, []);

    // single values: blank is missing
    let value = config(
        ContainerShape::new(),
        vec!["name".into(), lexical(" "), "level".into(), lexical("")],
    )
    .unwrap();
    assert_eq!((value.name, value.level), (None, 0));
    let value = config(
        ContainerShape::new(),
        vec!["name".into(), "x".into(), "level".into(), lexical("3")],
    )
    .unwrap();
    assert_eq!((value.name.as_deref(), value.level), (Some("x"), 3));
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Required {
        #[deser(as = SkipBlank)]
        name: String,
    }
    let err = deserialize::<Required>(vec![
        Event::map_start(),
        "name".into(),
        " ".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);

    // other values are passed on, also through the sink
    let value = config(
        ContainerShape::new(),
        vec![
            "ports".into(),
            Event::seq_start(),
            Event::seq_start(),
            Event::SeqEnd,
            Event::SeqEnd,
        ],
    )
    .unwrap_err();
    assert_eq!(value.message(), "unexpected sequence, expected u16");

    // serialization is not affected
    assert_eq!(
        serialize(&Config {
            hosts: vec![],
            tags: vec![" ".into()],
            ports: vec![],
            name: Some("".into()),
            level: 1,
        }),
        vec![
            Event::map_start(),
            "hosts".into(),
            "".into(),
            "tags".into(),
            Event::seq_start(),
            " ".into(),
            Event::SeqEnd,
            "ports".into(),
            Event::seq_start(),
            Event::SeqEnd,
            "name".into(),
            "".into(),
            "level".into(),
            1u64.into(),
            Event::MapEnd,
        ]
    );
}
