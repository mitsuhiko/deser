//! The test vectors from RFC 8949 Appendix A.
//!
//! Adapted from the cbor2 test suite.  Every vector is decoded and compared
//! against the expected value.  Vectors that are in the preferred
//! serialization are also encoded back byte for byte.
mod common;

use common::{Value, hex, to_hex};

struct Vector {
    hex: &'static str,
    value: Value,
    // Whether encoding `value` reproduces `hex` exactly.
    preferred: bool,
}

fn v(hex: &'static str, value: Value, preferred: bool) -> Vector {
    Vector {
        hex,
        value,
        preferred,
    }
}

fn vectors() -> Vec<Vector> {
    vec![
        v("00", Value::from(0u64), true),
        v("01", Value::from(1u64), true),
        v("0a", Value::from(10u64), true),
        v("17", Value::from(23u64), true),
        v("1818", Value::from(24u64), true),
        v("1819", Value::from(25u64), true),
        v("1864", Value::from(100u64), true),
        v("1903e8", Value::from(1000u64), true),
        v("1a000f4240", Value::from(1000000u64), true),
        v("1b000000e8d4a51000", Value::from(1000000000000u64), true),
        v(
            "1bffffffffffffffff",
            Value::from(18446744073709551615u64),
            true,
        ),
        v(
            "c249010000000000000000",
            Value::from(18446744073709551616u128),
            true,
        ),
        v(
            "3bffffffffffffffff",
            Value::from(-18446744073709551616i128),
            true,
        ),
        v(
            "c349010000000000000000",
            Value::from(-18446744073709551617i128),
            true,
        ),
        v("20", Value::from(-1i64), true),
        v("29", Value::from(-10i64), true),
        v("3863", Value::from(-100i64), true),
        v("3903e7", Value::from(-1000i64), true),
        v("f90000", Value::from(0.0), true),
        v("f98000", Value::from(-0.0), true),
        v("f93c00", Value::from(1.0), true),
        v("fb3ff199999999999a", Value::from(1.1), true),
        v("f93e00", Value::from(1.5), true),
        v("f97bff", Value::from(65504.0), true),
        v("fa47c35000", Value::from(100000.0), true),
        v("fa7f7fffff", Value::from(3.4028234663852886e+38), true),
        v("fb7e37e43c8800759c", Value::from(1.0e+300), true),
        v("f90001", Value::from(5.960464477539063e-8), true),
        v("f90400", Value::from(0.00006103515625), true),
        v("f9c400", Value::from(-4.0), true),
        v("fbc010666666666666", Value::from(-4.1), true),
        v("f97c00", Value::from(f64::INFINITY), true),
        v("f9fc00", Value::from(f64::NEG_INFINITY), true),
        v("fa7f800000", Value::from(f64::INFINITY), false),
        v("faff800000", Value::from(f64::NEG_INFINITY), false),
        v("fb7ff0000000000000", Value::from(f64::INFINITY), false),
        v("fbfff0000000000000", Value::from(f64::NEG_INFINITY), false),
        v("f4", Value::from(false), true),
        v("f5", Value::from(true), true),
        v("f6", Value::Null, true),
        // 0xf7 (undefined) is tested separately: it decodes as null.
        v(
            "c074323031332d30332d32315432303a30343a30305a",
            Value::ext(
                "2013-03-21T20:04:00Z"
                    .parse::<deser::ext::Datetime>()
                    .unwrap(),
            ),
            true,
        ),
        v(
            "c11a514b67b0",
            Value::tag(1, Value::from(1363896240u64)),
            true,
        ),
        v(
            "c1fb41d452d9ec200000",
            Value::tag(1, Value::from(1363896240.5)),
            true,
        ),
        v(
            "d74401020304",
            Value::tag(23, Value::bytes("01020304")),
            true,
        ),
        v(
            "d818456449455446",
            Value::tag(24, Value::bytes("6449455446")),
            true,
        ),
        v(
            "d82076687474703a2f2f7777772e6578616d706c652e636f6d",
            Value::tag(32, Value::from("http://www.example.com")),
            true,
        ),
        v("40", Value::Bytes(vec![]), true),
        v("4401020304", Value::bytes("01020304"), true),
        v("60", Value::from(""), true),
        v("6161", Value::from("a"), true),
        v("6449455446", Value::from("IETF"), true),
        v("62225c", Value::from("\"\\"), true),
        v("62c3bc", Value::from("\u{00fc}"), true),
        v("63e6b0b4", Value::from("\u{6c34}"), true),
        v("64f0908591", Value::from("\u{10151}"), true),
        v("80", Value::Array(vec![]), true),
        v("83010203", array![1u64, 2u64, 3u64], true),
        v(
            "8301820203820405",
            array![1u64, array![2u64, 3u64], array![4u64, 5u64]],
            true,
        ),
        v(
            "98190102030405060708090a0b0c0d0e0f101112131415161718181819",
            Value::from((1..=25).collect::<Vec<u64>>()),
            true,
        ),
        v("a0", Value::Map(vec![]), true),
        v("a201020304", map! {1u64 => 2u64, 3u64 => 4u64}, true),
        v(
            "a26161016162820203",
            map! {"a" => 1u64, "b" => array![2u64, 3u64]},
            true,
        ),
        v("826161a161626163", array!["a", map! {"b" => "c"}], true),
        v(
            "a56161614161626142616361436164614461656145",
            map! {"a" => "A", "b" => "B", "c" => "C", "d" => "D", "e" => "E"},
            true,
        ),
        v("5f42010243030405ff", Value::bytes("0102030405"), false),
        v(
            "7f657374726561646d696e67ff",
            Value::from("streaming"),
            false,
        ),
        v("9fff", Value::Array(vec![]), false),
        v(
            "9f018202039f0405ffff",
            array![1u64, array![2u64, 3u64], array![4u64, 5u64]],
            false,
        ),
        v(
            "9f01820203820405ff",
            array![1u64, array![2u64, 3u64], array![4u64, 5u64]],
            false,
        ),
        v(
            "83018202039f0405ff",
            array![1u64, array![2u64, 3u64], array![4u64, 5u64]],
            false,
        ),
        v(
            "83019f0203ff820405",
            array![1u64, array![2u64, 3u64], array![4u64, 5u64]],
            false,
        ),
        v(
            "9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff",
            Value::from((1..=25).collect::<Vec<u64>>()),
            false,
        ),
        v(
            "bf61610161629f0203ffff",
            map! {"a" => 1u64, "b" => array![2u64, 3u64]},
            false,
        ),
        v("826161bf61626163ff", array!["a", map! {"b" => "c"}], false),
        v(
            "bf6346756ef563416d7421ff",
            map! {"Fun" => true, "Amt" => -2i64},
            false,
        ),
    ]
}

