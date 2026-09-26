use std::fmt::Debug;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ext::{
    BigInt, Date, Datetime, Decimal, Duration, ExtValue, Offset, Time, Timestamp, Uuid,
};
use deser::ser::SerializeDriver;
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize};

fn deserialize<'a, 'de, T: Deserialize<'de>, E: Into<Event<'a>>>(event: E) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(event)?;
    }
    Ok(out.unwrap())
}

fn serialize(value: &dyn Serialize) -> Atom<'static> {
    let mut driver = SerializeDriver::new(value);
    let (event, _) = driver.next().unwrap().unwrap();
    match event {
        Event::Atom(atom) => atom.to_static(),
        other => panic!("unexpected event {:?}", other),
    }
}

/// Serializes the value, checks the fallback and deserializes it from both
/// the extension value and the fallback.
fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, fallback: &str) {
    let atom = serialize(&value);
    let Atom::Ext(ref ext) = atom else {
        panic!("expected extension value, got {:?}", atom);
    };
    assert_eq!(ext.fallback(), Atom::Str(fallback.into()));
    assert_eq!(deserialize::<T, _>(atom.clone()).unwrap(), value);
    assert_eq!(deserialize::<T, _>(fallback).unwrap(), value);
}

fn datetime(s: &str) -> Datetime {
    s.parse().unwrap()
}

#[test]
fn test_datetime() {
    for s in [
        "1979-05-27T07:32:00Z",
        "1979-05-27T00:32:00.999999-07:00",
        "1979-05-27T07:32:00",
        "1979-05-27",
        "07:32:00.5",
    ] {
        roundtrip(datetime(s), s);
    }
    assert_eq!(
        datetime("1979-05-27 07:32z").to_string(),
        "1979-05-27T07:32:00Z"
    );
    assert_eq!(
        datetime("1979-05-27T00:32:00-07:00"),
        Datetime {
            date: Some(Date {
                year: 1979,
                month: 5,
                day: 27
            }),
            time: Some(Time {
                hour: 0,
                minute: 32,
                second: 0,
                nanosecond: 0
            }),
            offset: Some(Offset::Custom { minutes: -420 }),
        }
    );
    for invalid in [
        "",
        "1979-02-29",
        "1979-05-27T",
        "1979-05-27 ",
        "24:00:00",
        "07:32:00Z",
        "1979-05-27T07:32:00+24:00",
        "1979-05-27T07:32:00.",
    ] {
        assert!(invalid.parse::<Datetime>().is_err(), "{}", invalid);
    }

    // timestamps convert to offset date-times
    let ts = Timestamp {
        seconds: 0,
        nanosecond: 0,
    };
    assert_eq!(
        deserialize::<Datetime, _>(Atom::Ext(ExtValue::borrowed(&ts))).unwrap(),
        datetime("1970-01-01T00:00:00Z")
    );
    // strings are passed on if the type is not known
    let value: String = deserialize(Atom::Ext(ExtValue::owned(datetime("07:32:00")))).unwrap();
    assert_eq!(value, "07:32:00");
}

#[test]
fn test_timestamp() {
    roundtrip(
        Timestamp {
            seconds: 296638320,
            nanosecond: 5,
        },
        "1979-05-27T07:32:00.000000005Z",
    );
    let expected = Timestamp {
        seconds: 296638320,
        nanosecond: 0,
    };
    // offset date-times, strings with offsets and numbers
    assert_eq!(
        deserialize::<Timestamp, _>(Atom::Ext(ExtValue::owned(datetime(
            "1979-05-27T00:32:00-07:00"
        ))))
        .unwrap(),
        expected
    );
    assert_eq!(
        deserialize::<Timestamp, _>("1979-05-27T00:32:00-07:00").unwrap(),
        expected
    );
    assert_eq!(deserialize::<Timestamp, _>(296638320u64).unwrap(), expected);
    assert_eq!(
        deserialize::<Timestamp, _>(-1.5f64).unwrap(),
        Timestamp {
            seconds: -2,
            nanosecond: 500_000_000
        }
    );
    // local date-times do not identify an instant
    let err =
        deserialize::<Timestamp, _>(Atom::Ext(ExtValue::owned(datetime("1979-05-27T00:32:00"))))
            .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected local date-time, expected offset date-time"
    );
}

