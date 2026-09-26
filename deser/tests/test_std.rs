use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, LinkedList, VecDeque};
use std::ffi::{CStr, CString};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::num::{NonZero, Saturating, Wrapping};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool, AtomicI8, AtomicI64, AtomicU32, AtomicUsize};

use deser::adapters::{As, DisplayFromStr};
use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::{Describe, SerializeDriver, Variant};
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize};

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

fn serialize(value: &dyn Serialize) -> Result<Vec<Event<'static>>, Error> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(value);
    while let Some((event, _, _)) = driver.next()? {
        events.push(without_len(event.to_static()));
    }
    Ok(events)
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

/// Serializes the value, compares the events and deserializes it again.
fn roundtrip<T: Serialize + DeserializeOwned>(value: &T, expected: Vec<Event<'static>>) -> T {
    let events = serialize(value).unwrap();
    assert_eq!(events, expected);
    deserialize(events).unwrap()
}

fn seq(values: impl IntoIterator<Item = Event<'static>>) -> Vec<Event<'static>> {
    let mut rv = vec![Event::seq_start()];
    rv.extend(values);
    rv.push(Event::SeqEnd);
    rv
}

fn ints(values: &[u64]) -> Vec<Event<'static>> {
    seq(values.iter().map(|&x| Event::Atom(Atom::U64(x))))
}