#[test]
fn decode() {
    for vector in vectors() {
        let bytes = hex(vector.hex);
        let decoded: Value = deser_cbor::from_slice(&bytes).unwrap();
        assert_eq!(decoded, vector.value, "decoding {}", vector.hex);
    }
}

#[test]
fn encode_preferred() {
    for vector in vectors().into_iter().filter(|x| x.preferred) {
        let bytes = deser_cbor::to_vec(&vector.value).unwrap();
        assert_eq!(to_hex(&bytes), vector.hex, "encoding {:?}", vector.value);
    }
}

#[test]
fn reencode_is_preferred() {
    // Decoding and encoding again always yields the preferred form, which
    // is a fixed point.
    for vector in vectors() {
        let value: Value = deser_cbor::from_slice(&hex(vector.hex)).unwrap();
        let bytes = deser_cbor::to_vec(&value).unwrap();
        let again: Value = deser_cbor::from_slice(&bytes).unwrap();
        assert_eq!(again, value, "{}", vector.hex);
        assert_eq!(deser_cbor::to_vec(&again).unwrap(), bytes, "{}", vector.hex);
        if vector.preferred {
            assert_eq!(to_hex(&bytes), vector.hex);
        }
    }
}

#[test]
fn nan() {
    for s in ["f97e00", "fa7fc00000", "fb7ff8000000000000"] {
        let decoded: Value = deser_cbor::from_slice(&hex(s)).unwrap();
        assert!(matches!(decoded, Value::F64(x) if x.is_nan()), "{}", s);
        let decoded: f64 = deser_cbor::from_slice(&hex(s)).unwrap();
        assert!(decoded.is_nan());
    }

    // NaN encodes to the preferred (smallest) form.
    assert_eq!(deser_cbor::to_vec(&f64::NAN).unwrap(), hex("f97e00"));
    assert_eq!(deser_cbor::to_vec(&f32::NAN).unwrap(), hex("f97e00"));
}

#[test]
fn undefined_decodes_as_null() {
    let decoded: Value = deser_cbor::from_slice(&[0xf7]).unwrap();
    assert_eq!(decoded, Value::Null);
}

#[test]
fn simple_values() {
    // Unassigned simple values are preserved.
    assert_eq!(
        deser_cbor::from_slice::<Value>(&hex("f0")).unwrap(),
        Value::Simple(16)
    );
    assert_eq!(
        deser_cbor::from_slice::<Value>(&hex("f8ff")).unwrap(),
        Value::Simple(255)
    );
    assert_eq!(common::ser(&Value::Simple(16)), "f0");
    assert_eq!(common::ser(&Value::Simple(255)), "f8ff");

    // Two-byte encodings of values below 32 are not well-formed.
    for x in 0u8..32 {
        let err = deser_cbor::from_slice::<Value>(&[0xf8, x]).unwrap_err();
        assert_eq!(err.kind(), deser::ErrorKind::Unexpected, "f8{:02x}", x);
        assert!(err.to_string().contains("offset 0"), "{}", err);
    }
}