#[test]
fn test_duration() {
    roundtrip(
        Duration {
            seconds: 5400,
            nanosecond: 0,
        },
        "PT1H30M",
    );
    roundtrip(
        Duration {
            seconds: -1,
            nanosecond: -5,
        },
        "-PT1.000000005S",
    );
    assert_eq!(
        deserialize::<Duration, _>(1.5f64).unwrap(),
        Duration {
            seconds: 1,
            nanosecond: 500_000_000
        }
    );
    assert_eq!(
        deserialize::<Duration, _>(-1.5f64).unwrap(),
        Duration {
            seconds: -1,
            nanosecond: -500_000_000
        }
    );
}

#[test]
fn test_uuid() {
    let uuid: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    roundtrip(uuid, "67e55044-10b1-426f-9247-bb680e5fe0c8");
    assert_eq!(deserialize::<Uuid, _>(&uuid.0[..]).unwrap(), uuid);
    assert!(deserialize::<Uuid, _>(&[1u8, 2, 3][..]).is_err());
    assert!(deserialize::<Uuid, _>(42u64).is_err());
}

#[test]
fn test_decimal() {
    roundtrip(Decimal::new("-12.50").unwrap(), "-12.50");
    assert_eq!(
        deserialize::<Decimal, _>(42u64).unwrap(),
        Decimal::new("42").unwrap()
    );
    assert_eq!(
        deserialize::<Decimal, _>(0.1f64).unwrap(),
        Decimal::new("0.1").unwrap()
    );
    assert_eq!(
        deserialize::<Decimal, _>(u128::MAX).unwrap().as_str(),
        u128::MAX.to_string()
    );
    assert!(deserialize::<Decimal, _>("1,5").is_err());
    assert!(deserialize::<Decimal, _>(f64::NAN).is_err());
}

#[test]
fn test_bigint() {
    let big: BigInt = "-123456789012345678901234567890123456789012"
        .parse()
        .unwrap();
    roundtrip(big.clone(), "-123456789012345678901234567890123456789012");
    assert_eq!(
        deserialize::<BigInt, _>(42u64).unwrap(),
        BigInt::from(42u64)
    );
    assert_eq!(
        deserialize::<BigInt, _>(i128::MIN).unwrap(),
        BigInt::from(i128::MIN)
    );
    // the fallback can be deserialized into strings
    let value: String = deserialize(Atom::Ext(ExtValue::owned(big))).unwrap();
    assert_eq!(value, "-123456789012345678901234567890123456789012");
    assert_eq!(BigInt::from(-5i64).into_atom(), Atom::I64(-5));
    assert!(matches!(
        BigInt::from(u128::MAX).into_atom(),
        Atom::Ext(ref ext) if ext.is::<u128>()
    ));
}

#[test]
fn test_std() {
    use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

    let duration = StdDuration::new(90, 5);
    let atom = serialize(&duration);
    assert_eq!(
        atom,
        Atom::Ext(ExtValue::owned(Duration {
            seconds: 90,
            nanosecond: 5
        }))
    );
    assert_eq!(deserialize::<StdDuration, _>(atom).unwrap(), duration);
    assert_eq!(
        deserialize::<StdDuration, _>("PT1M30.000000005S").unwrap(),
        duration
    );
    assert_eq!(
        deserialize::<StdDuration, _>("-PT1S").unwrap_err().kind(),
        ErrorKind::OutOfRange
    );

    for time in [
        UNIX_EPOCH + StdDuration::new(296638320, 5),
        UNIX_EPOCH - StdDuration::new(1, 500_000_000),
    ] {
        let atom = serialize(&time);
        assert_eq!(deserialize::<SystemTime, _>(atom).unwrap(), time);
    }
    let time: SystemTime = deserialize("1969-12-31T23:59:58.5Z").unwrap();
    assert_eq!(time, UNIX_EPOCH - StdDuration::new(1, 500_000_000));
}

