//! The collections and byte buffers of other crates (with the features of
//! the same name).
#![allow(dead_code)]

use std::borrow::Cow;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Atom, ContainerShape, Deserialize, Error, ErrorKind, Event, Order, Serialize};

/// Removes the length from container starts, the tests are not about it.
fn without_len(event: Event<'static>) -> Event<'static> {
    match event {
        Event::MapStart(shape) => Event::MapStart(ContainerShape::new().with_order(shape.order())),
        Event::SeqStart(shape) => Event::SeqStart(ContainerShape::new().with_order(shape.order())),
        event => event,
    }
}

fn serialize(value: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(without_len(event.to_static()));
    }
    events
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

fn borrowed<'de, T: Deserialize<'de>>(events: Vec<Event<'de>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit_borrowed(event)?;
        }
    }
    Ok(out.unwrap())
}

fn update<T: DeserializeOwned>(value: &mut T, events: Vec<Event<'_>>) -> Result<(), Error> {
    let mut driver = DeserializeDriver::update(value);
    for event in events {
        driver.emit(event)?;
    }
    Ok(())
}

/// Serializes the value, compares the events and deserializes it again.
fn roundtrip<T: Serialize + DeserializeOwned>(value: &T, expected: Vec<Event<'static>>) -> T {
    let events = serialize(value);
    assert_eq!(events, expected);
    deserialize(events).unwrap()
}