fn bytes(value: &'static [u8]) -> Vec<Event<'static>> {
    vec![Event::Atom(Atom::Bytes(deser::Bytes::new(value)))]
}

fn string(value: &'static str) -> Vec<Event<'static>> {
    vec![Event::Atom(Atom::Str(value.into()))]
}

fn assert_err<T: DeserializeOwned + Debug>(events: Vec<Event<'_>>, kind: ErrorKind, msg: &str) {
    let err = deserialize::<T>(events).unwrap_err();
    assert_eq!(err.kind(), kind);
    assert!(
        err.to_string().contains(msg),
        "{:?} does not contain {:?}",
        err.to_string(),
        msg
    );
}

#[test]
fn test_unsized_pointers() {
    assert_eq!(&*roundtrip(&Box::<str>::from("hi"), string("hi")), "hi");
    assert_eq!(&*roundtrip(&Arc::<str>::from("hi"), string("hi")), "hi");

    let value: Box<[u32]> = vec![1, 2].into();
    assert_eq!(*roundtrip(&value, ints(&[1, 2])), [1, 2]);
    let value: Arc<[u32]> = vec![1, 2].into();
    assert_eq!(*roundtrip(&value, ints(&[1, 2])), [1, 2]);

    // slices of bytes are bytes
    let value: Box<[u8]> = vec![1, 2].into();
    assert_eq!(*roundtrip(&value, bytes(b"\x01\x02")), [1, 2]);
    let value: Arc<[u8]> = vec![1, 2].into();
    assert_eq!(*roundtrip(&value, bytes(b"\x01\x02")), [1, 2]);
}

#[test]
fn test_pointers() {
    assert_eq!(*roundtrip(&Arc::new(42u32), ints(&[42])[1..2].to_vec()), 42);
    let value = Arc::new(vec![Arc::new(true)]);
    let rv = roundtrip(&value, seq([Event::Atom(Atom::Bool(true))]));
    assert!(*rv[0]);

    #[derive(Serialize, Deserialize)]
    struct Shared {
        name: Arc<str>,
        tags: Arc<[String]>,
    }
    let value: Shared = deserialize(vec![
        Event::map_start(),
        "name".into(),
        "demo".into(),
        "tags".into(),
        Event::seq_start(),
        "a".into(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(&*value.name, "demo");
    assert_eq!(&*value.tags, ["a".to_string()]);
}

#[test]
fn test_cow() {
    let value: Cow<'_, str> = Cow::Borrowed("hi");
    assert!(matches!(roundtrip(&value, string("hi")), Cow::Owned(ref x) if x == "hi"));
    let value: Cow<'_, [u8]> = Cow::Borrowed(b"hi");
    assert_eq!(&*roundtrip(&value, bytes(b"hi")), b"hi");
    let value: Cow<'_, [u32]> = Cow::Owned(vec![1, 2]);
    assert_eq!(&*roundtrip(&value, ints(&[1, 2])), [1, 2]);
    let value: Cow<'_, Path> = Cow::Borrowed(Path::new("/tmp"));
    assert_eq!(&*roundtrip(&value, string("/tmp")), Path::new("/tmp"));

    // strings accept chars
    assert_eq!(
        deserialize::<Cow<'static, str>>(vec![Event::Atom(Atom::Char('x'))]).unwrap(),
        "x"
    );
    assert_eq!(
        deserialize::<String>(vec![Event::Atom(Atom::Char('x'))]).unwrap(),
        "x"
    );
}

#[test]
fn test_phantom_data() {
    roundtrip(&PhantomData::<String>, vec![Event::Atom(Atom::Null)]);

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[deser(skip_serializing_optionals)]
    struct Marked<T> {
        value: u32,
        marker: PhantomData<T>,
    }
    let value = Marked::<String> {
        value: 1,
        marker: PhantomData,
    };
    let events = serialize(&value).unwrap();
    assert_eq!(
        events,
        vec![
            Event::map_start(),
            "value".into(),
            1u64.into(),
            Event::MapEnd
        ]
    );
    assert_eq!(deserialize::<Marked<String>>(events).unwrap(), value);
}

#[test]
fn test_sequences() {
    let value = VecDeque::from([1u32, 2, 3]);
    assert_eq!(roundtrip(&value, ints(&[1, 2, 3])), value);
    let value = LinkedList::from([1u32, 2, 3]);
    assert_eq!(roundtrip(&value, ints(&[1, 2, 3])), value);
    let value = BinaryHeap::from([3u32]);
    assert_eq!(roundtrip(&value, ints(&[3])).into_vec(), [3]);
    let value: BinaryHeap<u32> = deserialize(ints(&[1, 3, 2])).unwrap();
    assert_eq!(value.into_sorted_vec(), [1, 2, 3]);

    // deques of bytes are bytes, also if they wrap around
    let mut value = VecDeque::with_capacity(4);
    value.extend([0u8, 0, 1, 2]);
    value.pop_front();
    value.pop_front();
    value.extend([3u8, 4]);
    assert_eq!(value.as_slices(), (&[1u8, 2][..], &[3u8, 4][..]));
    assert_eq!(roundtrip(&value, bytes(b"\x01\x02\x03\x04")), value);
    let value: BinaryHeap<u8> = deserialize(bytes(b"\x01\x02")).unwrap();
    assert_eq!(value.into_sorted_vec(), [1, 2]);

    assert_err::<VecDeque<u32>>(
        string("x"),
        ErrorKind::Unexpected,
        "unexpected string, expected VecDeque",
    );
}

#[test]
fn test_hash_set_with_hasher() {
    use std::collections::HashSet;
    use std::hash::BuildHasherDefault;

    type Hasher = BuildHasherDefault<std::collections::hash_map::DefaultHasher>;
    let mut value = HashSet::<u32, Hasher>::default();
    value.insert(1);
    let events = serialize(&value).unwrap();
    assert_eq!(
        events,
        vec![
            Event::SeqStart(deser::ContainerShape::new().with_order(deser::Order::Arbitrary)),
            1u64.into(),
            Event::SeqEnd
        ]
    );
    assert_eq!(deserialize::<HashSet<u32, Hasher>>(events).unwrap(), value);
}

#[test]
fn test_non_zero() {
    let value = NonZero::new(42u32).unwrap();
    assert_eq!(roundtrip(&value, vec![42u64.into()]), value);
    let value = NonZero::new(-1i8).unwrap();
    assert_eq!(roundtrip(&value, vec![Event::Atom(Atom::I64(-1))]), value);
    let value = NonZero::new(u128::MAX).unwrap();
    let events = serialize(&value).unwrap();
    assert_eq!(deserialize::<NonZero<u128>>(events).unwrap(), value);

    assert_err::<NonZero<u32>>(
        vec![0u64.into()],
        ErrorKind::OutOfRange,
        "value must be non-zero",
    );
    assert_err::<NonZero<u8>>(vec![256u64.into()], ErrorKind::OutOfRange, "out of range");

    // usable as map keys
    let mut map = std::collections::BTreeMap::new();
    map.insert(NonZero::new(1u16).unwrap(), true);
    let rv: std::collections::BTreeMap<NonZero<u16>, bool> = deserialize(vec![
        Event::map_start(),
        "1".into(),
        true.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(rv, map);
}

#[test]
fn test_wrappers() {
    assert_eq!(roundtrip(&Wrapping(1u32), vec![1u64.into()]), Wrapping(1));
    assert_eq!(
        roundtrip(&Saturating(1u32), vec![1u64.into()]),
        Saturating(1)
    );
    assert_eq!(
        roundtrip(&Reverse("x".to_string()), string("x")),
        Reverse("x".to_string())
    );
    assert_eq!(
        roundtrip(&Reverse(vec![1u32]), ints(&[1])),
        Reverse(vec![1])
    );
}

#[test]
fn test_result() {
    let value: Result<u32, String> = Ok(1);
    let events = vec![Event::map_start(), "Ok".into(), 1u64.into(), Event::MapEnd];
    assert_eq!(roundtrip(&value, events), value);

    let value: Result<u32, String> = Err("bad".into());
    let events = vec![
        Event::map_start(),
        "Err".into(),
        "bad".into(),
        Event::MapEnd,
    ];
    assert_eq!(roundtrip(&value, events), value);

    let value: Result<Vec<u32>, ()> = Ok(vec![1]);
    let events = vec![
        Event::map_start(),
        "Ok".into(),
        Event::seq_start(),
        1u64.into(),
        Event::SeqEnd,
        Event::MapEnd,
    ];
    assert_eq!(roundtrip(&value, events), value);

    assert_err::<Result<u32, u32>>(
        vec![
            Event::map_start(),
            "Nope".into(),
            1u64.into(),
            Event::MapEnd,
        ],
        ErrorKind::Unexpected,
        "unknown variant \"Nope\", expected Ok or Err",
    );
    assert_err::<Result<u32, u32>>(
        vec![
            Event::map_start(),
            "Ok".into(),
            1u64.into(),
            "Err".into(),
            1u64.into(),
            Event::MapEnd,
        ],
        ErrorKind::Unexpected,
        "expected a single entry",
    );
    assert_err::<Result<u32, u32>>(
        vec![Event::map_start(), Event::MapEnd],
        ErrorKind::Unexpected,
        "expected an entry with Ok or Err",
    );
    assert_err::<Result<u32, u32>>(vec![1u64.into()], ErrorKind::Unexpected, "expected Result");
}

#[test]
fn test_net() {
    let value: IpAddr = "127.0.0.1".parse().unwrap();
    assert_eq!(roundtrip(&value, string("127.0.0.1")), value);
    let value: IpAddr = "::1".parse().unwrap();
    assert_eq!(roundtrip(&value, string("::1")), value);
    let value = Ipv4Addr::new(10, 0, 0, 1);
    assert_eq!(roundtrip(&value, string("10.0.0.1")), value);
    let value = Ipv6Addr::LOCALHOST;
    assert_eq!(roundtrip(&value, string("::1")), value);
    let value: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    assert_eq!(roundtrip(&value, string("127.0.0.1:8080")), value);
    let value: SocketAddrV4 = "127.0.0.1:8080".parse().unwrap();
    assert_eq!(roundtrip(&value, string("127.0.0.1:8080")), value);
    let value: SocketAddrV6 = "[::1]:8080".parse().unwrap();
    assert_eq!(roundtrip(&value, string("[::1]:8080")), value);

    assert_err::<IpAddr>(
        string("nope"),
        ErrorKind::Unexpected,
        "invalid IP address: invalid IP address syntax",
    );
    assert_err::<SocketAddr>(
        vec![1u64.into()],
        ErrorKind::Unexpected,
        "unexpected unsigned integer, expected socket address",
    );
}

#[test]
fn test_paths() {
    let value = PathBuf::from("/tmp/x");
    assert_eq!(roundtrip(&value, string("/tmp/x")), value);
    let value: Box<Path> = Path::new("/tmp/x").into();
    assert_eq!(roundtrip(&value, string("/tmp/x")), value);
    assert_eq!(serialize(&Path::new("/tmp")).unwrap(), string("/tmp"));

    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let value = Path::new(OsStr::from_bytes(b"\xff"));
        let err = serialize(&value).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unexpected);
        assert!(err.to_string().contains("invalid UTF-8"));
    }
}

#[test]
fn test_atomics() {
    let value = AtomicBool::new(true);
    assert!(roundtrip(&value, vec![true.into()]).load(atomic::Ordering::Relaxed));
    let value = AtomicI8::new(-2);
    assert_eq!(
        roundtrip(&value, vec![Event::Atom(Atom::I64(-2))]).load(atomic::Ordering::Relaxed),
        -2
    );
    let value = AtomicU32::new(7);
    assert_eq!(
        roundtrip(&value, vec![7u64.into()]).load(atomic::Ordering::Relaxed),
        7
    );
    let value = AtomicI64::new(i64::MIN);
    assert_eq!(
        roundtrip(&value, vec![Event::Atom(Atom::I64(i64::MIN))]).load(atomic::Ordering::Relaxed),
        i64::MIN
    );
    let value = AtomicUsize::new(3);
    assert_eq!(
        roundtrip(&value, vec![3u64.into()]).load(atomic::Ordering::Relaxed),
        3
    );
    assert_err::<AtomicI8>(vec![1000u64.into()], ErrorKind::OutOfRange, "out of range");
}

#[test]
fn test_c_strings() {
    let value = CString::new("hi").unwrap();
    assert_eq!(roundtrip(&value, bytes(b"hi")), value);
    let value: Box<CStr> = CString::new("hi").unwrap().into_boxed_c_str();
    assert_eq!(roundtrip(&value, bytes(b"hi")), value);
    assert_eq!(serialize(&c"hi").unwrap(), bytes(b"hi"));

    // formats without bytes represent them as base64 strings by default
    assert_eq!(
        deserialize::<CString>(string("aGk=")).unwrap(),
        c"hi".to_owned()
    );
    assert_eq!(
        deserialize::<CString>(ints(&[104, 105])).unwrap(),
        c"hi".to_owned()
    );
    assert_err::<CString>(bytes(b"h\0i"), ErrorKind::Unexpected, "nul byte");
}

#[test]
fn test_adapters() {
    let value: As<VecDeque<u32>, VecDeque<DisplayFromStr>> = As::new(VecDeque::from([1, 2]));
    let events = seq(["1".into(), "2".into()]);
    assert_eq!(*roundtrip(&value, events), VecDeque::from([1, 2]));

    let value: As<LinkedList<u32>, LinkedList<DisplayFromStr>> = As::new(LinkedList::from([1]));
    assert_eq!(*roundtrip(&value, seq(["1".into()])), LinkedList::from([1]));

    let value: As<BinaryHeap<u32>, BinaryHeap<DisplayFromStr>> = As::new(BinaryHeap::from([1]));
    assert_eq!(
        roundtrip(&value, seq(["1".into()])).into_inner().into_vec(),
        [1]
    );

    let value: As<Arc<u32>, Arc<DisplayFromStr>> = As::new(Arc::new(1));
    assert_eq!(**roundtrip(&value, string("1")), 1);

    let value: As<Box<[u32]>, Box<[DisplayFromStr]>> = As::new(vec![1].into());
    assert_eq!(**roundtrip(&value, seq(["1".into()])), [1]);

    let value: As<Arc<[u32]>, Arc<[DisplayFromStr]>> = As::new(vec![1].into());
    assert_eq!(**roundtrip(&value, seq(["1".into()])), [1]);

    let value: As<Result<u32, u32>, Result<DisplayFromStr, deser::adapters::Same>> = As::new(Ok(1));
    let events = vec![Event::map_start(), "Ok".into(), "1".into(), Event::MapEnd];
    assert_eq!(*roundtrip(&value, events), Ok(1));
}

#[test]
fn test_describe() {
    #[derive(Default)]
    struct Names(Vec<String>);

    impl Describe for Names {
        fn newtype(&mut self, name: &str) {
            self.0.push(format!("newtype {name}"));
        }

        fn variant(&mut self, variant: &Variant<'_>) {
            self.0
                .push(format!("variant {}::{}", variant.enum_name, variant.name));
        }

        fn some(&mut self) {
            self.0.push("some".into());
        }
    }

    let mut names = Names::default();
    Some(Wrapping(Reverse(1))).describe(&mut names);
    assert_eq!(names.0, ["some", "newtype Wrapping", "newtype Reverse"]);

    let mut names = Names::default();
    Err::<u32, u32>(1).describe(&mut names);
    assert_eq!(names.0, ["variant Result::Err"]);
}