#[cfg(feature = "jiff")]
#[test]
fn test_jiff() {
    use jiff::civil::{Date as JDate, DateTime, Time as JTime, date, time};
    use jiff::{SignedDuration, Timestamp as JTimestamp, Zoned};

    let check = |atom: Atom, expected: &str| match atom {
        Atom::Ext(ref ext) => assert_eq!(ext.fallback(), Atom::Str(expected.into())),
        other => panic!("unexpected {:?}", other),
    };

    let ts: JTimestamp = "1979-05-27T07:32:00.5Z".parse().unwrap();
    check(serialize(&ts), "1979-05-27T07:32:00.5Z");
    assert_eq!(deserialize::<JTimestamp, _>(serialize(&ts)).unwrap(), ts);
    assert_eq!(
        deserialize::<JTimestamp, _>("1979-05-27T00:32:00.5-07:00").unwrap(),
        ts
    );
    let before: JTimestamp = "1969-12-31T23:59:59.5Z".parse().unwrap();
    assert_eq!(
        deserialize::<JTimestamp, _>(serialize(&before)).unwrap(),
        before
    );

    let zoned: Zoned = "1979-05-27T00:32:00-07:00[-07:00]".parse().unwrap();
    check(serialize(&zoned), "1979-05-27T00:32:00-07:00");
    assert_eq!(deserialize::<Zoned, _>(serialize(&zoned)).unwrap(), zoned);
    // jiff's own formats are accepted as well
    let zoned: Zoned = deserialize("1979-05-27T00:32:00-07:00[-07:00]").unwrap();
    assert_eq!(zoned.offset().seconds(), -7 * 3600);

    let dt: DateTime = date(1979, 5, 27).at(7, 32, 0, 0);
    check(serialize(&dt), "1979-05-27T07:32:00");
    assert_eq!(deserialize::<DateTime, _>(serialize(&dt)).unwrap(), dt);
    check(serialize(&date(1979, 5, 27)), "1979-05-27");
    assert_eq!(
        deserialize::<JDate, _>("1979-05-27").unwrap(),
        date(1979, 5, 27)
    );
    check(serialize(&time(7, 32, 0, 0)), "07:32:00");
    assert_eq!(deserialize::<JTime, _>("07:32").unwrap(), time(7, 32, 0, 0));

    // kinds are checked
    assert!(deserialize::<JDate, _>("1979-05-27T07:32:00").is_err());
    assert!(deserialize::<DateTime, _>("1979-05-27T07:32:00Z").is_err());
    assert!(deserialize::<JTimestamp, _>("1979-05-27T07:32:00").is_err());
    // leap seconds are clamped
    assert_eq!(
        deserialize::<JTimestamp, _>("1990-12-31T23:59:60Z").unwrap(),
        "1990-12-31T23:59:59Z".parse::<JTimestamp>().unwrap()
    );
    // values that cannot be represented
    assert!(SerializeDriver::new(&date(-1, 1, 1)).next().is_err());

    let duration = SignedDuration::new(-90, -5);
    check(serialize(&duration), "-PT1M30.000000005S");
    assert_eq!(
        deserialize::<SignedDuration, _>(serialize(&duration)).unwrap(),
        duration
    );
}

#[cfg(feature = "chrono")]
#[test]
fn test_chrono() {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Utc};

    let utc: DateTime<Utc> = DateTime::from_timestamp(296638320, 500_000_000).unwrap();
    assert_eq!(
        serialize(&utc),
        Atom::Ext(ExtValue::owned(Timestamp {
            seconds: 296638320,
            nanosecond: 500_000_000
        }))
    );
    assert_eq!(
        deserialize::<DateTime<Utc>, _>(serialize(&utc)).unwrap(),
        utc
    );

    let fixed = DateTime::<FixedOffset>::parse_from_rfc3339("1979-05-27T00:32:00-07:00").unwrap();
    assert_eq!(
        serialize(&fixed),
        Atom::Ext(ExtValue::owned(datetime("1979-05-27T00:32:00-07:00")))
    );
    assert_eq!(
        deserialize::<DateTime<FixedOffset>, _>(serialize(&fixed)).unwrap(),
        fixed
    );

    let naive = NaiveDate::from_ymd_opt(1979, 5, 27)
        .unwrap()
        .and_hms_opt(7, 32, 0)
        .unwrap();
    assert_eq!(
        deserialize::<NaiveDateTime, _>(serialize(&naive)).unwrap(),
        naive
    );
    assert_eq!(
        deserialize::<NaiveDate, _>("1979-05-27").unwrap(),
        naive.date()
    );
    // leap seconds are supported by chrono
    let leap = NaiveTime::from_hms_nano_opt(23, 59, 59, 1_500_000_000).unwrap();
    assert_eq!(
        serialize(&leap),
        Atom::Ext(ExtValue::owned(datetime("23:59:60.5")))
    );
    assert_eq!(deserialize::<NaiveTime, _>(serialize(&leap)).unwrap(), leap);

    let delta = TimeDelta::new(-2, 500_000_000).unwrap();
    assert_eq!(
        serialize(&delta),
        Atom::Ext(ExtValue::owned(Duration {
            seconds: -1,
            nanosecond: -500_000_000
        }))
    );
    assert_eq!(
        deserialize::<TimeDelta, _>(serialize(&delta)).unwrap(),
        delta
    );
}

