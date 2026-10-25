use std::fmt::Debug;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deser::adapters::{Micros, Millis, Nanos, Seconds};
use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ext::ExtValue;
use deser::ser::SerializeDriver;
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

fn serialize<T: Serialize + ?Sized>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

/// Returns the events of a struct with a single field `v`.
fn field(value: impl Into<Event<'static>>) -> Vec<Event<'static>> {
    vec![Event::map_start(), "v".into(), value.into(), Event::MapEnd]
}

/// Returns the atom of the field `v` of the serialized struct.
fn serialized<T: Serialize>(value: &T) -> Atom<'static> {
    match &serialize(value)[..] {
        [Event::MapStart(_), _, Event::Atom(atom), Event::MapEnd] => atom.clone(),
        other => panic!("unexpected events {:?}", other),
    }
}

/// Checks that the value serializes to the atom and back.
fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T, atom: Atom<'static>) {
    assert_eq!(serialized(&value), atom);
    assert_eq!(deserialize::<T>(field(atom)).unwrap(), value);
}

macro_rules! wrapper {
    ($name:ident, $ty:ty, $adapter:ty) => {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct $name {
            #[deser(as = $adapter)]
            v: $ty,
        }
    };
}

wrapper!(DurSecs, Duration, Seconds);
wrapper!(DurSecsFloat, Duration, Seconds<f64>);
wrapper!(DurSecsString, Duration, Seconds<String>);
wrapper!(DurMillis, Duration, Millis);
wrapper!(DurMicros, Duration, Micros);
wrapper!(DurNanos, Duration, Nanos);
wrapper!(TsSecs, SystemTime, Seconds);
wrapper!(TsSecsFloat, SystemTime, Seconds<f64>);
wrapper!(TsMillis, SystemTime, Millis);
wrapper!(TsNanos, SystemTime, Nanos);
wrapper!(ExtSecs, deser::ext::Duration, Seconds);
wrapper!(ExtSecsFloat, deser::ext::Duration, Seconds<f64>);
wrapper!(ExtTsSecs, deser::ext::Timestamp, Seconds);

#[test]
fn test_durations() {
    let d = Duration::from_millis(1500);
    // integers truncate the fraction
    assert_eq!(serialized(&DurSecs { v: d }), Atom::I64(1));
    assert_eq!(serialized(&DurSecsString { v: d }), Atom::Str("1".into()));
    check(DurSecsFloat { v: d }, Atom::F64(1.5));
    check(DurMillis { v: d }, Atom::I64(1500));
    check(DurMicros { v: d }, Atom::I64(1_500_000));
    check(DurNanos { v: d }, Atom::I64(1_500_000_000));
    check(
        DurSecs {
            v: Duration::from_secs(3),
        },
        Atom::I64(3),
    );
    check(
        DurSecsString {
            v: Duration::from_secs(3),
        },
        Atom::Str("3".into()),
    );
}

#[test]
fn test_deserialize_forms() {
    let d = Duration::from_millis(1500);
    // every form is accepted independent of how the number is written
    for atom in [
        Atom::F64(1.5),
        Atom::F32(1.5),
        Atom::Str("1.5".into()),
        Atom::Lexical("1.5".into()),
    ] {
        assert_eq!(deserialize::<DurSecs>(field(atom)).unwrap().v, d);
    }
    for atom in [
        Atom::U64(1500),
        Atom::I64(1500),
        Atom::Str("1500".into()),
        Atom::Lexical("1500".into()),
        Atom::Ext(ExtValue::owned(1500u128)),
        Atom::Ext(ExtValue::owned(1500i128)),
    ] {
        assert_eq!(deserialize::<DurMillis>(field(atom)).unwrap().v, d);
    }
    // fractions are rounded to the nearest nanosecond
    assert_eq!(
        deserialize::<DurMillis>(field(Atom::F64(0.0000015)))
            .unwrap()
            .v,
        Duration::from_nanos(2)
    );
    assert_eq!(
        deserialize::<DurMillis>(field("0.0000014999")).unwrap().v,
        Duration::from_nanos(1)
    );
    // floats are the decimal number they read back from
    assert_eq!(
        deserialize::<DurSecs>(field(0.1f64)).unwrap().v,
        Duration::from_millis(100)
    );
    assert_eq!(
        deserialize::<TsSecs>(field(1_700_000_000.9f64)).unwrap().v,
        UNIX_EPOCH + Duration::from_millis(1_700_000_000_900)
    );
    // exponents and signs
    for (text, nanos) in [
        ("2e3", 2_000_000_000_000),
        ("2.5E-3", 2_500_000),
        ("+7", 7_000_000_000),
        ("007.50", 7_500_000_000),
        (".5", 500_000_000),
        ("5.", 5_000_000_000),
        ("1.0000000005", 1_000_000_001),
        ("1.0000000004999", 1_000_000_000),
        ("1e-30", 0),
        ("0e999999", 0),
    ] {
        let d = deserialize::<DurSecs>(field(text)).unwrap().v;
        assert_eq!(d, Duration::from_nanos(nanos), "{text:?}");
    }
    assert_eq!(
        deserialize::<ExtSecs>(field("-1.5")).unwrap().v,
        deser::ext::Duration {
            seconds: -1,
            nanosecond: -500_000_000,
        }
    );
    assert_eq!(
        deserialize::<ExtSecs>(field(-0.0000000005f64)).unwrap().v,
        deser::ext::Duration {
            seconds: 0,
            nanosecond: -1,
        }
    );

    // the form the value serializes as without the adapter
    let native = Atom::Ext(ExtValue::owned(deser::ext::Duration {
        seconds: 1,
        nanosecond: 500_000_000,
    }));
    assert_eq!(deserialize::<DurMillis>(field(native)).unwrap().v, d);
}

