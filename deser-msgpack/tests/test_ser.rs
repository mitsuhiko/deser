//! Serialization tests including the deterministic encoding.
use crate::common;

use std::collections::{BTreeMap, HashMap};

use common::{Value, hex, ser, to_hex};
use deser::Serialize;
use deser_msgpack::SerializerConfig;

const CANONICAL: SerializerConfig = SerializerConfig::new().canonical(true);

#[test]
fn test_basic() {
    assert_eq!(ser(&[1, 2, 3, 4]), "9401020304");
    assert_eq!(ser(&"IETF"), "a449455446");
    assert_eq!(ser(&'\u{fc}'), "a2c3bc");
    assert_eq!(ser(&()), "c0");
    assert_eq!(ser(&true), "c3");
    assert_eq!(ser(&Some(false)), "c2");
    assert_eq!(ser(&Vec::<u32>::new()), "90");
    assert_eq!(ser(&BTreeMap::<u32, u32>::new()), "80");
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

    let bytes = deser_msgpack::to_vec(&User {
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
        "83 a26964 2a a869735f61646d696e c3 a5666c616773 91a178".replace(' ', "")
    );
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_container_lengths() {
    // The length of containers is not known upfront, the header is patched
    // when the container ends.  Check all header sizes.
    let lengths: &[usize] = if cfg!(miri) {
        &[0, 1, 15, 16, 255, 256]
    } else {
        &[0, 1, 15, 16, 255, 256, 65535, 65536]
    };
    for &len in lengths {
        let vec: Vec<u32> = (0..len as u32).map(|x| x % 7).collect();
        let bytes = deser_msgpack::to_vec(&vec).unwrap();
        let header = match len {
            0..=15 => format!("{:02x}", 0x90 + len),
            16..=65535 => format!("dc{:04x}", len),
            _ => format!("dd{:08x}", len),
        };
        assert!(to_hex(&bytes).starts_with(&header), "{}", len);
        assert_eq!(bytes.len(), header.len() / 2 + len);
        assert_eq!(deser_msgpack::from_slice::<Vec<u32>>(&bytes).unwrap(), vec);

        let map: BTreeMap<u32, bool> = (0..len as u32).map(|x| (x, x % 2 == 0)).collect();
        let bytes = deser_msgpack::to_vec(&map).unwrap();
        let header = match len {
            0..=15 => format!("{:02x}", 0x80 + len),
            16..=65535 => format!("de{:04x}", len),
            _ => format!("df{:08x}", len),
        };
        assert!(to_hex(&bytes).starts_with(&header), "{}", len);
        assert_eq!(
            deser_msgpack::from_slice::<BTreeMap<u32, bool>>(&bytes).unwrap(),
            map
        );
    }
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_nested_container_lengths() {
    // nested containers which grow their headers move the contents of the
    // outer containers.  In miri only just past the first header growth.
    let count: usize = if cfg!(miri) { 18 } else { 30 };
    let value: Vec<Vec<Vec<u8>>> = (0..count)
        .map(|x| (0..x).map(|y| vec![y as u8; y]).collect())
        .collect();
    let bytes = deser_msgpack::to_vec(&value).unwrap();
    assert_eq!(
        deser_msgpack::from_slice::<Vec<Vec<Vec<u8>>>>(&bytes).unwrap(),
        value
    );
    let dynamic: Value = deser_msgpack::from_slice(&bytes).unwrap();
    assert_eq!(deser_msgpack::to_vec(&dynamic).unwrap(), bytes);

    let value: Vec<BTreeMap<String, Vec<u32>>> = (0..count as u32)
        .map(|x| {
            (0..x)
                .map(|y| (format!("key{}", y), (0..y).collect()))
                .collect()
        })
        .collect();
    let bytes = deser_msgpack::to_vec(&value).unwrap();
    assert_eq!(
        deser_msgpack::from_slice::<Vec<BTreeMap<String, Vec<u32>>>>(&bytes).unwrap(),
        value
    );
}

#[test]
fn test_string_lengths() {
    let lengths: &[usize] = if cfg!(miri) {
        &[0, 31, 32, 255, 256]
    } else {
        &[0, 31, 32, 255, 256, 65535, 65536]
    };
    for &len in lengths {
        let s = "x".repeat(len);
        let bytes = deser_msgpack::to_vec(&s).unwrap();
        assert_eq!(deser_msgpack::from_slice::<String>(&bytes).unwrap(), s);
        let b = vec![7u8; len];
        let bytes = deser_msgpack::to_vec(&b).unwrap();
        assert_eq!(deser_msgpack::from_slice::<Vec<u8>>(&bytes).unwrap(), b);
    }
    // (the prefixes are compared as bytes, hex is slow in miri)
    let prefix = |value: deser::ser::SerializeRef<'_>, len: usize| {
        deser_msgpack::to_vec(&value).unwrap()[..len].to_vec()
    };
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&"x".repeat(31)), 1),
        hex("bf")
    );
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&"x".repeat(32)), 2),
        hex("d920")
    );
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&"x".repeat(256)), 3),
        hex("da0100")
    );
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&"x".repeat(65536)), 5),
        hex("db00010000")
    );
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&vec![0u8; 0]), 2),
        hex("c400")
    );
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&vec![0u8; 256]), 3),
        hex("c50100")
    );
    assert_eq!(
        prefix(deser::ser::SerializeRef::new(&vec![0u8; 65536]), 5),
        hex("c600010000")
    );
}

