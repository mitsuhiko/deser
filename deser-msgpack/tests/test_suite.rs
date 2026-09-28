//! Runs the dataset of [kawanet/msgpack-test-suite].
//!
//! Every case holds a value and all of its valid encodings.  All encodings
//! have to decode into the value, and encoding the value has to produce one
//! of them.  Except for floats (which keep their precision) that has to be
//! the shortest encoding.
//!
//! [kawanet/msgpack-test-suite]: https://github.com/kawanet/msgpack-test-suite
use std::collections::BTreeMap;

use crate::common;

use common::{Value, to_hex};
use deser::ext::Timestamp;
use deser_msgpack::Ext;

const SUITE: &str = include_str!("data/msgpack-test-suite/msgpack-test-suite.json");

type Case = BTreeMap<String, Value>;

/// Returns the cases of the suite, which are loaded once (they are slow to
/// load in miri).
fn load() -> &'static BTreeMap<String, Vec<Case>> {
    static CASES: std::sync::OnceLock<BTreeMap<String, Vec<Case>>> = std::sync::OnceLock::new();
    CASES.get_or_init(|| deser_json::from_str(SUITE).unwrap())
}

/// Decodes the hex notation of the suite (`"c4-01-01"`).
fn hex(s: &str) -> Vec<u8> {
    common::hex(&s.replace('-', ""))
}

fn as_str(value: &Value) -> &str {
    match value {
        Value::Str(value) => value,
        other => panic!("expected string, got {:?}", other),
    }
}

fn as_int(value: &Value) -> i128 {
    match *value {
        Value::U64(value) => value.into(),
        Value::I64(value) => value.into(),
        ref other => panic!("expected integer, got {:?}", other),
    }
}

fn int(value: i128) -> Value {
    match u64::try_from(value) {
        Ok(value) => Value::U64(value),
        Err(_) => Value::I64(value.try_into().unwrap()),
    }
}

/// Returns the value a case describes.
fn expected(case: &Case) -> Value {
    // big numbers are given as strings as well
    if let Some(bignum) = case.get("bignum") {
        return int(as_str(bignum).parse().unwrap());
    }
    let (kind, value) = case
        .iter()
        .find(|(key, _)| *key != "msgpack")
        .expect("case without value");
    match kind.as_str() {
        "nil" | "bool" | "number" | "string" | "array" | "map" => value.clone(),
        "binary" => Value::Bytes(hex(as_str(value))),
        "timestamp" => match value {
            Value::Array(items) => Value::ext(Timestamp {
                seconds: as_int(&items[0]).try_into().unwrap(),
                nanosecond: as_int(&items[1]).try_into().unwrap(),
            }),
            other => panic!("invalid timestamp {:?}", other),
        },
        "ext" => match value {
            Value::Array(items) => Value::ext(Ext::new(
                as_int(&items[0]).try_into().unwrap(),
                hex(as_str(&items[1])),
            )),
            other => panic!("invalid extension {:?}", other),
        },
        other => panic!("unknown kind {}", other),
    }
}

/// Compares values, numbers are compared by their value.
fn equivalent(a: &Value, b: &Value) -> bool {
    fn number(value: &Value) -> Option<f64> {
        match *value {
            Value::U64(value) => Some(value as f64),
            Value::I64(value) => Some(value as f64),
            Value::F32(value) => Some(value.into()),
            Value::F64(value) => Some(value),
            _ => None,
        }
    }
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
        }
        (Value::Map(a), Value::Map(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| equivalent(&a.0, &b.0) && equivalent(&a.1, &b.1))
        }
        // integers are compared exactly, all of them are in the range of
        // u64 and i64
        (Value::U64(_) | Value::I64(_), Value::U64(_) | Value::I64(_)) => as_int(a) == as_int(b),
        _ => match (number(a), number(b)) {
            (Some(a), Some(b)) => a == b,
            _ => a == b,
        },
    }
}