#[cfg(feature = "time")]
#[test]
fn test_time() {
    use time::macros::{date, datetime, time};
    use time::{OffsetDateTime, PrimitiveDateTime, UtcDateTime};

    let odt: OffsetDateTime = datetime!(1979-05-27 00:32:00.5 -7);
    assert_eq!(
        serialize(&odt),
        Atom::Ext(ExtValue::owned(datetime_str("1979-05-27T00:32:00.5-07:00")))
    );
    assert_eq!(
        deserialize::<OffsetDateTime, _>(serialize(&odt)).unwrap(),
        odt
    );

    let pdt: PrimitiveDateTime = datetime!(1979-05-27 07:32:00);
    assert_eq!(
        deserialize::<PrimitiveDateTime, _>(serialize(&pdt)).unwrap(),
        pdt
    );
    assert_eq!(
        deserialize::<time::Date, _>("1979-05-27").unwrap(),
        date!(1979 - 05 - 27)
    );
    assert_eq!(deserialize::<time::Time, _>("07:32").unwrap(), time!(07:32));

    let utc: UtcDateTime = datetime!(1979-05-27 07:32:00 UTC).to_utc();
    assert_eq!(
        serialize(&utc),
        Atom::Ext(ExtValue::owned(Timestamp {
            seconds: 296638320,
            nanosecond: 0
        }))
    );
    assert_eq!(deserialize::<UtcDateTime, _>(serialize(&utc)).unwrap(), utc);

    let duration = time::Duration::new(-1, -5);
    assert_eq!(
        deserialize::<time::Duration, _>(serialize(&duration)).unwrap(),
        duration
    );

    fn datetime_str(s: &str) -> Datetime {
        s.parse().unwrap()
    }
}

#[cfg(feature = "uuid")]
#[test]
fn test_uuid_crate() {
    let uuid = uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
    assert_eq!(
        serialize(&uuid),
        Atom::Ext(ExtValue::owned(Uuid(*uuid.as_bytes())))
    );
    assert_eq!(
        deserialize::<uuid::Uuid, _>(serialize(&uuid)).unwrap(),
        uuid
    );
    assert_eq!(
        deserialize::<uuid::Uuid, _>("{67e55044-10b1-426f-9247-bb680e5fe0c8}").unwrap(),
        uuid
    );
    assert_eq!(
        deserialize::<uuid::Uuid, _>(&uuid.as_bytes()[..]).unwrap(),
        uuid
    );
}

#[cfg(feature = "rust_decimal")]
#[test]
fn test_rust_decimal() {
    use std::str::FromStr;
    let value = rust_decimal::Decimal::from_str("-12.50").unwrap();
    assert_eq!(
        serialize(&value),
        Atom::Ext(ExtValue::owned(Decimal::new("-12.50").unwrap()))
    );
    let rv: rust_decimal::Decimal = deserialize(serialize(&value)).unwrap();
    assert_eq!(rv.to_string(), "-12.50");
    let rv: rust_decimal::Decimal = deserialize("1.5e3").unwrap();
    assert_eq!(rv.to_string(), "1500");
    // too many digits
    assert!(deserialize::<rust_decimal::Decimal, _>("0.1234567890123456789012345678901").is_err());
}