fn seq(order: Order, values: impl IntoIterator<Item = Event<'static>>) -> Vec<Event<'static>> {
    let mut rv = vec![Event::SeqStart(ContainerShape::new().with_order(order))];
    rv.extend(values);
    rv.push(Event::SeqEnd);
    rv
}

fn map(order: Order, entries: &[(&str, Event<'static>)]) -> Vec<Event<'static>> {
    let mut rv = vec![Event::MapStart(ContainerShape::new().with_order(order))];
    for (key, value) in entries {
        rv.push(Event::from(key.to_string()));
        rv.push(value.clone());
    }
    rv.push(Event::MapEnd);
    rv
}

fn bytes(value: &[u8]) -> Event<'static> {
    Event::Atom(Atom::Bytes(deser::Bytes::new(Cow::Owned(value.to_vec()))))
}

#[cfg(feature = "indexmap")]
mod with_indexmap {
    use deser::adapters::{DisplayFromStr, MapSkipError};
    use indexmap::{IndexMap, IndexSet};

    use super::*;

    #[test]
    fn test_map() {
        let value = IndexMap::from([("b".to_string(), 1u32), ("a".to_string(), 2)]);
        let rv = roundtrip(
            &value,
            map(Order::Natural, &[("b", 1u64.into()), ("a", 2u64.into())]),
        );
        // the order is kept
        assert_eq!(rv.keys().collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(rv, value);

        let err = deserialize::<IndexMap<String, u32>>(vec![Event::seq_start(), Event::SeqEnd])
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unexpected: unexpected sequence, expected IndexMap"
        );

        let err = deserialize::<IndexMap<String, u32>>(map(
            Order::Natural,
            &[("a", 1u64.into()), ("a", 2u64.into())],
        ))
        .unwrap_err();
        assert_eq!(err.message(), "duplicate key in map");
    }

    #[test]
    fn test_set() {
        let value = IndexSet::from([3u32, 1, 2]);
        let rv = roundtrip(
            &value,
            seq(Order::Natural, [3u64.into(), 1u64.into(), 2u64.into()]),
        );
        assert_eq!(rv.iter().copied().collect::<Vec<_>>(), [3, 1, 2]);
    }

    #[test]
    fn test_update() {
        let mut value = IndexMap::from([("b".to_string(), 1u32), ("a".to_string(), 1)]);
        update(
            &mut value,
            map(Order::Natural, &[("c", 2u64.into()), ("b", 2u64.into())]),
        )
        .unwrap();
        // existing keys keep their position
        assert_eq!(
            value.into_iter().collect::<Vec<_>>(),
            [
                ("b".to_string(), 2),
                ("a".to_string(), 1),
                ("c".to_string(), 2)
            ]
        );
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Hosts {
        #[deser(as = IndexMap<_, DisplayFromStr>)]
        addrs: IndexMap<String, std::net::Ipv4Addr>,
        #[deser(as = IndexSet<DisplayFromStr>)]
        ports: IndexSet<u16>,
        #[deser(as = MapSkipError)]
        weights: IndexMap<String, u32>,
    }

    #[test]
    fn test_adapters() {
        let mut events = vec![Event::map_start(), "addrs".into()];
        events.extend(map(Order::Natural, &[("a", "127.0.0.1".into())]));
        events.push("ports".into());
        events.extend(seq(Order::Natural, ["80".into(), "443".into()]));
        events.push("weights".into());
        events.extend(map(
            Order::Natural,
            &[("a", 1u64.into()), ("b", "x".into()), ("c", 3u64.into())],
        ));
        events.push(Event::MapEnd);
        let value: Hosts = deserialize(events).unwrap();
        assert_eq!(
            value,
            Hosts {
                addrs: IndexMap::from([("a".to_string(), [127, 0, 0, 1].into())]),
                ports: IndexSet::from([80, 443]),
                weights: IndexMap::from([("a".to_string(), 1), ("c".to_string(), 3)]),
            }
        );

        let events = serialize(&value);
        assert_eq!(events[4], Event::from("127.0.0.1"));
        assert_eq!(&events[8..10], [Event::from("80"), Event::from("443")]);
    }
}

#[cfg(feature = "hashbrown")]
mod with_hashbrown {
    use deser::adapters::DisplayFromStr;
    use hashbrown::{HashMap, HashSet};

    use super::*;

    #[test]
    fn test_map() {
        let value = HashMap::from([("a".to_string(), 1u32)]);
        let rv = roundtrip(&value, map(Order::Arbitrary, &[("a", 1u64.into())]));
        assert_eq!(rv, value);

        let err = deserialize::<HashMap<String, u32>>(vec![1u64.into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unexpected: unexpected unsigned integer, expected HashMap"
        );
    }

    #[test]
    fn test_set() {
        let value = HashSet::from([1u32]);
        let rv = roundtrip(&value, seq(Order::Arbitrary, [1u64.into()]));
        assert_eq!(rv, value);
    }

    #[test]
    fn test_update() {
        let mut value = HashMap::from([("a".to_string(), 1u32), ("z".to_string(), 1)]);
        update(
            &mut value,
            map(
                Order::Arbitrary,
                &[("a", 2u64.into()), ("b", 2u64.into()), ("c", 2u64.into())],
            ),
        )
        .unwrap();
        assert_eq!(
            value,
            HashMap::from([
                ("a".to_string(), 2),
                ("b".to_string(), 2),
                ("c".to_string(), 2),
                ("z".to_string(), 1),
            ])
        );
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Ports {
        #[deser(as = HashMap<DisplayFromStr, _>)]
        names: HashMap<u16, String>,
        #[deser(as = HashSet<DisplayFromStr>)]
        open: HashSet<u16>,
    }

    #[test]
    fn test_adapters() {
        let value = Ports {
            names: HashMap::from([(80, "http".to_string())]),
            open: HashSet::from([443]),
        };
        let events = serialize(&value);
        assert_eq!(events[3], Event::from("80"));
        assert_eq!(events[8], Event::from("443"));
        assert_eq!(deserialize::<Ports>(events).unwrap(), value);
    }
}

#[cfg(feature = "smallvec")]
mod with_smallvec {
    use deser::adapters::{Base64Url, DisplayFromStr};
    use smallvec::{SmallVec, smallvec};

    use super::*;

    #[test]
    fn test_seq() {
        let value: SmallVec<[u32; 2]> = smallvec![1, 2, 3];
        let rv = roundtrip(
            &value,
            seq(Order::Natural, [1u64.into(), 2u64.into(), 3u64.into()]),
        );
        assert_eq!(rv, value);

        let value: SmallVec<[String; 2]> = smallvec!["a".to_string()];
        let rv = roundtrip(&value, seq(Order::Natural, ["a".into()]));
        assert_eq!(rv, value);

        let err = deserialize::<SmallVec<[u32; 2]>>(vec![true.into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unexpected: unexpected bool, expected SmallVec"
        );
    }

    #[test]
    fn test_bytes() {
        let value: SmallVec<[u8; 4]> = smallvec![1, 255];
        let rv = roundtrip(&value, vec![bytes(&[1, 255])]);
        assert_eq!(rv, value);
        let rv: SmallVec<[u8; 4]> = deserialize(vec!["Af8=".into()]).unwrap();
        assert_eq!(&rv[..], [1, 255]);
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Item {
        #[deser(as = SmallVec<[DisplayFromStr; 2]>)]
        ports: SmallVec<[u16; 2]>,
        #[deser(as = Base64Url)]
        id: SmallVec<[u8; 4]>,
    }

    #[test]
    fn test_adapters() {
        let value = Item {
            ports: smallvec![80, 443],
            id: smallvec![251, 255],
        };
        let events = serialize(&value);
        assert_eq!(
            events,
            [
                Event::map_start(),
                "ports".into(),
                Event::seq_start(),
                "80".into(),
                "443".into(),
                Event::SeqEnd,
                "id".into(),
                "-_8=".into(),
                Event::MapEnd,
            ]
        );
        assert_eq!(deserialize::<Item>(events).unwrap(), value);
    }
}

#[cfg(feature = "arrayvec")]
mod with_arrayvec {
    use arrayvec::{ArrayString, ArrayVec};
    use deser::adapters::{BytesFallback, DisplayFromStr, IntSeq};

    use super::*;

    #[test]
    fn test_seq() {
        let value = ArrayVec::from([1u32, 2]);
        let rv = roundtrip(&value, seq(Order::Natural, [1u64.into(), 2u64.into()]));
        assert_eq!(rv, value);

        let rv: ArrayVec<u32, 4> = deserialize(seq(Order::Natural, [1u64.into()])).unwrap();
        assert_eq!(&rv[..], [1]);

        let err = deserialize::<ArrayVec<u32, 1>>(seq(Order::Natural, [1u64.into(), 2u64.into()]))
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WrongLength);
        assert_eq!(err.message(), "ArrayVec exceeds the capacity of 1");
    }

    #[test]
    fn test_bytes() {
        let value = ArrayVec::from([1u8, 255]);
        let rv = roundtrip(&value, vec![bytes(&[1, 255])]);
        assert_eq!(rv, value);

        let err = deserialize::<ArrayVec<u8, 1>>(vec!["Af8=".into()]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WrongLength);
    }

    #[test]
    fn test_string() {
        let value = ArrayString::<8>::from("hello").unwrap();
        let rv = roundtrip(&value, vec!["hello".into()]);
        assert_eq!(rv, value);

        let rv: ArrayString<1> = deserialize(vec!['x'.into()]).unwrap();
        assert_eq!(rv.as_str(), "x");

        let err = deserialize::<ArrayString<4>>(vec!["hello".into()]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WrongLength);
        assert_eq!(err.message(), "string exceeds the capacity of 4");
        let err = deserialize::<ArrayString<4>>(vec![1u64.into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unexpected: unexpected unsigned integer, expected string"
        );
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Item {
        #[deser(as = ArrayVec<DisplayFromStr, 2>)]
        ports: ArrayVec<u16, 2>,
        #[deser(as = BytesFallback<IntSeq>)]
        id: ArrayVec<u8, 4>,
        name: ArrayString<8>,
    }

    #[test]
    fn test_adapters() {
        let value = Item {
            ports: ArrayVec::from([80, 443]),
            id: ArrayVec::from([1, 2, 3, 4]),
            name: ArrayString::from("x").unwrap(),
        };
        let events = serialize(&value);
        assert_eq!(&events[3..5], [Event::from("80"), Event::from("443")]);
        assert_eq!(deserialize::<Item>(events).unwrap(), value);
    }
}

#[cfg(feature = "bytes")]
mod with_bytes {
    use ::bytes::{Bytes, BytesMut};
    use deser::adapters::{Base64Url, BytesFallback, IntSeq};

    use super::*;

    #[test]
    fn test_bytes() {
        let value = Bytes::from_static(b"\x01\xff");
        let rv = roundtrip(&value, vec![bytes(&[1, 255])]);
        assert_eq!(rv, value);

        let value = BytesMut::from(&b"\x01\xff"[..]);
        let rv = roundtrip(&value, vec![bytes(&[1, 255])]);
        assert_eq!(rv, value);

        // like `Vec<u8>`: strings are decoded and sequences accepted
        let rv: Bytes = deserialize(vec!["Af8=".into()]).unwrap();
        assert_eq!(rv, &b"\x01\xff"[..]);
        let rv: BytesMut = deserialize(seq(Order::Natural, [1u64.into(), 255u64.into()])).unwrap();
        assert_eq!(rv, &b"\x01\xff"[..]);

        let err = deserialize::<Bytes>(vec![true.into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unexpected: unexpected bool, expected bytes"
        );
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Blob {
        data: Bytes,
        #[deser(as = Base64Url)]
        token: Bytes,
        #[deser(as = Option<BytesFallback<IntSeq>>)]
        legacy: Option<BytesMut>,
    }

    #[test]
    fn test_adapters() {
        let value = Blob {
            data: Bytes::from_static(b"\x01"),
            token: Bytes::from_static(b"\xfb\xff"),
            legacy: Some(BytesMut::from(&b"\x02"[..])),
        };
        let events = serialize(&value);
        assert_eq!(events[2], bytes(&[1]));
        assert_eq!(events[4], Event::from("-_8="));
        assert_eq!(deserialize::<Blob>(events).unwrap(), value);
    }
}

#[cfg(feature = "bstr")]
mod with_bstr {
    use ::bstr::{BStr, BString};
    use deser::adapters::{Base64Url, BytesFormat};

    use super::*;

    #[test]
    fn test_serialize() {
        // valid UTF-8 is a string
        let value = BString::from("hi");
        let rv = roundtrip(&value, vec!["hi".into()]);
        assert_eq!(rv, value);
        assert_eq!(serialize(&BStr::new("hi")), [Event::from("hi")]);

        // everything else bytes which are sequences in formats without bytes
        let value = BString::from(&b"hi\xff"[..]);
        let expected = Event::Atom(Atom::Bytes(
            deser::Bytes::new(&b"hi\xff"[..]).with_fallback(&BytesFormat::SEQ),
        ));
        let rv = roundtrip(&value, vec![expected]);
        assert_eq!(rv, value);
        let rv: Box<BStr> = deserialize(vec![bytes(b"hi\xff")]).unwrap();
        assert_eq!(&*rv, BStr::new(b"hi\xff"));
    }

    #[test]
    fn test_deserialize() {
        // strings are text, not encoded bytes
        let rv: BString = deserialize(vec!["aGk=".into()]).unwrap();
        assert_eq!(rv, "aGk=");
        let rv: BString = deserialize(vec!['x'.into()]).unwrap();
        assert_eq!(rv, "x");
        let rv: BString = deserialize(seq(Order::Natural, [104u64.into(), 255u64.into()])).unwrap();
        assert_eq!(rv, &b"h\xff"[..]);
        let rv: Cow<'static, BStr> = deserialize(vec![bytes(b"hi")]).unwrap();
        assert_eq!(rv, BStr::new("hi"));
        let rv: Vec<BString> = deserialize(seq(
            Order::Natural,
            [
                "a".into(),
                bytes(b"b"),
                Event::seq_start(),
                99u64.into(),
                Event::SeqEnd,
            ],
        ))
        .unwrap();
        assert_eq!(rv, ["a", "b", "c"]);

        let err = deserialize::<BString>(vec![true.into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unexpected: unexpected bool, expected byte string"
        );
        let err = deserialize::<BString>(seq(Order::Natural, [256u64.into()])).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::OutOfRange);
    }

    #[test]
    fn test_borrowed() {
        let data = b"hi".to_vec();
        let value: &BStr = borrowed(vec![data.as_slice().into()]).unwrap();
        assert_eq!(value, "hi");
        assert!(std::ptr::eq(value.as_ptr(), data.as_ptr()));

        let text = String::from("hi");
        let value: &BStr = borrowed(vec![text.as_str().into()]).unwrap();
        assert!(std::ptr::eq(value.as_ptr(), text.as_ptr()));

        let err = borrowed::<&BStr>(vec![bytes(b"hi")]).unwrap_err();
        assert!(err.message().starts_with("unexpected owned byte string"));
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Blob {
        // the adapters encode the raw bytes
        #[deser(as = Base64Url)]
        token: BString,
    }

    #[test]
    fn test_adapters() {
        let value = Blob {
            token: BString::from("hi"),
        };
        let events = serialize(&value);
        assert_eq!(events[2], Event::from("aGk="));
        assert_eq!(deserialize::<Blob>(events).unwrap(), value);
    }
}
