//! Checks extension parsers, formatting and numeric conversions directly.
#![no_main]

use std::fmt::Debug;
use std::str::FromStr;

use arbitrary::Unstructured;
use deser::de::DeserializeDriver;
use deser::ext::{BigInt, Datetime, Decimal, Duration, Number, Timestamp, Uuid};
use deser::{Atom, Deserialize, Error};
use libfuzzer_sys::fuzz_target;

fn roundtrip<T: ToString + FromStr + PartialEq + Debug>(value: T)
where
    T::Err: Debug,
{
    let text = value.to_string();
    assert_eq!(text.parse::<T>().unwrap(), value, "text: {text:?}");
}

fn deserialize<T: for<'de> Deserialize<'de>>(atom: Atom<'_>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(atom)?;
    }
    Ok(out.unwrap())
}

fn parts(value: &Decimal) {
    let (mantissa, exponent) = value.to_parts();
    let rebuilt = Decimal::from_parts(&mantissa, exponent);
    assert!(Decimal::new(rebuilt.as_str()).is_ok());
    assert_eq!(rebuilt.to_parts(), (mantissa, exponent));
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, data)) = data.split_first() else {
        return;
    };
    let mut u = Unstructured::new(data);
    match mode % 6 {
        0 => {
            let Ok(text) = std::str::from_utf8(data) else {
                return;
            };
            if let Ok(value) = text.parse::<Datetime>() {
                assert!(value.is_valid());
                roundtrip(value);
            }
            if let Ok(value) = text.parse::<Timestamp>() {
                roundtrip(value);
            }
            if let Ok(value) = text.parse::<Duration>() {
                assert!(value.is_valid());
                roundtrip(value);
            }
            if let Ok(value) = text.parse::<BigInt>() {
                roundtrip(value);
            }
            if let Ok(value) = text.parse::<Uuid>() {
                roundtrip(value);
            }
            if let Ok(value) = text.parse::<Decimal>() {
                parts(&value);
                roundtrip(value);
            }
            let _ = Number::parse(text);
        }
        1 => {
            let (Ok(seconds), Ok(mut nanosecond)) =
                (u.arbitrary::<i64>(), u.int_in_range(0..=999_999_999i32))
            else {
                return;
            };
            if seconds < 0 {
                nanosecond = -nanosecond;
            }
            roundtrip(Duration {
                seconds,
                nanosecond,
            });
        }
        2 => {
            let (Ok(seconds), Ok(nanosecond)) =
                (u.arbitrary::<i64>(), u.int_in_range(0..=999_999_999u32))
            else {
                return;
            };
            let value = Timestamp {
                seconds,
                nanosecond,
            };
            roundtrip(value);
            if let Some(datetime) = value.to_datetime() {
                assert!(datetime.is_valid());
                assert_eq!(Timestamp::try_from(datetime).unwrap(), value);
            }
        }
        3 => {
            let Ok(value) = u.arbitrary::<f64>() else {
                return;
            };
            if let Ok(duration) = deserialize::<Duration>(Atom::F64(value)) {
                assert!(
                    duration.is_valid(),
                    "float: {value:?}, duration: {duration:?}"
                );
                roundtrip(duration);
            }
            if let Ok(timestamp) = deserialize::<Timestamp>(Atom::F64(value)) {
                assert!(timestamp.nanosecond < 1_000_000_000);
                roundtrip(timestamp);
            }
            if let Ok(decimal) = Decimal::try_from(value) {
                assert_eq!(decimal.as_str().parse::<f64>().unwrap(), value);
                parts(&decimal);
            }
        }
        4 => {
            let Ok(negative) = u.arbitrary() else {
                return;
            };
            let value = BigInt {
                negative,
                magnitude: u.take_rest().to_vec(),
            };
            roundtrip(value);
        }
        _ => {
            let (Ok(mantissa), Ok(exponent)) = (u.arbitrary::<i128>(), u.arbitrary::<i64>()) else {
                return;
            };
            let mantissa = BigInt::from(mantissa);
            let value = Decimal::from_parts(&mantissa, exponent);
            assert!(Decimal::new(value.as_str()).is_ok());
            assert_eq!(value.to_parts(), (mantissa, exponent));
        }
    }
});