#[test]
fn test_extensions() {
    use deser_msgpack::Ext;

    // fixext for the sizes 1, 2, 4, 8 and 16, ext 8, 16 and 32 otherwise
    assert_eq!(ser(&Ext::new(1, [0x10])), "d40110");
    assert_eq!(ser(&Ext::new(2, [0x20, 0x21])), "d5022021");
    assert_eq!(ser(&Ext::new(3, [0; 4])), "d60300000000");
    assert_eq!(ser(&Ext::new(4, [0; 8])), "d7040000000000000000");
    assert_eq!(&ser(&Ext::new(5, [0; 16]))[..4], "d805");
    assert_eq!(ser(&Ext::new(6, [])), "c70006");
    assert_eq!(ser(&Ext::new(7, [0x70, 0x71, 0x72])), "c70307707172");
    let prefix = |value: &Ext, len: usize| deser_msgpack::to_vec(value).unwrap()[..len].to_vec();
    assert_eq!(prefix(&Ext::new(-2, vec![0; 256]), 4), hex("c80100fe"));
    assert_eq!(prefix(&Ext::new(8, vec![0; 65536]), 6), hex("c90001000008"));
}

#[test]
fn test_extension_fallback() {
    use deser::State;
    use deser::ext::{ExtValue, Extension};
    use deser::ser::Chunk;
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
        fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
            Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(value))))
        }
    }

    // unknown extension values are written as their fallback
    assert_eq!(ser(&Timestamp(-2)), "fe");
    assert_eq!(ser(&vec![Timestamp(1), Timestamp(2)]), "920102");
}

#[test]
fn test_owned_atoms() {
    use std::borrow::Cow;

    let strings: Vec<Cow<'static, str>> = vec![Cow::Owned("owned".into()), Cow::Borrowed("b")];
    assert_eq!(ser(&strings), "92a56f776e6564a162");
}

// Keys of different types, scrambled.
fn example_map() -> Value {
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
    match deser_msgpack::from_slice::<Value>(bytes).unwrap() {
        Value::Map(entries) => entries
            .iter()
            .map(|(k, _)| to_hex(&deser_msgpack::to_vec(k).unwrap()))
            .collect(),
        _ => panic!("not a map"),
    }
}

