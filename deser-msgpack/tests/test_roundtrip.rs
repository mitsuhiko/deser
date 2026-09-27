//! Round-trip and wire-format tests.
use crate::common;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

use common::{Value, de, hex, ser};
use deser::de::DeserializeOwned;
use deser::{Deserialize, Serialize};
use deser_msgpack::SerializerConfig;

const CANONICAL: SerializerConfig = SerializerConfig::new().canonical(true);

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T) {
    let bytes = deser_msgpack::to_vec(&value).unwrap();
    assert_eq!(deser_msgpack::from_slice::<T>(&bytes).unwrap(), value);
    // the dynamic value writes the same bytes
    let dynamic: Value = deser_msgpack::from_slice(&bytes).unwrap();
    assert_eq!(
        deser_msgpack::to_vec(&dynamic).unwrap(),
        bytes,
        "{:?}",
        value
    );
    // and so does the canonical encoding for values without maps
    if !matches!(dynamic, Value::Map(_)) {
        assert_eq!(CANONICAL.to_vec(&value).unwrap(), bytes);
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum Enum {
    Unit,
    Newtype(u32),
    Tuple(u32, u32),
    Struct { x: u32 },
}

#[test]
fn primitives() {
    roundtrip(false);
    roundtrip(true);
    roundtrip(0u8);
    roundtrip(127u8);
    roundtrip(255u8);
    roundtrip(u16::MAX);
    roundtrip(u32::MAX);
    roundtrip(u64::MAX);
    roundtrip(i8::MIN);
    roundtrip(i16::MIN);
    roundtrip(i32::MIN);
    roundtrip(i64::MIN);
    roundtrip(i64::MAX);
    roundtrip(0.0f32);
    roundtrip(1.625f32);
    roundtrip(0.1f32);
    roundtrip(core::f64::consts::PI);
    roundtrip(f64::MAX);
    roundtrip(f64::MIN_POSITIVE);
    roundtrip(f32::MAX);
    roundtrip('a');
    roundtrip('\u{6c34}');
    roundtrip(String::from("hello, world"));
    roundtrip(());
}

#[test]
fn integer_wire_forms() {
    assert_eq!(ser(&0u8), "00");
    assert_eq!(ser(&127u8), "7f");
    assert_eq!(ser(&128u8), "cc80");
    assert_eq!(ser(&255u8), "ccff");
    assert_eq!(ser(&256u16), "cd0100");
    assert_eq!(ser(&65535u32), "cdffff");
    assert_eq!(ser(&65536u32), "ce00010000");
    assert_eq!(ser(&u32::MAX), "ceffffffff");
    assert_eq!(ser(&(u32::MAX as u64 + 1)), "cf0000000100000000");
    assert_eq!(ser(&7i8), "07");
    assert_eq!(ser(&7i64), "07");
    assert_eq!(ser(&200i64), "ccc8");
    assert_eq!(ser(&-1i8), "ff");
    assert_eq!(ser(&-32i8), "e0");
    assert_eq!(ser(&-33i8), "d0df");
    assert_eq!(ser(&-128i16), "d080");
    assert_eq!(ser(&-129i16), "d1ff7f");
    assert_eq!(ser(&-32768i32), "d18000");
    assert_eq!(ser(&-32769i32), "d2ffff7fff");
    assert_eq!(ser(&(i32::MIN as i64 - 1)), "d3ffffffff7fffffff");
    assert_eq!(ser(&i64::MIN), "d38000000000000000");
    assert_eq!(ser(&2i128), "02");
    assert_eq!(ser(&-2i128), "fe");
}

#[test]
fn big_integers() {
    roundtrip(u64::MAX as u128);
    roundtrip(i64::MIN as i128);

    // A 128 bit type accepts all integers.
    assert_eq!(de::<u128>("cfffffffffffffffff").unwrap(), u64::MAX as u128);
    assert_eq!(de::<i128>("d38000000000000000").unwrap(), i64::MIN as i128);
}

#[test]
fn widening_and_narrowing() {
    // Integer width is not a wire property: anything fits anywhere as long
    // as the value is in range.
    let one = deser_msgpack::to_vec(&1u8).unwrap();
    assert_eq!(deser_msgpack::from_slice::<u64>(&one).unwrap(), 1);
    assert_eq!(deser_msgpack::from_slice::<i8>(&one).unwrap(), 1);
    assert_eq!(deser_msgpack::from_slice::<u128>(&one).unwrap(), 1);

    let big = deser_msgpack::to_vec(&300u64).unwrap();
    assert!(deser_msgpack::from_slice::<u8>(&big).is_err());

    let neg = deser_msgpack::to_vec(&-1i8).unwrap();
    assert!(deser_msgpack::from_slice::<u64>(&neg).is_err());

    // Floats decode at any width.
    let f = deser_msgpack::to_vec(&1.5f32).unwrap();
    assert_eq!(deser_msgpack::from_slice::<f64>(&f).unwrap(), 1.5);
    assert_eq!(deser_msgpack::from_slice::<f32>(&f).unwrap(), 1.5);
    let f = deser_msgpack::to_vec(&1.5f64).unwrap();
    assert_eq!(deser_msgpack::from_slice::<f32>(&f).unwrap(), 1.5);
}

#[test]
fn options() {
    roundtrip(Option::<u32>::None);
    roundtrip(Some(42u32));
    assert_eq!(ser(&Option::<u32>::None), "c0");

    // Like in most formats, Some(None) collapses to nil on the wire (and
    // deser fills the inner option).
    let bytes = deser_msgpack::to_vec(&Some(Option::<String>::None)).unwrap();
    assert_eq!(common::to_hex(&bytes), "c0");
    assert_eq!(
        deser_msgpack::from_slice::<Option<Option<String>>>(&bytes).unwrap(),
        Some(None)
    );
}

#[test]
fn sequences_and_maps() {
    roundtrip(vec![1u32, 2, 3]);
    roundtrip((1u8, String::from("x"), 1.5f64));
    roundtrip(Vec::<u32>::new());
    roundtrip(vec![vec![1u32], vec![], vec![2, 3]]);
    roundtrip([1u16, 2, 3]);

    let mut map = BTreeMap::new();
    map.insert(String::from("a"), 1u32);
    map.insert(String::from("b"), 2u32);
    roundtrip(map);

    // Map keys can be anything.
    let mut intmap = BTreeMap::new();
    intmap.insert(-1i64, vec![1u32]);
    intmap.insert(1i64, vec![]);
    roundtrip(intmap.clone());
    assert_eq!(ser(&intmap), "82ff9101 0190".replace(' ', ""));

    let mut boolmap = BTreeMap::new();
    boolmap.insert(false, 1u32);
    boolmap.insert(true, 2u32);
    roundtrip(boolmap);

    let mut vecmap = BTreeMap::new();
    vecmap.insert(vec![1u32, 2], "x".to_string());
    roundtrip(vecmap);
}

#[test]
fn binary_data() {
    // Vec<u8> is binary data.
    let bytes = vec![1u8, 2, 3, 4];
    let encoded = deser_msgpack::to_vec(&bytes).unwrap();
    assert_eq!(common::to_hex(&encoded), "c40401020304");
    let back: Vec<u8> = deser_msgpack::from_slice(&encoded).unwrap();
    assert_eq!(back, bytes);
    roundtrip(bytes);
    roundtrip(Vec::<u8>::new());

    // An array of ints also decodes as Vec<u8>.
    let back: Vec<u8> = de("9401020304").unwrap();
    assert_eq!(back, vec![1, 2, 3, 4]);
    let back: [u8; 4] = de("c40401020304").unwrap();
    assert_eq!(back, [1, 2, 3, 4]);

    // Bodies larger than any internal chunk size.
    let big = vec![0xabu8; if cfg!(miri) { 300 } else { 70_000 }];
    roundtrip(big);
    let text = "雨".repeat(if cfg!(miri) { 100 } else { 20_000 });
    roundtrip(text);
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Plain {
    name: String,
    size: u64,
    tags: Vec<String>,
    ratio: Option<f64>,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Newtype(u32);

#[test]
fn structs() {
    roundtrip(Plain {
        name: "x".into(),
        size: 42,
        tags: vec!["a".into()],
        ratio: None,
    });
    roundtrip(Newtype(99));

    // A struct is a map keyed by field names.
    assert_eq!(
        ser(&Plain {
            name: "x".into(),
            size: 42,
            tags: vec![],
            ratio: Some(0.5),
        }),
        "84 a46e616d65 a178 a473697a65 2a a474616773 90 a5726174696f cb3fe0000000000000"
            .replace(' ', "")
    );
    // a newtype is transparent
    assert_eq!(ser(&Newtype(7)), "07");
}

#[test]
fn enums() {
    roundtrip(Enum::Unit);
    roundtrip(Enum::Newtype(42));
    roundtrip(Enum::Tuple(1, 2));
    roundtrip(Enum::Struct { x: 7 });
    roundtrip(vec![Enum::Unit, Enum::Newtype(0)]);

    // A unit variant is a bare string, anything else is a single-entry map
    // keyed by the variant name.
    assert_eq!(ser(&Enum::Unit), "a4556e6974"); // "Unit"
    assert_eq!(ser(&Enum::Newtype(42)), "81a74e6577747970652a"); // {"Newtype": 42}
    assert_eq!(ser(&Enum::Tuple(1, 2)), "81a55475706c65920102"); // {"Tuple": [1, 2]}
    assert_eq!(ser(&Enum::Struct { x: 7 }), "81a653747275637481a17807"); // {"Struct": {"x": 7}}

    // Long headers work too.
    assert_eq!(
        de::<Enum>("de0001d9074e6577747970652a").unwrap(),
        Enum::Newtype(42)
    );
    assert_eq!(
        de::<Enum>("df00000001a55475706c65dd000000020102").unwrap(),
        Enum::Tuple(1, 2)
    );

    // The bare string form only encodes unit variants.
    assert!(de::<Enum>("a74e657774797065").is_err());
    assert!(de::<Enum>("a55475706c65").is_err());
    // A multi-entry map does not encode an enum.
    assert!(de::<Enum>("82a74e6577747970652aa155c0").is_err());
    assert!(de::<Enum>("80").is_err());
}

#[test]
fn internally_tagged_enums_buffer_non_string_keys() {
    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    #[deser(tag = "t")]
    enum Message {
        Stats {
            counts: HashMap<u32, u32>,
            total: u64,
            at: deser::ext::Timestamp,
        },
    }

    // {"counts": {1: 2}, "total": 2^64 - 1, "at": timestamp, "t": "Stats"}:
    // the tag comes last so the content is buffered and replayed
    let bytes = hex(
        "84 a6636f756e7473 810102 a5746f74616c cfffffffffffffffff a26174 d6ff00000001 a174 a55374617473",
    );
    let msg: Message = deser_msgpack::from_slice(&bytes).unwrap();
    assert_eq!(
        msg,
        Message::Stats {
            counts: [(1, 2)].into_iter().collect(),
            total: u64::MAX,
            at: deser::ext::Timestamp {
                seconds: 1,
                nanosecond: 0
            },
        }
    );
}

#[test]
fn skipped_and_unknown_fields() {
    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct Small {
        name: String,
    }

    // Extra fields in the input are ignored, whatever their shape.
    let full = deser_msgpack::to_vec(&Plain {
        name: "x".into(),
        size: 42,
        tags: vec!["a".into(), "b".into()],
        ratio: Some(0.5),
    })
    .unwrap();
    let small: Small = deser_msgpack::from_slice(&full).unwrap();
    assert_eq!(small, Small { name: "x".into() });

    // Unknown fields with extensions and nested values are ignored too.
    let small: Small = de("83 a178 92 01 81 c3 c0 a46e616d65 a178 a179 d40701 ").unwrap();
    assert_eq!(small, Small { name: "x".into() });
}

#[test]
fn stream_of_items() {
    let mut buffer = Vec::new();
    buffer.extend(deser_msgpack::to_vec(&1u32).unwrap());
    buffer.extend(deser_msgpack::to_vec(&"two").unwrap());
    buffer.extend(deser_msgpack::to_vec(&vec![3u32]).unwrap());

    let mut de = deser_msgpack::Deserializer::from_slice(&buffer);
    let mut iter = de.iter::<Value>();
    assert_eq!(iter.next().unwrap().unwrap(), Value::from(1u64));
    assert_eq!(iter.next().unwrap().unwrap(), Value::from("two"));
    assert_eq!(iter.next().unwrap().unwrap(), array![3u64]);
    assert!(iter.next().is_none());

    // from_slice rejects trailing items.
    assert!(deser_msgpack::from_slice::<u32>(&buffer).is_err());

    // A truncated trailing item is an error, not a silent end.
    let mut truncated = deser_msgpack::to_vec(&1u32).unwrap();
    truncated.extend_from_slice(&[0xcd, 0x01]); // u16 missing a byte

    let mut de = deser_msgpack::Deserializer::from_slice(&truncated);
    let mut iter = de.iter::<u32>();
    assert_eq!(iter.next().unwrap().unwrap(), 1);
    assert!(iter.next().unwrap().is_err());
    assert!(iter.next().is_none());

    // Empty input is an empty stream.
    let mut de = deser_msgpack::Deserializer::from_slice(&[]);
    assert!(de.iter::<u32>().next().is_none());

    // The iterator stops after the first error.
    let mut de = deser_msgpack::Deserializer::from_slice(&[0x01, 0xc1, 0x02]);
    let mut iter = de.iter::<u32>();
    assert_eq!(iter.next().unwrap().unwrap(), 1);
    let err = iter.next().unwrap().unwrap_err();
    assert!(err.to_string().contains("offset 1"), "{}", err);
    assert!(iter.next().is_none());
}

#[test]
fn deserializer_offset() {
    let bytes = deser_msgpack::to_vec(&(1u64, "ab")).unwrap();
    let mut de = deser_msgpack::Deserializer::from_slice(&bytes);
    assert_eq!(de.offset(), 0);
    let _: (u64, String) = de.deserialize().unwrap();
    assert_eq!(de.offset(), bytes.len());
    assert!(de.is_end());
    de.end().unwrap();
}
