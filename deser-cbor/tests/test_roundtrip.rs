//! Round-trip and wire-format tests.
//!
//! Adapted from the cbor2 test suite.
mod common;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

use common::{de, hex, ser, Value};
use deser::{Deserialize, Serialize};

fn roundtrip<T: Serialize + Deserialize + PartialEq + Debug>(value: T) {
    let bytes = deser_cbor::to_vec(&value).unwrap();
    assert_eq!(deser_cbor::from_slice::<T>(&bytes).unwrap(), value);
    // the dynamic value writes the same bytes
    let dynamic: Value = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(deser_cbor::to_vec(&dynamic).unwrap(), bytes, "{:?}", value);
    // and so does the canonical encoding for values without maps
    if !matches!(dynamic, Value::Map(_)) {
        assert_eq!(deser_cbor::to_canonical_vec(&value).unwrap(), bytes);
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
    roundtrip(23u8);
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
    assert_eq!(ser(&23u8), "17");
    assert_eq!(ser(&24u8), "1818");
    assert_eq!(ser(&255u8), "18ff");
    assert_eq!(ser(&256u16), "190100");
    assert_eq!(ser(&65535u32), "19ffff");
    assert_eq!(ser(&65536u32), "1a00010000");
    assert_eq!(ser(&u32::MAX), "1affffffff");
    assert_eq!(ser(&(u32::MAX as u64 + 1)), "1b0000000100000000");
    assert_eq!(ser(&7i8), "07");
    assert_eq!(ser(&7i64), "07");
    assert_eq!(ser(&-1i8), "20");
    assert_eq!(ser(&-24i8), "37");
    assert_eq!(ser(&-25i8), "3818");
    assert_eq!(ser(&i64::MIN), "3b7fffffffffffffff");
    assert_eq!(ser(&2i128), "02");
    assert_eq!(ser(&-2i128), "21");
}

#[test]
fn float_wire_forms() {
    // floats use the shortest form that preserves the value, independent of
    // the type.
    assert_eq!(ser(&1.5f64), "f93e00");
    assert_eq!(ser(&1.5f32), "f93e00");
    assert_eq!(ser(&100000.0f64), "fa47c35000");
    assert_eq!(ser(&0.1f32), "fa3dcccccd");
    assert_eq!(ser(&0.1f64), "fb3fb999999999999a");
    assert_eq!(ser(&-0.0f64), "f98000");
    assert_eq!(ser(&f32::INFINITY), "f97c00");
    // the smallest subnormal half
    assert_eq!(ser(&5.960464477539063e-8), "f90001");
    assert_eq!(ser(&f64::from_bits(1)), "fb0000000000000001");
}

#[test]
fn big_integers() {
    roundtrip(u128::MAX);
    roundtrip(i128::MAX);
    roundtrip(i128::MIN);
    roundtrip(u64::MAX as u128 + 1);
    roundtrip(-(u64::MAX as i128) - 1);
    roundtrip(-(u64::MAX as i128) - 2);
    roundtrip(i64::MIN as i128 - 1);

    // Values within the u64/i64 range encode as plain integers...
    assert_eq!(ser(&1u128), "01");
    assert_eq!(ser(&-1i128), "20");
    assert_eq!(ser(&(u64::MAX as i128)), "1bffffffffffffffff");
    assert_eq!(ser(&(-(u64::MAX as i128) - 1)), "3bffffffffffffffff");
    // ...and beyond it as bignums.
    assert_eq!(ser(&(u64::MAX as u128 + 1)), "c249010000000000000000");
    assert_eq!(ser(&(u64::MAX as i128 + 1)), "c249010000000000000000");
    assert_eq!(ser(&(-(u64::MAX as i128) - 2)), "c349010000000000000000");
    assert_eq!(ser(&u128::MAX), "c250ffffffffffffffffffffffffffffffff");
    assert_eq!(ser(&i128::MIN), "c3507fffffffffffffffffffffffffffffff");

    // A bignum that fits in a primitive type decodes into it.
    assert_eq!(de::<u8>("c24101").unwrap(), 1);
    assert_eq!(de::<i8>("c34101").unwrap(), -2);
    assert_eq!(de::<u64>("c248ffffffffffffffff").unwrap(), u64::MAX);
    assert_eq!(de::<i64>("c3487fffffffffffffff").unwrap(), i64::MIN);
    // Negative integers below i64::MIN decode into i128.
    assert_eq!(
        de::<i128>("3bffffffffffffffff").unwrap(),
        -(u64::MAX as i128) - 1
    );
    assert!(de::<i64>("3bffffffffffffffff").is_err());
    // Segmented bignum payloads.
    assert_eq!(de::<u32>("c25f41014102ff").unwrap(), 0x0102);
    // An empty payload is zero.
    assert_eq!(de::<u32>("c240").unwrap(), 0);
    assert_eq!(de::<i32>("c340").unwrap(), -1);
}

#[test]
fn widening_and_narrowing() {
    // Integer width is not a wire property: anything fits anywhere as long
    // as the value is in range.
    let one = deser_cbor::to_vec(&1u8).unwrap();
    assert_eq!(deser_cbor::from_slice::<u64>(&one).unwrap(), 1);
    assert_eq!(deser_cbor::from_slice::<i8>(&one).unwrap(), 1);
    assert_eq!(deser_cbor::from_slice::<u128>(&one).unwrap(), 1);

    let big = deser_cbor::to_vec(&300u64).unwrap();
    assert!(deser_cbor::from_slice::<u8>(&big).is_err());

    let neg = deser_cbor::to_vec(&-1i8).unwrap();
    assert!(deser_cbor::from_slice::<u64>(&neg).is_err());

    // Floats decode at any width (f32 -> f64).
    let f = deser_cbor::to_vec(&1.5f32).unwrap();
    assert_eq!(deser_cbor::from_slice::<f64>(&f).unwrap(), 1.5);
    assert_eq!(deser_cbor::from_slice::<f32>(&f).unwrap(), 1.5);
}

#[test]
fn options() {
    roundtrip(Option::<u32>::None);
    roundtrip(Some(42u32));
    assert_eq!(ser(&Option::<u32>::None), "f6");

    // Like in most formats, Some(None) collapses to null on the wire (and
    // deser fills the inner option).
    let bytes = deser_cbor::to_vec(&Some(Option::<String>::None)).unwrap();
    assert_eq!(common::to_hex(&bytes), "f6");
    assert_eq!(
        deser_cbor::from_slice::<Option<Option<String>>>(&bytes).unwrap(),
        Some(None)
    );

    // Undefined (0xf7) also decodes as None.
    assert_eq!(
        deser_cbor::from_slice::<Option<u32>>(&[0xf7]).unwrap(),
        None
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
    assert_eq!(ser(&intmap), "a2208101 0180".replace(' ', ""));

    let mut boolmap = BTreeMap::new();
    boolmap.insert(false, 1u32);
    boolmap.insert(true, 2u32);
    roundtrip(boolmap);

    let mut vecmap = BTreeMap::new();
    vecmap.insert(vec![1u32, 2], "x".to_string());
    roundtrip(vecmap);
}

#[test]
fn byte_strings() {
    // Vec<u8> is a byte string.
    let bytes = vec![1u8, 2, 3, 4];
    let encoded = deser_cbor::to_vec(&bytes).unwrap();
    assert_eq!(common::to_hex(&encoded), "4401020304");
    let back: Vec<u8> = deser_cbor::from_slice(&encoded).unwrap();
    assert_eq!(back, bytes);
    roundtrip(bytes);
    roundtrip(Vec::<u8>::new());

    // An array of ints also decodes as Vec<u8>.
    let back: Vec<u8> = de("8401020304").unwrap();
    assert_eq!(back, vec![1, 2, 3, 4]);
    let back: [u8; 4] = de("4401020304").unwrap();
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
        "a4646e616d65617864 73697a65182a 6474616773 80 65726174696ff93800".replace(' ', "")
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
    assert_eq!(ser(&Enum::Unit), "64556e6974"); // "Unit"
    assert_eq!(ser(&Enum::Newtype(42)), "a1674e657774797065182a"); // {"Newtype": 42}
    assert_eq!(ser(&Enum::Tuple(1, 2)), "a1655475706c65820102"); // {"Tuple": [1, 2]}
    assert_eq!(ser(&Enum::Struct { x: 7 }), "a166537472756374a1617807"); // {"Struct": {"x": 7}}

    // Indefinite length maps work too.
    assert_eq!(
        de::<Enum>("bf674e657774797065182aff").unwrap(),
        Enum::Newtype(42)
    );
    assert_eq!(
        de::<Enum>("bf655475706c659f0102ffff").unwrap(),
        Enum::Tuple(1, 2)
    );
    assert_eq!(
        de::<Enum>("bf66537472756374bf617805ffff").unwrap(),
        Enum::Struct { x: 5 }
    );

    // The bare-text form only encodes unit variants.
    assert!(de::<Enum>("674e657774797065").is_err());
    assert!(de::<Enum>("655475706c65").is_err());
    // A multi-entry map does not encode an enum.
    assert!(de::<Enum>("a2674e657774797065182a6155f6").is_err());
    assert!(de::<Enum>("bfff").is_err());
}

#[test]
fn internally_tagged_enums_buffer_non_string_keys() {
    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    #[deser(tag = "t")]
    enum Message {
        Stats {
            counts: HashMap<u32, u32>,
            total: u128,
        },
    }

    // {"counts": {1: 2}, "total": 2^128 - 1, "t": "Stats"}: the tag comes
    // last so the content is buffered and replayed
    let mut bytes = hex("a3 66636f756e7473 a10102 65746f74616c");
    bytes.extend(deser_cbor::to_vec(&u128::MAX).unwrap());
    bytes.extend(hex("6174 655374617473"));
    let msg: Message = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(
        msg,
        Message::Stats {
            counts: [(1, 2)].into_iter().collect(),
            total: u128::MAX,
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
    let full = deser_cbor::to_vec(&Plain {
        name: "x".into(),
        size: 42,
        tags: vec!["a".into(), "b".into()],
        ratio: Some(0.5),
    })
    .unwrap();
    let small: Small = deser_cbor::from_slice(&full).unwrap();
    assert_eq!(small, Small { name: "x".into() });

    // Unknown fields with tagged or indefinite values are ignored too.
    let small: Small = de("bf 6178 c1 9f 01 a1 f5 f6 ff 646e616d65 6178 6179 c2 41 01 ff").unwrap();
    assert_eq!(small, Small { name: "x".into() });
}

#[test]
fn indefinite_containers_decode() {
    // [_ 1, 2] (indefinite array)
    assert_eq!(de::<Vec<u32>>("9f0102ff").unwrap(), vec![1, 2]);

    // {_ "a": 1} (indefinite map)
    let map: BTreeMap<String, u32> = de("bf616101ff").unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map["a"], 1);

    // (_ "strea", "ming") (segmented text)
    assert_eq!(
        de::<String>("7f657374726561646d696e67ff").unwrap(),
        "streaming"
    );
    // (_ ) (empty segmented text)
    assert_eq!(de::<String>("7fff").unwrap(), "");

    // (_ h'0102', h'030405') (segmented bytes)
    assert_eq!(
        de::<Vec<u8>>("5f42010243030405ff").unwrap(),
        vec![1, 2, 3, 4, 5]
    );
}

#[test]
fn unknown_tags_are_transparent() {
    // 4711("x") decodes as a plain string when a string is requested.
    assert_eq!(de::<String>("d912676178").unwrap(), "x");

    // 1(1363896240) decodes as a plain integer.
    assert_eq!(de::<u64>("c11a514b67b0").unwrap(), 1363896240);
}

#[test]
fn stream_of_items() {
    let mut buffer = Vec::new();
    buffer.extend(deser_cbor::to_vec(&1u32).unwrap());
    buffer.extend(deser_cbor::to_vec(&"two").unwrap());
    buffer.extend(deser_cbor::to_vec(&vec![3u32]).unwrap());

    let mut de = deser_cbor::Deserializer::new(&buffer);
    let mut iter = de.iter::<Value>();
    assert_eq!(iter.next().unwrap().unwrap(), Value::from(1u64));
    assert_eq!(iter.next().unwrap().unwrap(), Value::from("two"));
    assert_eq!(iter.next().unwrap().unwrap(), array![3u64]);
    assert!(iter.next().is_none());

    // from_slice rejects trailing items.
    assert!(deser_cbor::from_slice::<u32>(&buffer).is_err());

    // A truncated trailing item is an error, not a silent end.
    let mut truncated = deser_cbor::to_vec(&1u32).unwrap();
    truncated.extend_from_slice(&[0x19, 0x01]); // u16 header missing a byte

    let mut de = deser_cbor::Deserializer::new(&truncated);
    let mut iter = de.iter::<u32>();
    assert_eq!(iter.next().unwrap().unwrap(), 1);
    assert!(iter.next().unwrap().is_err());
    assert!(iter.next().is_none());

    // Empty input is an empty sequence.
    let mut de = deser_cbor::Deserializer::new(&[]);
    assert!(de.iter::<u32>().next().is_none());

    // The iterator stops after the first error, even when the failed item
    // consumed nothing (a reserved additional-information value).
    let mut de = deser_cbor::Deserializer::new(&[0x01, 0x1c, 0x02]);
    let mut iter = de.iter::<u32>();
    assert_eq!(iter.next().unwrap().unwrap(), 1);
    let err = iter.next().unwrap().unwrap_err();
    assert!(err.to_string().contains("offset 1"), "{}", err);
    assert!(iter.next().is_none());
}

#[test]
fn deserializer_offset() {
    let bytes = deser_cbor::to_vec(&(1u64, "ab")).unwrap();
    let mut de = deser_cbor::Deserializer::new(&bytes);
    assert_eq!(de.offset(), 0);
    let _: (u64, String) = de.deserialize().unwrap();
    assert_eq!(de.offset(), bytes.len());
    assert!(de.is_end());
    de.end().unwrap();
}