#[test]
fn canonical_key_order() {
    // bytewise lexicographic order of the encoded keys
    let bytes = CANONICAL.to_vec(&example_map()).unwrap();
    assert_eq!(
        keys_of(&bytes),
        ["0a", "64", "9164", "91ff", "a17a", "a26161", "c2", "ff"]
    );
    // the regular encoding keeps the order
    let bytes = deser_msgpack::to_vec(&example_map()).unwrap();
    assert_eq!(
        keys_of(&bytes),
        ["91ff", "c2", "a26161", "64", "ff", "9164", "0a", "a17a"]
    );
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn canonical_hash_maps() {
    let map: HashMap<i64, bool> = [(100, true), (-1, false)].into_iter().collect();
    assert_eq!(to_hex(&CANONICAL.to_vec(&map).unwrap()), "8264c3ffc2");

    // HashMap iteration order is nondeterministic; the canonical encoding
    // is not.
    let map: HashMap<&str, i32> = [("z", 1), ("aa", 2), ("b", 3), ("c", 4)]
        .into_iter()
        .collect();
    assert_eq!(
        to_hex(&CANONICAL.to_vec(&map).unwrap()),
        "84 a16203 a16304 a17a01 a2616102".replace(' ', "")
    );

    // large maps with longer headers (the keys are sorted by their encoding
    // which is not the numeric order)
    let len = if cfg!(miri) { 300 } else { 1000 };
    let map: HashMap<u32, u32> = (0..len).map(|x| (x, x)).collect();
    let bytes = CANONICAL.to_vec(&map).unwrap();
    let mut keys: Vec<Vec<u8>> = (0..len)
        .map(|x| deser_msgpack::to_vec(&x).unwrap())
        .collect();
    keys.sort();
    let mut expected = hex(&format!("de{:04x}", len));
    for key in keys {
        let value = key.clone();
        expected.extend(key);
        expected.extend(value);
    }
    assert_eq!(bytes, expected);
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
        to_hex(&CANONICAL.to_vec(&Unsorted { b: 1, a: 2 }).unwrap()),
        "82a16102a16201"
    );
    assert_eq!(ser(&Unsorted { b: 1, a: 2 }), "82a16201a16102");
}

