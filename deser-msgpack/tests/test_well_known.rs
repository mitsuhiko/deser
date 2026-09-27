//! Tests for the well-known types of deser and extensions.
use crate::common;

use common::{Value, de, ser};
use deser::ext::{BigInt, Datetime, Decimal, Duration, Timestamp, Uuid};
use deser_msgpack::Ext;

#[test]
fn timestamps() {
    // the smallest of the three timestamp formats is used
    let value = Timestamp {
        seconds: 1363896240,
        nanosecond: 0,
    };
    assert_eq!(ser(&value), "d6ff514b67b0");
    assert_eq!(de::<Timestamp>("d6ff514b67b0").unwrap(), value);
    assert_eq!(de::<Value>("d6ff514b67b0").unwrap(), Value::ext(value));
    let value = Timestamp {
        seconds: 1363896240,
        nanosecond: 5,
    };
    assert_eq!(ser(&value), "d7ff00000014514b67b0");
    assert_eq!(de::<Timestamp>("d7ff00000014514b67b0").unwrap(), value);
    let value = Timestamp {
        seconds: -1,
        nanosecond: 500_000_000,
    };
    assert_eq!(ser(&value), "c70cff1dcd6500ffffffffffffffff");
    assert_eq!(de::<Timestamp>(&ser(&value)).unwrap(), value);

    // timestamps are also accepted as strings and numbers
    assert_eq!(
        de::<Timestamp>("b4323031332d30332d32315432303a30343a30305a").unwrap(),
        Timestamp {
            seconds: 1363896240,
            nanosecond: 0
        }
    );
    assert_eq!(
        de::<Timestamp>("ce514b67b0").unwrap(),
        Timestamp {
            seconds: 1363896240,
            nanosecond: 0
        }
    );
    // the fallback of timestamps is a string
    assert_eq!(
        de::<String>("d6ff514b67b0").unwrap(),
        "2013-03-21T20:04:00Z"
    );

    // invalid timestamps (nanoseconds out of range, wrong lengths) are
    // extensions
    assert_eq!(
        de::<Value>("d7ffee6b280000000000").unwrap(),
        Value::ext(Ext::new(-1, [0xee, 0x6b, 0x28, 0, 0, 0, 0, 0]))
    );
    assert_eq!(
        de::<Value>("d5ff0000").unwrap(),
        Value::ext(Ext::new(-1, [0, 0]))
    );
    assert!(de::<Timestamp>("d5ff0000").is_err());

    // std types work as well
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1363896240);
    assert_eq!(ser(&time), "d6ff514b67b0");
    assert_eq!(de::<std::time::SystemTime>("d6ff514b67b0").unwrap(), time);
}

#[test]
fn extensions() {
    let value = Ext::new(42, vec![1, 2, 3]);
    let hex = ser(&value);
    assert_eq!(hex, "c7032a010203");
    assert_eq!(de::<Ext>(&hex).unwrap(), value);
    assert_eq!(de::<Value>(&hex).unwrap(), Value::ext(value.clone()));
    // the fallback is the data
    assert_eq!(de::<Vec<u8>>(&hex).unwrap(), [1, 2, 3]);
    // extensions are retained by recordings
    let recording: deser::de::Recording = de(&hex).unwrap();
    assert_eq!(ser(&recording), hex);
    // negative types
    assert_eq!(ser(&Ext::new(-128, [0])), "d48000");
    assert_eq!(de::<Ext>("d48000").unwrap(), Ext::new(-128, [0]));
    // timestamps are extensions with their encoding
    assert_eq!(
        de::<Ext>("d6ff514b67b0").unwrap(),
        Ext::new(-1, [0x51, 0x4b, 0x67, 0xb0])
    );
    // other formats write the fallback
    assert_eq!(deser_json::to_string(&value).unwrap(), r#""AQID""#);
}

#[test]
fn datetimes() {
    // date-times are written as strings
    let value: Datetime = "2013-03-21T20:04:00Z".parse().unwrap();
    let hex = ser(&value);
    assert_eq!(hex, "b4323031332d30332d32315432303a30343a30305a");
    assert_eq!(de::<Datetime>(&hex).unwrap(), value);
    // and accept timestamps
    assert_eq!(de::<Datetime>("d6ff514b67b0").unwrap(), value);
    let value: Datetime = "20:04:00".parse().unwrap();
    assert_eq!(de::<Datetime>(&ser(&value)).unwrap(), value);
}

#[test]
fn uuids() {
    let value: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    let hex = ser(&value);
    assert_eq!(
        hex,
        "d92436376535353034342d313062312d343236662d393234372d626236383065356665306338"
    );
    assert_eq!(de::<Uuid>(&hex).unwrap(), value);
    // binary data is accepted too
    assert_eq!(
        de::<Uuid>("c41067e5504410b1426f9247bb680e5fe0c8").unwrap(),
        value
    );
}

#[test]
fn decimals() {
    let value: Decimal = "273.15".parse().unwrap();
    let hex = ser(&value);
    assert_eq!(hex, "a63237332e3135");
    assert_eq!(de::<Decimal>(&hex).unwrap(), value);
}

#[test]
fn big_integers() {
    // big integers are integers if they fit into 64 bits
    assert_eq!(ser(&BigInt::from(-1i64)), "ff");
    assert_eq!(ser(&BigInt::from(u64::MAX)), "cfffffffffffffffff");
    assert_eq!(ser(&BigInt::from(i64::MIN)), "d38000000000000000");
    for s in [
        "18446744073709551616",
        "-9223372036854775809",
        "-123456789012345678901234567890123456789012",
    ] {
        // otherwise strings
        let value: BigInt = s.parse().unwrap();
        let hex = ser(&value);
        assert_eq!(de::<String>(&hex).unwrap(), s);
        assert_eq!(de::<BigInt>(&hex).unwrap(), value, "{}", s);
    }
    assert_eq!(de::<BigInt>("ff").unwrap(), BigInt::from(-1i64));
}

#[test]
fn durations() {
    let value = Duration {
        seconds: 90,
        nanosecond: 0,
    };
    assert_eq!(ser(&value), "a75054314d333053"); // "PT1M30S"
    assert_eq!(de::<Duration>(&ser(&value)).unwrap(), value);
}
