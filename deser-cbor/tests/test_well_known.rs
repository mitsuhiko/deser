//! Tests for the well-known types of deser.
use crate::common;

use common::{Value, de, ser};
use deser::ext::{BigInt, Datetime, Decimal, Duration, Timestamp, Uuid};

fn datetime(s: &str) -> Datetime {
    s.parse().unwrap()
}

#[test]
fn datetimes() {
    // offset date-times use tag 0
    let value = datetime("2013-03-21T20:04:00Z");
    let hex = ser(&value);
    assert_eq!(hex, "c074323031332d30332d32315432303a30343a30305a");
    assert_eq!(de::<Datetime>(&hex).unwrap(), value);
    assert_eq!(de::<String>(&hex).unwrap(), "2013-03-21T20:04:00Z");
    // local dates use tag 1004 (RFC 8943)
    let value = datetime("2013-03-21");
    let hex = ser(&value);
    assert_eq!(hex, "d903ec6a323031332d30332d3231");
    assert_eq!(de::<Value>(&hex).unwrap(), Value::ext(value));
    // other kinds of date-times are strings
    let value = datetime("20:04:00");
    assert_eq!(ser(&value), "6832303a30343a3030");
    assert_eq!(de::<Datetime>(&ser(&value)).unwrap(), value);
    // tag 0 with a local date-time is not a valid date/time string
    let hex = ser(&deser_cbor::Tagged::new(0, "2013-03-21T20:04:00"));
    assert_eq!(
        de::<Value>(&hex).unwrap(),
        Value::tag(0, Value::from("2013-03-21T20:04:00"))
    );
}

#[test]
fn timestamps() {
    // whole seconds use tag 1
    let value = Timestamp {
        seconds: 1363896240,
        nanosecond: 0,
    };
    assert_eq!(ser(&value), "c11a514b67b0");
    assert_eq!(de::<Timestamp>("c11a514b67b0").unwrap(), value);
    // floats are accepted
    assert_eq!(
        de::<Timestamp>("c1fb41d452d9ec200000").unwrap(),
        Timestamp {
            seconds: 1363896240,
            nanosecond: 500_000_000
        }
    );
    // fractions are written as date/time strings to retain the precision
    let value = Timestamp {
        seconds: 1363896240,
        nanosecond: 5,
    };
    let hex = ser(&value);
    assert!(hex.starts_with("c0"));
    assert_eq!(de::<Timestamp>(&hex).unwrap(), value);
    // std types work as well
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1363896240);
    assert_eq!(ser(&time), "c11a514b67b0");
    assert_eq!(de::<std::time::SystemTime>("c11a514b67b0").unwrap(), time);
}

#[test]
fn uuids() {
    let value: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    let hex = ser(&value);
    assert_eq!(hex, "d8255067e5504410b1426f9247bb680e5fe0c8");
    assert_eq!(de::<Value>(&hex).unwrap(), Value::ext(value));
    assert_eq!(de::<Uuid>(&hex).unwrap(), value);
    // untagged bytes are accepted too
    assert_eq!(de::<Uuid>(&hex[4..]).unwrap(), value);
    // tag 37 with the wrong content stays tagged
    assert_eq!(
        de::<Value>("d8254101").unwrap(),
        Value::tag(37, Value::bytes("01"))
    );
}

#[test]
fn decimals() {
    // RFC 8949 example: 273.15 is [-2, 27315]
    let value: Decimal = "273.15".parse().unwrap();
    let hex = ser(&value);
    assert_eq!(hex, "c48221196ab3");
    assert_eq!(de::<Decimal>(&hex).unwrap(), value);
    assert_eq!(de::<String>(&hex).unwrap(), "273.15");
    // mantissas can be bignums
    let value: Decimal = "-123456789012345678901234567890.5".parse().unwrap();
    let hex = ser(&value);
    assert!(hex.starts_with("c48220c3"));
    assert_eq!(de::<Decimal>(&hex).unwrap(), value);
    // positive exponents
    let value: Decimal = "15e9".parse().unwrap();
    assert_eq!(de::<Decimal>(&ser(&value)).unwrap(), value);
    // malformed decimal fractions stay tagged arrays
    assert_eq!(
        de::<Value>("c48101").unwrap(),
        Value::tag(4, Value::Array(vec![Value::from(1u64)]))
    );
}

#[test]
fn big_integers() {
    for s in [
        "340282366920938463463374607431768211456",
        "-340282366920938463463374607431768211457",
        "-123456789012345678901234567890123456789012",
    ] {
        let value: BigInt = s.parse().unwrap();
        let hex = ser(&value);
        assert_eq!(de::<BigInt>(&hex).unwrap(), value, "{}", s);
    }
    // small values are plain integers
    assert_eq!(ser(&BigInt::from(-1i64)), "20");
    assert_eq!(
        ser(&BigInt::from(u128::MAX)),
        "c250ffffffffffffffffffffffffffffffff"
    );
}

#[test]
fn durations() {
    let value = Duration {
        seconds: 90,
        nanosecond: 0,
    };
    assert_eq!(ser(&value), "675054314d333053");
    assert_eq!(de::<Duration>("675054314d333053").unwrap(), value);
}
