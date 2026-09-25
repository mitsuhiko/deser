//! Serialization tests including the deterministic encoding.
//!
//! The canonical tests are adapted from the cbor2 test suite.
mod common;

use std::collections::{BTreeMap, HashMap};

use common::{de, hex, ser, to_hex, Value};
use deser::Serialize;

#[test]
fn test_basic() {
    assert_eq!(ser(&[1, 2, 3, 4]), "8401020304");
    assert_eq!(ser(&"IETF"), "6449455446");
    assert_eq!(ser(&'\u{fc}'), "62c3bc");
    assert_eq!(ser(&()), "f6");
    assert_eq!(ser(&true), "f5");
    assert_eq!(ser(&Some(false)), "f4");
    assert_eq!(ser(&Vec::<u32>::new()), "80");
    assert_eq!(ser(&BTreeMap::<u32, u32>::new()), "a0");
}

#[test]
fn test_flatten() {
    #[derive(Serialize)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(Serialize)]
    pub struct Attrs {
        is_admin: bool,
        flags: Vec<String>,
    }

    let bytes = deser_cbor::to_vec(&User {
        id: 42,
        attrs: Attrs {
            is_admin: true,
            flags: vec!["x".into()],
        },
    })
    .unwrap();
    // {"id": 42, "is_admin": true, "flags": ["x"]}
    assert_eq!(
        to_hex(&bytes),
        "a3 626964 182a 6869735f61646d696e f5 65666c616773 816178".replace(' ', "")
    );
}

#[test]
fn test_container_lengths() {
    // The length of containers is not known upfront, the header is patched
    // when the container ends.  Check all header sizes.
    let lengths: &[usize] = if cfg!(miri) {
        &[0, 1, 23, 24, 255, 256]
    } else {
        &[0, 1, 23, 24, 255, 256, 65535, 65536]
    };
    for &len in lengths {
        let vec: Vec<u32> = (0..len as u32).map(|x| x % 7).collect();
        let bytes = deser_cbor::to_vec(&vec).unwrap();
        let header = match len {
            0..=23 => format!("{:02x}", 0x80 + len),
            24..=255 => format!("98{:02x}", len),
            256..=65535 => format!("99{:04x}", len),
            _ => format!("9a{:08x}", len),
        };
        assert!(to_hex(&bytes).starts_with(&header), "{}", len);
        assert_eq!(bytes.len(), header.len() / 2 + len);
        assert_eq!(deser_cbor::from_slice::<Vec<u32>>(&bytes).unwrap(), vec);

        let map: BTreeMap<u32, bool> = (0..len as u32).map(|x| (x, x % 2 == 0)).collect();
        let bytes = deser_cbor::to_vec(&map).unwrap();
        assert_eq!(bytes[0] >> 5, 5);
        assert_eq!(
            deser_cbor::from_slice::<BTreeMap<u32, bool>>(&bytes).unwrap(),
            map
        );
    }
}

#[test]
fn test_nested_container_lengths() {
    // nested containers which grow their headers move the contents of the
    // outer containers.
    let value: Vec<Vec<Vec<u8>>> = (0..30)
        .map(|x| (0..x).map(|y| vec![y as u8; y]).collect())
        .collect();
    let bytes = deser_cbor::to_vec(&value).unwrap();
    assert_eq!(
        deser_cbor::from_slice::<Vec<Vec<Vec<u8>>>>(&bytes).unwrap(),
        value
    );
    let dynamic: Value = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(deser_cbor::to_vec(&dynamic).unwrap(), bytes);

    let value: Vec<BTreeMap<String, Vec<u32>>> = (0..30)
        .map(|x| {
            (0..x)
                .map(|y| (format!("key{}", y), (0..y).collect()))
                .collect()
        })
        .collect();
    let bytes = deser_cbor::to_vec(&value).unwrap();
    assert_eq!(
        deser_cbor::from_slice::<Vec<BTreeMap<String, Vec<u32>>>>(&bytes).unwrap(),
        value
    );
}