#[cfg(feature = "bigdecimal")]
#[test]
fn test_bigdecimal() {
    use std::str::FromStr;
    let value = bigdecimal::BigDecimal::from_str("-12.50").unwrap();
    assert_eq!(
        serialize(&value),
        Atom::Ext(ExtValue::owned(Decimal::new("-12.50").unwrap()))
    );
    let rv: bigdecimal::BigDecimal = deserialize(serialize(&value)).unwrap();
    assert_eq!(rv, value);
    let big = "123456789012345678901234567890.123456789012345678901234567890";
    let rv: bigdecimal::BigDecimal = deserialize(big).unwrap();
    assert_eq!(
        serialize(&rv),
        Atom::Ext(ExtValue::owned(Decimal::new(big).unwrap()))
    );
    let rv: bigdecimal::BigDecimal = deserialize("1.5e300").unwrap();
    assert_eq!(rv, bigdecimal::BigDecimal::from_str("1.5e300").unwrap());
}

#[cfg(feature = "num-bigint")]
#[test]
fn test_num_bigint() {
    use num_bigint::{BigInt as NumBigInt, BigUint};

    // small values are plain integers
    assert_eq!(serialize(&NumBigInt::from(-5)), Atom::I64(-5));
    assert_eq!(serialize(&BigUint::from(5u32)), Atom::U64(5));
    assert!(matches!(
        serialize(&NumBigInt::from(i128::MIN)),
        Atom::Ext(ref ext) if ext.is::<i128>()
    ));
    let big: NumBigInt = "-123456789012345678901234567890123456789012"
        .parse()
        .unwrap();
    assert_eq!(
        serialize(&big),
        Atom::Ext(ExtValue::owned(
            "-123456789012345678901234567890123456789012"
                .parse::<BigInt>()
                .unwrap()
        ))
    );
    assert_eq!(deserialize::<NumBigInt, _>(serialize(&big)).unwrap(), big);
    assert_eq!(
        deserialize::<NumBigInt, _>(-5i64).unwrap(),
        NumBigInt::from(-5)
    );
    assert_eq!(
        deserialize::<BigUint, _>(u128::MAX).unwrap(),
        BigUint::from(u128::MAX)
    );
    assert!(deserialize::<BigUint, _>(-1i64).is_err());
}

#[test]
fn test_number() {
    use deser::ext::Number;

    let number = Number::parse("0.10000000000000000001").unwrap();
    let atom = || Atom::Ext(ExtValue::borrowed_value::<Number>(&number));
    // floats get the value
    assert_eq!(deserialize::<f64, _>(atom()).unwrap(), 0.1);
    // decimals get the text
    assert_eq!(
        deserialize::<Decimal, _>(atom()).unwrap().as_str(),
        "0.10000000000000000001"
    );
    // big integers only accept integer numbers
    assert!(deserialize::<BigInt, _>(atom()).is_err());
    let big = Number::parse("123456789012345678901234567890123456789012").unwrap();
    assert_eq!(
        deserialize::<BigInt, _>(Atom::Ext(ExtValue::borrowed_value::<Number>(&big)))
            .unwrap()
            .to_string(),
        "123456789012345678901234567890123456789012"
    );
    // numbers themselves
    assert_eq!(deserialize::<Number, _>(atom()).unwrap(), number);
    assert_eq!(deserialize::<Number, _>(42u64).unwrap().as_str(), "42");
    assert_eq!(deserialize::<Number, _>("1e5").unwrap().value(), 100000.0);

    #[cfg(feature = "rust_decimal")]
    {
        let value: rust_decimal::Decimal = deserialize(atom()).unwrap();
        assert_eq!(value.to_string(), "0.10000000000000000001");
    }
    #[cfg(feature = "bigdecimal")]
    {
        let value: bigdecimal::BigDecimal = deserialize(atom()).unwrap();
        assert_eq!(value.to_string(), "0.10000000000000000001");
    }
    #[cfg(feature = "num-bigint")]
    {
        let value: num_bigint::BigInt =
            deserialize(Atom::Ext(ExtValue::borrowed_value::<Number>(&big))).unwrap();
        assert_eq!(
            value.to_string(),
            "123456789012345678901234567890123456789012"
        );
    }
}
