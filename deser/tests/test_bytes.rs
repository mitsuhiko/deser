use std::borrow::Cow;
use std::collections::BTreeMap;

use deser::adapters::{As, Borrowed, ByteSeq, Encoded, EncodedStr};
use deser::bytes::{Base64Url, BytesEncoding, BytesFormat, Hex};
use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    deserialize_with(events, None)
}

fn deserialize_with<T: DeserializeOwned>(
    events: Vec<Event<'_>>,
    format: Option<BytesFormat>,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        if let Some(format) = format {
            *driver.state_mut().get_mut::<BytesFormat>() = format;
        }
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

/// Serializes a value into events with the requested bytes formats.
fn serialize(value: &dyn Serialize) -> Vec<(Event<'static>, Option<BytesFormat>)> {
    let mut events = Vec::new();
    SerializeDriver::new(value)
        .drive(|event, descriptor, _| {
            events.push((event.to_static(), descriptor.bytes_format()));
            Ok(())
        })
        .unwrap();
    events
}

fn bytes(value: &[u8]) -> Event<'static> {
    Event::Atom(Atom::Bytes(Cow::Owned(value.to_vec())))
}

#[test]
fn test_bytes_from_strings() {
    let value: Vec<u8> = deserialize(vec!["Af8=".into()]).unwrap();
    assert_eq!(value, [1, 255]);
    let value: Vec<u8> = deserialize(vec!["-_8".into()]).unwrap();
    assert_eq!(value, [251, 255]);
    let value: [u8; 2] = deserialize(vec!["Af8".into()]).unwrap();
    assert_eq!(value, [1, 255]);
    let value: Cow<'static, [u8]> = deserialize(vec!["Af8=".into()]).unwrap();
    assert_eq!(&*value, [1, 255]);
    let value: Option<Vec<u8>> = deserialize(vec!["".into()]).unwrap();
    assert_eq!(value, Some(vec![]));

    // sequences still work
    let value: Vec<u8> = deserialize(vec![
        Event::SeqStart,
        1u64.into(),
        255u64.into(),
        Event::SeqEnd,
    ])
    .unwrap();
    assert_eq!(value, [1, 255]);

    let err = deserialize::<Vec<u8>>(vec!["A".into()]).unwrap_err();
    assert_eq!(err.to_string(), "Unexpected: invalid base64 string");
    let err = deserialize::<[u8; 3]>(vec!["Af8=".into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::WrongLength);

    // only bytes accept strings
    assert!(deserialize::<Vec<u16>>(vec!["Af8=".into()]).is_err());
    assert!(deserialize::<[u16; 2]>(vec!["Af8=".into()]).is_err());

    // bytes cannot be borrowed from strings
    let mut out = None::<&[u8]>;
    let mut driver = DeserializeDriver::new(&mut out);
    let err = driver.emit_borrowed("Af8=").unwrap_err();
    assert!(err.to_string().contains("cannot be borrowed"));
}

#[test]
fn test_borrowed_bytes_from_strings() {
    let mut out = None::<As<Cow<[u8]>, Borrowed>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit_borrowed("Af8=").unwrap();
    }
    assert_eq!(&**out.unwrap(), [1, 255]);
}

#[test]
fn test_bytes_format_in_state() {
    let hex = Some(BytesFormat::encoded::<Hex>());
    let value: Vec<u8> = deserialize_with(vec!["01ff".into()], hex).unwrap();
    assert_eq!(value, [1, 255]);
    let value: [u8; 2] = deserialize_with(vec!["01ff".into()], hex).unwrap();
    assert_eq!(value, [1, 255]);
    let value: Cow<'static, [u8]> = deserialize_with(vec!["01ff".into()], hex).unwrap();
    assert_eq!(&*value, [1, 255]);
    let value: Vec<u8> = deserialize_with(vec!["Af8=".into()], Some(BytesFormat::SEQ)).unwrap();
    assert_eq!(value, [1, 255]);

    // adapters are not affected
    let value: As<Vec<u8>, Base64Url> = deserialize_with(vec!["Af8".into()], hex).unwrap();
    assert_eq!(*value, [1, 255]);
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    plain: Vec<u8>,
    #[deser(as = Hex)]
    hex: [u8; 2],
    #[deser(as = Encoded<Hex>)]
    encoded: Vec<u8>,
    #[deser(as = EncodedStr<Hex>)]
    forced: Vec<u8>,
    #[deser(as = ByteSeq)]
    seq: Vec<u8>,
    #[deser(as = Option<Hex>)]
    optional: Option<Vec<u8>>,
    #[deser(as = Vec<Hex>)]
    many: Vec<Vec<u8>>,
    #[deser(as = BTreeMap<Hex, _>)]
    keys: BTreeMap<Vec<u8>, u32>,
    #[deser(as = Hex)]
    cow: Cow<'static, [u8]>,
}