#[test]
fn test_string_lengths() {
    let lengths: &[usize] = if cfg!(miri) {
        &[0, 23, 24, 255, 256]
    } else {
        &[0, 23, 24, 255, 256, 65535, 65536]
    };
    for &len in lengths {
        let s = "x".repeat(len);
        let bytes = deser_cbor::to_vec(&s).unwrap();
        assert_eq!(deser_cbor::from_slice::<String>(&bytes).unwrap(), s);
        let b = vec![7u8; len];
        let bytes = deser_cbor::to_vec(&b).unwrap();
        assert_eq!(deser_cbor::from_slice::<Vec<u8>>(&bytes).unwrap(), b);
    }
    assert_eq!(&ser(&"x".repeat(24))[..4], "7818");
    assert_eq!(&ser(&vec![0u8; 256])[..6], "590100");
}

#[test]
fn test_simple_values() {
    use deser_cbor::Simple;

    assert_eq!(ser(&Simple::new(0).unwrap()), "e0");
    assert_eq!(ser(&Simple::new(19).unwrap()), "f3");
    assert_eq!(ser(&Simple::new(32).unwrap()), "f820");
    assert!(Simple::new(24).is_none());
    assert!(Simple::new(31).is_none());
    assert_eq!(de::<Simple>("f3").unwrap(), Simple::new(19).unwrap());
    assert_eq!(de::<Vec<Simple>>("82f820f6").unwrap().len(), 2);
    // The fallback of a simple value is its number.
    assert_eq!(de::<u8>("f0").unwrap(), 16);
}

#[test]
fn test_extension_fallback() {
    use deser::ext::{ExtValue, Extension};
    use deser::ser::{Chunk, SerializerState};
    use deser::{Atom, Error};

    #[derive(Debug, Clone, PartialEq)]
    struct Timestamp(i64);

    impl Extension for Timestamp {
        fn name(&self) -> &str {
            "timestamp"
        }

        fn fallback(&self) -> Atom<'_> {
            Atom::I64(self.0)
        }
    }

    impl Serialize for Timestamp {
        fn serialize(&self, _state: &mut SerializerState) -> Result<Chunk<'_>, Error> {
            Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
        }
    }

    // unknown extension values are written as their fallback
    assert_eq!(ser(&Timestamp(-2)), "21");
    assert_eq!(ser(&vec![Timestamp(1), Timestamp(2)]), "820102");
}

#[test]
fn test_owned_atoms() {
    use std::borrow::Cow;

    let strings: Vec<Cow<'static, str>> = vec![Cow::Owned("owned".into()), Cow::Borrowed("b")];
    assert_eq!(ser(&strings), "82656f776e65646162");
}

// The eight example keys shared by RFC 8949 §4.2.1 and §4.2.3, scrambled.
fn rfc_example_map() -> Value {
    Value::Map(vec![
        (array![-1i64], Value::from(6u64)),
        (Value::from(false), Value::from(7u64)),
        (Value::from("aa"), Value::from(4u64)),
        (Value::from(100u64), Value::from(1u64)),
        (Value::from(-1i64), Value::from(2u64)),
        (array![100u64], Value::from(5u64)),
        (Value::from(10u64), Value::from(0u64)),
        (Value::from("z"), Value::from(3u64)),
    ])
}

fn keys_of(bytes: &[u8]) -> Vec<String> {
    match deser_cbor::from_slice::<Value>(bytes).unwrap() {
        Value::Map(entries) => entries
            .iter()
            .map(|(k, _)| to_hex(&deser_cbor::to_vec(k).unwrap()))
            .collect(),
        _ => panic!("not a map"),
    }
}

#[test]
fn canonical_rfc_key_order_example() {
    // RFC 8949 §4.2.1 (bytewise lexicographic): the keys must come out as
    // 10, 100, -1, "z", "aa", [100], [-1], false.
    let bytes = deser_cbor::to_canonical_vec(&rfc_example_map()).unwrap();
    assert_eq!(
        keys_of(&bytes),
        ["0a", "1864", "20", "617a", "626161", "811864", "8120", "f4"]
    );
    // the regular encoding keeps the order
    let bytes = deser_cbor::to_vec(&rfc_example_map()).unwrap();
    assert_eq!(
        keys_of(&bytes),
        ["8120", "f4", "626161", "1864", "20", "811864", "0a", "617a"]
    );
}