#[test]
fn test_errors() {
    let err = deserialize::<DurSecs>(field(true)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
    assert_eq!(err.message(), "unexpected bool, expected seconds");
    let err = deserialize::<DurMillis>(field("soon")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
    assert_eq!(err.message(), "invalid number of milliseconds");
    let err = deserialize::<DurSecs>(field(-1i64)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    assert_eq!(err.message(), "duration cannot be negative");
    let err = deserialize::<DurSecs>(field(f64::NAN)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    for text in ["inf", "", "-", "1.2.3", "1e", "e3", ".", "0x10", "1_000"] {
        let err = deserialize::<DurSecs>(field(text)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue, "{text:?}");
    }
    let err = deserialize::<DurSecs>(field("1e30")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    let err = deserialize::<DurSecs>(field("1e2147483647")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    let err = deserialize::<DurSecs>(field(f64::MAX)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    let err = deserialize::<DurSecs>(field(u64::MAX)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    let err = deserialize::<DurNanos>(field(Atom::Ext(ExtValue::owned(u128::MAX)))).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
}

#[test]
fn test_timestamps() {
    let ts = UNIX_EPOCH + Duration::from_millis(1_700_000_000_500);
    assert_eq!(serialized(&TsSecs { v: ts }), Atom::I64(1_700_000_000));
    check(TsSecsFloat { v: ts }, Atom::F64(1_700_000_000.5));
    check(TsMillis { v: ts }, Atom::I64(1_700_000_000_500));
    check(TsNanos { v: ts }, Atom::I64(1_700_000_000_500_000_000));

    // before the epoch timestamps are truncated towards the past
    let ts = UNIX_EPOCH - Duration::from_millis(1500);
    assert_eq!(serialized(&TsSecs { v: ts }), Atom::I64(-2));
    check(TsSecsFloat { v: ts }, Atom::F64(-1.5));
    check(TsMillis { v: ts }, Atom::I64(-1500));
    check(
        ExtTsSecs {
            v: deser::ext::Timestamp {
                seconds: -2,
                nanosecond: 0,
            },
        },
        Atom::I64(-2),
    );
    assert_eq!(
        serialized(&ExtTsSecs {
            v: deser::ext::Timestamp {
                seconds: -2,
                nanosecond: 500_000_000,
            },
        }),
        Atom::I64(-2)
    );

    // and durations towards zero
    let d = deser::ext::Duration {
        seconds: -1,
        nanosecond: -500_000_000,
    };
    assert_eq!(serialized(&ExtSecs { v: d }), Atom::I64(-1));
    check(ExtSecsFloat { v: d }, Atom::F64(-1.5));
}

#[test]
fn test_large_values() {
    // nanoseconds after 2262 do not fit an `i64`
    let ts = deser::ext::Timestamp {
        seconds: 10_000_000_000,
        nanosecond: 1,
    };
    wrapper!(ExtTsNanos, deser::ext::Timestamp, Nanos);
    check(
        ExtTsNanos { v: ts },
        Atom::Ext(ExtValue::owned(10_000_000_000_000_000_001i128)),
    );

    let d = deser::ext::Duration {
        seconds: i64::MAX,
        nanosecond: 999_999_999,
    };
    wrapper!(ExtNanos, deser::ext::Duration, Nanos);
    check(
        ExtNanos { v: d },
        Atom::Ext(ExtValue::owned(
            i128::from(i64::MAX) * 1_000_000_000 + 999_999_999,
        )),
    );

    // a duration that does not fit the seconds
    let err = deserialize::<ExtNanos>(field(Atom::Ext(ExtValue::owned(i128::MAX)))).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
}

#[test]
fn test_optional() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Session {
        #[deser(as = Option<Millis>)]
        expires: Option<SystemTime>,
        #[deser(as = Vec<Seconds>)]
        intervals: Vec<Duration>,
    }

    let session: Session = deserialize(vec![
        Event::map_start(),
        "intervals".into(),
        Event::seq_start(),
        1u64.into(),
        2.5f64.into(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        session,
        Session {
            expires: None,
            intervals: vec![Duration::from_secs(1), Duration::from_millis(2500)],
        }
    );
}

#[cfg(feature = "chrono")]
#[test]
fn test_chrono() {
    wrapper!(Delta, chrono::TimeDelta, Millis);
    wrapper!(Utc, chrono::DateTime<chrono::Utc>, Seconds);
    check(
        Delta {
            v: chrono::TimeDelta::milliseconds(-1500),
        },
        Atom::I64(-1500),
    );
    check(
        Utc {
            v: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        },
        Atom::I64(1_700_000_000),
    );
}

#[cfg(feature = "jiff")]
#[test]
fn test_jiff() {
    wrapper!(Signed, jiff::SignedDuration, Micros);
    wrapper!(Ts, jiff::Timestamp, Millis);
    check(
        Signed {
            v: jiff::SignedDuration::from_micros(-7),
        },
        Atom::I64(-7),
    );
    check(
        Ts {
            v: jiff::Timestamp::from_millisecond(1_700_000_000_123).unwrap(),
        },
        Atom::I64(1_700_000_000_123),
    );
}

#[cfg(feature = "time")]
#[test]
fn test_time() {
    wrapper!(Dur, time::Duration, Seconds<f64>);
    wrapper!(Utc, time::UtcDateTime, Seconds);
    check(
        Dur {
            v: time::Duration::milliseconds(2500),
        },
        Atom::F64(2.5),
    );
    check(
        Utc {
            v: time::UtcDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
        },
        Atom::I64(1_700_000_000),
    );
}