fn is_float(value: &Value) -> bool {
    matches!(value, Value::F32(_) | Value::F64(_))
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_suite() {
    let suite = load();
    assert_eq!(suite.len(), 15);
    let mut checked = 0;
    for (group, cases) in suite {
        for case in cases {
            let value = expected(case);
            let encodings: Vec<String> = match &case["msgpack"] {
                Value::Array(items) => items
                    .iter()
                    .map(|item| as_str(item).replace('-', ""))
                    .collect(),
                other => panic!("invalid encodings {:?}", other),
            };
            let context = format!("{}: {:?}", group, value);

            // all encodings decode into the value
            for encoding in &encodings {
                let bytes = common::hex(encoding);
                let decoded: Value = deser_msgpack::from_slice(&bytes)
                    .unwrap_or_else(|err| panic!("{}: {} failed: {}", context, encoding, err));
                assert!(
                    equivalent(&decoded, &value),
                    "{}: {} decoded as {:?}",
                    context,
                    encoding,
                    decoded
                );
                // the decoded value writes the same bytes, unless integers
                // were written in a longer form
                if !matches!(decoded, Value::U64(_) | Value::I64(_)) && encoding == &encodings[0] {
                    assert_eq!(
                        to_hex(&deser_msgpack::to_vec(&decoded).unwrap()),
                        *encoding,
                        "{}",
                        context
                    );
                }
                checked += 1;
            }

            // the value encodes into one of them, the shortest unless it's
            // a float
            let encoded = to_hex(&deser_msgpack::to_vec(&value).unwrap());
            assert!(
                encodings.contains(&encoded),
                "{}: encoded as {}, expected one of {:?}",
                context,
                encoded,
                encodings
            );
            if !is_float(&value) {
                // integers are sometimes also given as exact floats
                let shortest = encodings
                    .iter()
                    .filter(|x| !x.starts_with("ca") && !x.starts_with("cb"))
                    .map(|x| x.len())
                    .min()
                    .unwrap();
                assert_eq!(encoded.len(), shortest, "{}: {}", context, encoded);
            }
        }
    }
    assert_eq!(checked, 233);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_suite_typed() {
    // the encodings also decode into typed values
    for (group, cases) in load() {
        for case in cases {
            let value = expected(case);
            let Value::Array(ref encodings) = case["msgpack"] else {
                panic!("invalid case");
            };
            for encoding in encodings {
                let bytes = hex(as_str(encoding));
                let context = format!("{}: {:?} from {:?}", group, value, encoding);
                let is_float_encoding = matches!(bytes[0], 0xca | 0xcb);
                match value {
                    Value::Null => deser_msgpack::from_slice::<()>(&bytes).unwrap(),
                    Value::Bool(expected) => {
                        assert_eq!(deser_msgpack::from_slice::<bool>(&bytes).unwrap(), expected)
                    }
                    Value::U64(_) | Value::I64(_) if !is_float_encoding => {
                        let expected = as_int(&value);
                        assert_eq!(
                            deser_msgpack::from_slice::<i128>(&bytes).unwrap(),
                            expected,
                            "{}",
                            context
                        );
                        if let Ok(expected) = i64::try_from(expected) {
                            assert_eq!(deser_msgpack::from_slice::<i64>(&bytes).unwrap(), expected);
                        }
                        if let Ok(expected) = u64::try_from(expected) {
                            assert_eq!(deser_msgpack::from_slice::<u64>(&bytes).unwrap(), expected);
                        }
                    }
                    Value::U64(_) | Value::I64(_) | Value::F64(_) => {
                        let expected = match value {
                            Value::F64(value) => value,
                            _ => as_int(&value) as f64,
                        };
                        assert_eq!(
                            deser_msgpack::from_slice::<f64>(&bytes).unwrap(),
                            expected,
                            "{}",
                            context
                        );
                    }
                    Value::Str(ref expected) => {
                        assert_eq!(
                            &deser_msgpack::from_slice::<String>(&bytes).unwrap(),
                            expected
                        );
                        // strings are borrowed from the input
                        assert_eq!(deser_msgpack::from_slice::<&str>(&bytes).unwrap(), expected);
                    }
                    Value::Bytes(ref expected) => {
                        assert_eq!(
                            &deser_msgpack::from_slice::<Vec<u8>>(&bytes).unwrap(),
                            expected
                        );
                        assert_eq!(
                            deser_msgpack::from_slice::<&[u8]>(&bytes).unwrap(),
                            expected
                        );
                    }
                    Value::Ext(ref ext) => {
                        if let Some(expected) = ext.downcast_ref::<Timestamp>() {
                            assert_eq!(
                                &deser_msgpack::from_slice::<Timestamp>(&bytes).unwrap(),
                                expected
                            );
                            // and as an extension it holds the data
                            let raw = deser_msgpack::from_slice::<Ext>(&bytes).unwrap();
                            assert_eq!(raw.kind, -1);
                            assert!(bytes.ends_with(&raw.data), "{}", context);
                        } else {
                            let expected = ext.downcast_ref::<Ext>().unwrap();
                            assert_eq!(
                                &deser_msgpack::from_slice::<Ext>(&bytes).unwrap(),
                                expected
                            );
                            // the fallback of extensions is the data
                            assert_eq!(
                                deser_msgpack::from_slice::<Vec<u8>>(&bytes).unwrap(),
                                expected.data
                            );
                        }
                    }
                    Value::Array(ref items) if items.is_empty() => {
                        assert!(
                            deser_msgpack::from_slice::<Vec<u8>>(&bytes)
                                .unwrap()
                                .is_empty()
                        );
                    }
                    Value::Array(_) | Value::Map(_) => {}
                    ref other => panic!("unexpected value {:?}", other),
                }
            }
        }
    }
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_suite_as_stream() {
    // all encodings after each other are a stream of items
    let mut bytes = Vec::new();
    let mut values_expected = Vec::new();
    for cases in load().values() {
        for case in cases {
            let Value::Array(ref encodings) = case["msgpack"] else {
                panic!("invalid case");
            };
            for encoding in encodings {
                bytes.extend(hex(as_str(encoding)));
                values_expected.push(expected(case));
            }
        }
    }
    let mut de = deser_msgpack::Deserializer::from_slice(&bytes);
    let values = de.iter::<Value>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values.len(), values_expected.len());
    for (value, expected) in values.iter().zip(&values_expected) {
        assert!(equivalent(value, expected), "{:?} != {:?}", value, expected);
    }

    #[cfg(feature = "io")]
    for &size in if cfg!(miri) {
        &[7][..]
    } else {
        &[1, 2, 3, 7, 64][..]
    } {
        let mut reader = deser::io::Reader::new(
            common::Chunked {
                input: &bytes,
                size,
            },
            deser_msgpack::DeserializerConfig::new(),
        );
        let mut count = 0;
        while let Some(value) = reader.read::<Value>().unwrap() {
            assert!(equivalent(&value, &values_expected[count]), "size {}", size);
            count += 1;
        }
        assert_eq!(count, values_expected.len());
    }
}