#[test]
fn canonical_hash_maps() {
    let map: HashMap<i64, bool> = [(100, true), (-1, false)].into_iter().collect();
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&map).unwrap()),
        "a21864f520f4"
    );

    // HashMap iteration order is nondeterministic; the canonical encoding
    // is not.
    let map: HashMap<&str, i32> = [("z", 1), ("aa", 2), ("b", 3), ("c", 4)]
        .into_iter()
        .collect();
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&map).unwrap()),
        "a461620361630461 7a01626161 02".replace(' ', "")
    );

    // large maps with longer headers
    let map: HashMap<u32, u32> = (0..1000).map(|x| (x, x)).collect();
    let bytes = deser_cbor::to_canonical_vec(&map).unwrap();
    let sorted: BTreeMap<u32, u32> = (0..1000).map(|x| (x, x)).collect();
    assert_eq!(bytes, deser_cbor::to_vec(&sorted).unwrap());
}

#[test]
fn canonical_structs() {
    // Struct fields are sorted too (here "b" < "a" is fixed up).
    #[derive(Serialize)]
    struct Unsorted {
        b: u8,
        a: u8,
    }
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&Unsorted { b: 1, a: 2 }).unwrap()),
        "a26161026162 01".replace(' ', "")
    );
    assert_eq!(
        ser(&Unsorted { b: 1, a: 2 }),
        "a26162016161 02".replace(' ', "")
    );
}

#[test]
fn canonical_sorting_recurses() {
    // Nested maps are sorted wherever they appear: in values, in array
    // elements, inside tags and even when used as keys.
    let inner = || map! {"z" => 1u64, "aa" => 2u64};
    let value = map! {
        "outer" => inner(),
        "list" => array![inner()],
    };
    let sorted_inner = "a2617a01626161 02".replace(' ', "");
    let expected = format!(
        "a2 646c697374 81{} 656f75746572 {}",
        sorted_inner, sorted_inner
    )
    .replace(' ', "");
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&value).unwrap()),
        expected
    );

    let tagged = Value::tag(1000, value.clone());
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&tagged).unwrap()),
        format!("d903e8{}", expected)
    );

    let map_key = Value::Map(vec![(value, Value::Null)]);
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&map_key).unwrap()),
        format!("a1{}f6", expected)
    );
}

#[test]
fn canonical_duplicate_keys_are_rejected() {
    let dup = || {
        Value::Map(vec![
            (Value::from(1u64), Value::Null),
            (Value::from(1u64), Value::Null),
        ])
    };
    assert!(deser_cbor::to_canonical_vec(&dup()).is_err());
    // the regular encoding writes them
    assert_eq!(ser(&dup()), "a201f601f6");

    // Keys that are only equal after normalization also count.
    let value = Value::Map(vec![
        (Value::from(1u64), Value::from("a")),
        (Value::from(1u128), Value::from("b")),
    ]);
    assert!(deser_cbor::to_canonical_vec(&value).is_err());

    // Nested failures propagate.
    assert!(deser_cbor::to_canonical_vec(&array![dup()]).is_err());
    assert!(deser_cbor::to_canonical_vec(&Value::tag(9, dup())).is_err());
    assert!(deser_cbor::to_canonical_vec(&Value::Map(vec![(dup(), Value::Null)])).is_err());
    assert!(deser_cbor::to_canonical_vec(&Value::Map(vec![(Value::Null, dup())])).is_err());
}

#[test]
fn canonical_decode_encode_roundtrip_is_stable() {
    // Decoding an indefinite-length, unsorted document and re-encoding it
    // canonically yields a definite-length, sorted document; doing it
    // again is a fixed point.
    let messy = hex("bf617a017f626161ff9f0102ffff"); // {_ "z": 1, (_ "aa"): [_ 1, 2]}
    let value: Value = deser_cbor::from_slice(&messy).unwrap();

    let once = deser_cbor::to_canonical_vec(&value).unwrap();
    let twice =
        deser_cbor::to_canonical_vec(&deser_cbor::from_slice::<Value>(&once).unwrap()).unwrap();

    assert_eq!(once, twice);
    assert_eq!(to_hex(&once), "a2617a01626161 820102".replace(' ', ""));
}

#[test]
fn canonical_floats_and_nan() {
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&f64::from_bits(0x7ff8_dead_beef_0000)).unwrap()),
        "f97e00"
    );
    assert_eq!(
        to_hex(&deser_cbor::to_canonical_vec(&vec![1.0f64, 1.5, 0.1]).unwrap()),
        "83f93c00f93e00fb3fb999999999999a"
    );
}