fn blob() -> Blob {
    Blob {
        plain: vec![1],
        hex: [2, 3],
        encoded: vec![4],
        forced: vec![5],
        seq: vec![6],
        optional: Some(vec![7]),
        many: vec![vec![8]],
        keys: [(vec![9], 10)].into_iter().collect(),
        cow: Cow::Borrowed(&[11]),
    }
}

#[test]
fn test_adapters_serialize() {
    let hex = Some(BytesFormat::encoded::<Hex>());
    let seq = Some(BytesFormat::SEQ);
    let events = serialize(&blob());
    let expected = vec![
        (Event::MapStart, None),
        ("plain".into(), None),
        (bytes(&[1]), None),
        ("hex".into(), None),
        (bytes(&[2, 3]), hex),
        ("encoded".into(), None),
        (bytes(&[4]), hex),
        ("forced".into(), None),
        // forced strings are strings for all formats
        ("05".into(), hex),
        ("seq".into(), None),
        (bytes(&[6]), seq),
        ("optional".into(), None),
        (bytes(&[7]), hex),
        ("many".into(), None),
        (Event::SeqStart, None),
        (bytes(&[8]), hex),
        (Event::SeqEnd, None),
        ("keys".into(), None),
        (Event::MapStart, None),
        (bytes(&[9]), hex),
        (10u64.into(), None),
        (Event::MapEnd, None),
        ("cow".into(), None),
        (bytes(&[11]), hex),
        (Event::MapEnd, None),
    ];
    assert_eq!(events, expected);
}

#[test]
fn test_adapters_deserialize() {
    // from native bytes
    let events = serialize(&blob())
        .into_iter()
        .map(|(event, _)| event)
        .collect::<Vec<_>>();
    assert_eq!(deserialize::<Blob>(events).unwrap(), blob());

    // from strings
    let events = vec![
        Event::MapStart,
        "plain".into(),
        "AQ==".into(),
        "hex".into(),
        "0203".into(),
        "encoded".into(),
        "04".into(),
        "forced".into(),
        "05".into(),
        "seq".into(),
        Event::SeqStart,
        6u64.into(),
        Event::SeqEnd,
        "optional".into(),
        "07".into(),
        "many".into(),
        Event::SeqStart,
        "08".into(),
        Event::SeqEnd,
        "keys".into(),
        Event::MapStart,
        "09".into(),
        10u64.into(),
        Event::MapEnd,
        "cow".into(),
        "0B".into(),
        Event::MapEnd,
    ];
    assert_eq!(deserialize::<Blob>(events).unwrap(), blob());

    let err = deserialize::<As<[u8; 2], Hex>>(vec!["zz".into()]).unwrap_err();
    assert_eq!(err.to_string(), "Unexpected: invalid hex string");
    let err = deserialize::<As<[u8; 2], Hex>>(vec!["01".into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::WrongLength);
    let err = deserialize::<As<Vec<u8>, Hex>>(vec![1u64.into()]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected unsigned integer, expected bytes or hex string"
    );
}

/// A custom encoding.
struct Dotted;

impl BytesEncoding for Dotted {
    const NAME: &'static str = "dotted";

    fn encode(bytes: &[u8], out: &mut String) {
        let parts = bytes.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        out.push_str(&parts.join("."));
    }

    fn decode(s: &str) -> Result<Vec<u8>, Error> {
        s.split('.')
            .map(|x| {
                x.parse()
                    .map_err(|_| Error::new(ErrorKind::Unexpected, "invalid byte"))
            })
            .collect()
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Custom {
    #[deser(as = Encoded<Dotted>)]
    hint: Vec<u8>,
    #[deser(as = EncodedStr<Dotted>)]
    forced: Vec<u8>,
}

#[test]
fn test_custom_encoding() {
    let value = Custom {
        hint: vec![1, 2],
        forced: vec![3, 4],
    };
    let events = serialize(&value);
    assert_eq!(events[2].1, Some(BytesFormat::encoded::<Dotted>()));
    assert_eq!(events[2].1.unwrap().name(), "dotted");
    assert_eq!(events[4].0, "3.4".into());

    let value: Custom = deserialize(vec![
        Event::MapStart,
        "hint".into(),
        "1.2".into(),
        "forced".into(),
        "3.4".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.hint, [1, 2]);
    assert_eq!(value.forced, [3, 4]);
}

#[cfg(feature = "bytes-encoding")]
#[test]
fn test_data_encoding_adapters() {
    use deser::bytes::Base32;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Key {
        #[deser(as = EncodedStr<Base32>)]
        a: Vec<u8>,
        #[deser(as = Base32)]
        b: Vec<u8>,
    }

    let value = Key {
        a: b"foo".to_vec(),
        b: b"bar".to_vec(),
    };
    let events = serialize(&value);
    assert_eq!(events[2].0, "MZXW6===".into());
    assert_eq!(events[4].1, Some(BytesFormat::encoded::<Base32>()));
    let value: Key = deserialize(vec![
        Event::MapStart,
        "a".into(),
        "MZXW6===".into(),
        "b".into(),
        "MJQXE===".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value.b, b"bar");
}