#[test]
fn canonical_sorting_recurses() {
    // Nested maps are sorted wherever they appear: in values, in array
    // elements and even when used as keys.
    let inner = || map! {"z" => 1u64, "aa" => 2u64};
    let value = map! {
        "outer" => inner(),
        "list" => array![inner()],
    };
    let sorted_inner = "82a17a01a2616102";
    let expected = format!(
        "82 a46c697374 91{} a56f75746572 {}",
        sorted_inner, sorted_inner
    )
    .replace(' ', "");
    assert_eq!(to_hex(&CANONICAL.to_vec(&value).unwrap()), expected);

    let map_key = Value::Map(vec![(value, Value::Null)]);
    assert_eq!(
        to_hex(&CANONICAL.to_vec(&map_key).unwrap()),
        format!("81{}c0", expected)
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
    assert!(CANONICAL.to_vec(&dup()).is_err());
    // the regular encoding writes them
    assert_eq!(ser(&dup()), "8201c001c0");

    // Keys that are only equal after normalization also count.
    let value = Value::Map(vec![
        (Value::from(1u64), Value::from("a")),
        (Value::from(1u128), Value::from("b")),
    ]);
    assert!(CANONICAL.to_vec(&value).is_err());
    let value = Value::Map(vec![
        (Value::from(1u64), Value::from("a")),
        (Value::I64(1), Value::from("b")),
    ]);
    assert!(CANONICAL.to_vec(&value).is_err());

    // Nested failures propagate.
    assert!(CANONICAL.to_vec(&array![dup()]).is_err());
    assert!(
        CANONICAL
            .to_vec(&Value::Map(vec![(dup(), Value::Null)]))
            .is_err()
    );
    assert!(
        CANONICAL
            .to_vec(&Value::Map(vec![(Value::Null, dup())]))
            .is_err()
    );
}

#[test]
fn canonical_decode_encode_roundtrip_is_stable() {
    // Decoding an unsorted document with long headers and re-encoding it
    // canonically yields a sorted document with short headers; doing it
    // again is a fixed point.
    let messy = hex("de0002 a17a 01 d9026161 dc0002 01 02"); // {"z": 1, "aa": [1, 2]}
    let value: Value = deser_msgpack::from_slice(&messy).unwrap();

    let once = CANONICAL.to_vec(&value).unwrap();
    let twice = CANONICAL
        .to_vec(&deser_msgpack::from_slice::<Value>(&once).unwrap())
        .unwrap();

    assert_eq!(once, twice);
    assert_eq!(to_hex(&once), "82a17a01a26161920102");
}

#[test]
fn floats_keep_their_precision() {
    assert_eq!(ser(&1.5f64), "cb3ff8000000000000");
    assert_eq!(ser(&1.5f32), "ca3fc00000");
    assert_eq!(ser(&-0.0f64), "cb8000000000000000");
    assert_eq!(ser(&f32::INFINITY), "ca7f800000");
    assert_eq!(ser(&f64::NAN), "cb7ff8000000000000");
}

#[test]
fn big_integers_are_out_of_range() {
    // 128 bit integers are written if they fit into 64 bits
    assert_eq!(ser(&(u64::MAX as u128)), "cfffffffffffffffff");
    assert_eq!(ser(&(i64::MIN as i128)), "d38000000000000000");
    assert_eq!(ser(&(u64::MAX as i128)), "cfffffffffffffffff");
    assert_eq!(ser(&-1i128), "ff");
    for err in [
        deser_msgpack::to_vec(&(u64::MAX as u128 + 1)).unwrap_err(),
        deser_msgpack::to_vec(&(i64::MIN as i128 - 1)).unwrap_err(),
        deser_msgpack::to_vec(&(u64::MAX as i128 + 1)).unwrap_err(),
    ] {
        assert_eq!(err.kind(), deser::ErrorKind::OutOfRange);
        assert_eq!(
            err.to_string(),
            "OutOfRange: integer out of range for MessagePack"
        );
    }
    // big integers are written as their fallback
    let big: deser::ext::BigInt = "18446744073709551616".parse().unwrap();
    assert_eq!(
        ser(&Value::ext(big)),
        "b43138343436373434303733373039353531363136"
    );
}

/// A sequence that reports a wrong length in its shape.
struct Liar(usize);

impl Serialize for Liar {
    fn serialize<'a>(
        _value: &'a Self,
        state: &mut deser::State,
    ) -> Result<deser::ser::Chunk<'a>, deser::Error> {
        struct Emitter(usize);
        impl deser::ser::SeqEmitter for Emitter {
            fn next(
                &mut self,
                state: &mut deser::State,
            ) -> Result<Option<deser::ser::SerializeHandle<'_>>, deser::Error> {
                Ok(if self.0 > 0 {
                    self.0 -= 1;
                    Some(deser::ser::SerializeHandle::arena(1u64, state))
                } else {
                    None
                })
            }
        }
        Ok(deser::ser::Chunk::seq(Emitter(2), state))
    }

    fn container_shape(value: &Self) -> deser::ContainerShape {
        deser::ContainerShape::new().with_len(value.0)
    }
}

#[test]
fn test_known_lengths() {
    // known lengths are written upfront, the output is the same
    assert_eq!(ser(&Liar(2)), "920101");
    let long = (0..30u64).collect::<Vec<_>>();
    assert_eq!(&ser(&long)[..6], "dc001e");
    // a wrong length is an error
    let err = deser_msgpack::to_vec(&Liar(3)).unwrap_err();
    assert!(err.to_string().contains("does not match"), "{}", err);
    assert!(deser_msgpack::to_vec(&Liar(1)).is_err());
    // a length beyond 32 bits cannot be written
    if usize::BITS > 32 {
        let err = deser_msgpack::to_vec(&Liar(u32::MAX as usize + 1)).unwrap_err();
        assert_eq!(err.kind(), deser::ErrorKind::OutOfRange);
    }
}

#[test]
fn test_lengths_are_passed_on() {
    let mut shapes = Vec::new();
    let recording: deser::de::Recording =
        deser_msgpack::from_slice(&hex("82a16101a162929102dc0000")).unwrap();
    for event in recording.events() {
        if let deser::Event::MapStart(shape) | deser::Event::SeqStart(shape) = event {
            shapes.push(shape.len());
        }
    }
    assert_eq!(shapes, [Some(2), Some(2), Some(1), Some(0)]);
}
