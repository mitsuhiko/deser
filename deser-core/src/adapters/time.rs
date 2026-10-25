//! Adapters for durations and timestamps as numbers of a unit:
//! [`Seconds`], [`Millis`], [`Micros`] and [`Nanos`].
use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use core::marker::PhantomData;

use crate::State;
use crate::Text;
use crate::de::{Deserialize, Slot, default_atom};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ext::known::{Bridge, WellKnown, out_of_range};
use crate::ext::{Duration, ExtValue, Number, Timestamp};
use crate::ser::{Emit, Serialize};

const NANOS_PER_SECOND: i128 = 1_000_000_000;

mod sealed {
    pub trait Sealed {}
}

/// A duration or timestamp that the time unit adapters support.
///
/// This is implemented for [`ext::Duration`](Duration) and
/// [`ext::Timestamp`](Timestamp) and for all types that serialize as one
/// of them: [`core::time::Duration`] and [`SystemTime`], and
/// with the features of the same names `chrono::TimeDelta`,
/// `chrono::DateTime<Utc>`, `jiff::SignedDuration`, `jiff::Timestamp`,
/// `time::Duration` and `time::UtcDateTime`.  Date-times with an offset
/// or a time zone are not supported, as a number of seconds since the
/// epoch loses them.
///
/// This trait is sealed.
///
#[cfg_attr(feature = "std", doc = "[`SystemTime`]: std::time::SystemTime")]
#[cfg_attr(
    not(feature = "std"),
    doc = "[`SystemTime`]: https://doc.rust-lang.org/std/time/struct.SystemTime.html"
)]
pub trait TimeValue: Sized + Send + Sync + sealed::Sealed {
    /// Returns the value in nanoseconds (since the epoch for timestamps).
    #[doc(hidden)]
    fn __to_nanos(&self) -> Result<i128, Error>;

    /// Creates the value from nanoseconds.
    #[doc(hidden)]
    fn __from_nanos(nanos: i128) -> Result<Self, Error>;

    /// Creates the value from the extension value it serializes as.
    #[doc(hidden)]
    fn __from_ext(ext: &ExtValue) -> Result<Option<Self>, Error>;

    /// Returns `true` for timestamps, `false` for durations.
    ///
    /// Timestamps are truncated towards the past, durations towards zero.
    #[doc(hidden)]
    fn __is_instant() -> bool;
}

/// The well-known types the time unit adapters support.
pub(crate) trait KnownTime: WellKnown + Copy {
    const IS_INSTANT: bool;
    fn to_nanos(&self) -> i128;
    fn from_nanos(nanos: i128) -> Result<Self, Error>;
}

impl KnownTime for Duration {
    const IS_INSTANT: bool = false;

    fn to_nanos(&self) -> i128 {
        i128::from(self.seconds) * NANOS_PER_SECOND + i128::from(self.nanosecond)
    }

    fn from_nanos(nanos: i128) -> Result<Duration, Error> {
        // the seconds and nanoseconds have the same sign
        Ok(Duration {
            seconds: i64::try_from(nanos / NANOS_PER_SECOND)
                .map_err(|_| out_of_range("duration out of range"))?,
            nanosecond: (nanos % NANOS_PER_SECOND) as i32,
        })
    }
}

impl KnownTime for Timestamp {
    const IS_INSTANT: bool = true;

    fn to_nanos(&self) -> i128 {
        i128::from(self.seconds) * NANOS_PER_SECOND + i128::from(self.nanosecond)
    }

    fn from_nanos(nanos: i128) -> Result<Timestamp, Error> {
        // the nanoseconds are always positive
        Ok(Timestamp {
            seconds: i64::try_from(nanos.div_euclid(NANOS_PER_SECOND))
                .map_err(|_| out_of_range("timestamp out of range"))?,
            nanosecond: nanos.rem_euclid(NANOS_PER_SECOND) as u32,
        })
    }
}

/// Implements [`TimeValue`] for the well-known time types.
macro_rules! known_time_value {
    ($($ty:ty),*) => {
        $(
            impl sealed::Sealed for $ty {}

            impl TimeValue for $ty {
                fn __to_nanos(&self) -> Result<i128, Error> {
                    Ok(self.to_nanos())
                }

                fn __from_nanos(nanos: i128) -> Result<Self, Error> {
                    <$ty as KnownTime>::from_nanos(nanos)
                }

                fn __from_ext(ext: &ExtValue) -> Result<Option<Self>, Error> {
                    Ok(ext.downcast_ref::<$ty>().copied())
                }

                fn __is_instant() -> bool {
                    <$ty as KnownTime>::IS_INSTANT
                }
            }
        )*
    };
}

known_time_value!(Duration, Timestamp);

/// Implements [`TimeValue`] for types that are bridged onto a well-known
/// time type.
macro_rules! bridged_time_value {
    ($($ty:ty),* $(,)?) => {
        $(
            impl sealed::Sealed for $ty {}

            impl TimeValue for $ty {
                fn __to_nanos(&self) -> Result<i128, Error> {
                    Ok(Bridge::to_known(self)?.to_nanos())
                }

                fn __from_nanos(nanos: i128) -> Result<Self, Error> {
                    <$ty as Bridge>::from_known(KnownTime::from_nanos(nanos)?)
                }

                fn __from_ext(ext: &ExtValue) -> Result<Option<Self>, Error> {
                    match ext.downcast_ref::<<$ty as Bridge>::Known>() {
                        Some(known) => <$ty as Bridge>::from_known(*known).map(Some),
                        None => Ok(None),
                    }
                }

                fn __is_instant() -> bool {
                    <<$ty as Bridge>::Known as KnownTime>::IS_INSTANT
                }
            }
        )*
    };
}

bridged_time_value!(core::time::Duration);

#[cfg(feature = "std")]
bridged_time_value!(std::time::SystemTime);

#[cfg(feature = "chrono")]
bridged_time_value!(::chrono::TimeDelta, ::chrono::DateTime<::chrono::Utc>);

#[cfg(feature = "jiff")]
bridged_time_value!(::jiff::SignedDuration, ::jiff::Timestamp);

#[cfg(feature = "time")]
bridged_time_value!(::time::Duration, ::time::UtcDateTime);

/// How the time unit adapters write numbers: as `i64`, `f64` or `String`.
///
/// This trait is sealed.
pub trait UnitFormat: unit_format::Sealed + 'static {
    /// Returns the atom for a number of nanoseconds.
    #[doc(hidden)]
    fn __atom(nanos: i128, unit: i128, instant: bool) -> Atom<'static>;
}

mod unit_format {
    pub trait Sealed {}
    impl Sealed for i64 {}
    impl Sealed for f64 {}
    impl Sealed for alloc::string::String {}
}

/// Returns the number of units, truncated.
///
/// Timestamps are truncated towards the past and durations towards zero.
fn whole_units(nanos: i128, unit: i128, instant: bool) -> i128 {
    if instant {
        nanos.div_euclid(unit)
    } else {
        nanos / unit
    }
}

/// Returns the atom for an integer.
fn integer_atom(value: i128) -> Atom<'static> {
    match i64::try_from(value) {
        Ok(value) => Atom::I64(value),
        Err(_) => Atom::Ext(ExtValue::owned(value)),
    }
}

/// Writes whole units as integer, the fraction of a unit is truncated
/// (towards the past for timestamps, towards zero for durations).
impl UnitFormat for i64 {
    fn __atom(nanos: i128, unit: i128, instant: bool) -> Atom<'static> {
        integer_atom(whole_units(nanos, unit, instant))
    }
}

/// Writes units as float with the fraction of a unit.
impl UnitFormat for f64 {
    fn __atom(nanos: i128, unit: i128, instant: bool) -> Atom<'static> {
        let whole = whole_units(nanos, unit, instant);
        let fraction = nanos - whole * unit;
        Atom::F64(whole as f64 + fraction as f64 / unit as f64)
    }
}

/// Writes whole units as string, the fraction of a unit is truncated like
/// for `i64`.
impl UnitFormat for String {
    fn __atom(nanos: i128, unit: i128, instant: bool) -> Atom<'static> {
        Atom::Str(Text::owned(whole_units(nanos, unit, instant).to_string()))
    }
}

/// Returns the error for a time that is out of range.
#[cold]
fn time_out_of_range() -> Error {
    out_of_range("time out of range")
}

/// Converts a number of units into nanoseconds.
fn int_nanos(value: i128, unit_exp: u32) -> Result<i128, Error> {
    value
        .checked_mul(10i128.pow(unit_exp))
        .ok_or_else(time_out_of_range)
}

/// Converts the decimal text of a number of units into nanoseconds.
///
/// The number can have a fraction and an exponent (`-1.5`, `2.5e3`), it's
/// rounded to the nearest nanosecond (half way cases away from zero).
/// Returns `None` if the text is not a number.
fn decimal_nanos(text: &str, unit_exp: u32) -> Option<Result<i128, Error>> {
    let text = text.as_bytes();
    let (negative, text) = match text.first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (mantissa, exp) = match text.iter().position(|&x| x == b'e' || x == b'E') {
        Some(index) => {
            let exp = core::str::from_utf8(&text[index + 1..]).ok()?;
            (&text[..index], exp.parse::<i32>().ok()?)
        }
        None => (text, 0),
    };
    let (int_part, frac_part) = match mantissa.iter().position(|&x| x == b'.') {
        Some(index) => (&mantissa[..index], &mantissa[index + 1..]),
        None => (mantissa, &[][..]),
    };
    if (int_part.is_empty() && frac_part.is_empty())
        || !int_part.iter().chain(frac_part).all(u8::is_ascii_digit)
    {
        return None;
    }

    // the number of digits before the decimal point of the nanoseconds
    let point = int_part.len() as i64 + i64::from(exp) + i64::from(unit_exp);
    let mut value = 0i128;
    let mut round_up = false;
    let mut index = 0i64;
    for &digit in int_part.iter().chain(frac_part) {
        if index < point {
            value = match value
                .checked_mul(10)
                .and_then(|x| x.checked_add(i128::from(digit - b'0')))
            {
                Some(value) => value,
                None => return Some(Err(time_out_of_range())),
            };
        } else if index == point {
            // only the first digit after the point decides the rounding
            round_up = digit >= b'5';
            break;
        }
        index += 1;
    }
    // the zeros between the digits and the decimal point
    if value != 0 && point > index {
        let zeros = u32::try_from(point - index).unwrap_or(u32::MAX);
        value = match 10i128.checked_pow(zeros).and_then(|x| value.checked_mul(x)) {
            Some(value) => value,
            None => return Some(Err(time_out_of_range())),
        };
    }
    if round_up {
        value += 1;
    }
    Some(Ok(if negative { -value } else { value }))
}

/// A buffer on the stack for the text of a float.
struct FloatText {
    buf: [u8; 48],
    len: usize,
}

impl core::fmt::Write for FloatText {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        self.buf
            .get_mut(self.len..end)
            .ok_or(core::fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Converts a float number of units into nanoseconds.
///
/// The float is converted as the shortest decimal number that reads back
/// as the same float (`0.1` is exactly 100 milliseconds), rounded to the
/// nearest nanosecond.
fn float_nanos(value: f64, unit_exp: u32) -> Result<i128, Error> {
    if !value.is_finite() {
        return Err(time_out_of_range());
    }
    let mut text = FloatText {
        buf: [0; 48],
        len: 0,
    };
    // the exponent form is short also for very large and small values
    core::fmt::write(&mut text, format_args!("{:e}", value)).map_err(|_| time_out_of_range())?;
    let text = core::str::from_utf8(&text.buf[..text.len]).map_err(|_| time_out_of_range())?;
    decimal_nanos(text, unit_exp).unwrap_or_else(|| Err(time_out_of_range()))
}

/// Converts the text of a number of units into nanoseconds.
fn text_nanos(text: &str, unit_exp: u32, name: &str) -> Result<i128, Error> {
    decimal_nanos(text, unit_exp).unwrap_or_else(|| {
        Err(Error::new(
            ErrorKind::InvalidValue,
            alloc::format!("invalid number of {}", name),
        ))
    })
}

/// Deserializes an atom with a number of units into a value.
fn unit_atom<'de, T, A>(
    slot: &mut Slot<T, A>,
    atom: Atom,
    unit_exp: u32,
    name: &str,
    state: &mut State,
) -> Result<(), Error>
where
    T: TimeValue,
    A: Deserialize<'de, T>,
{
    let nanos = match atom {
        Atom::U64(value) => int_nanos(i128::from(value), unit_exp)?,
        Atom::I64(value) => int_nanos(i128::from(value), unit_exp)?,
        Atom::F64(value) => float_nanos(value, unit_exp)?,
        Atom::Str(ref text) | Atom::Lexical(ref text) => text_nanos(text, unit_exp, name)?,
        Atom::Ext(ref ext) => {
            if let Some(&value) = ext.downcast_ref::<i128>() {
                int_nanos(value, unit_exp)?
            } else if let Some(&value) = ext.downcast_ref::<u128>() {
                let value = i128::try_from(value).map_err(|_| time_out_of_range())?;
                int_nanos(value, unit_exp)?
            } else if let Some(number) = ext.downcast_value_ref::<Number>() {
                // exact numbers keep their text
                text_nanos(number.as_str(), unit_exp, name)?
            } else if let Some(value) = T::__from_ext(ext)? {
                // the value is written as the type it serializes as
                slot.set(value);
                return Ok(());
            } else {
                return default_atom(slot, atom, state);
            }
        }
        other => return default_atom(slot, other, state),
    };
    slot.set(T::__from_nanos(nanos)?);
    Ok(())
}

/// Implements the time unit adapters.
macro_rules! time_unit {
    ($(#[$meta:meta])* $name:ident, $unit_exp:expr, $expecting:literal;) => {
        $(#[$meta])*
        pub struct $name<F = i64>(PhantomData<fn() -> F>);

        impl<'de, T: TimeValue, F: UnitFormat> Deserialize<'de, T> for $name<F> {
            fn deserialize_atom(
                slot: &mut Slot<T, Self>,
                atom: Atom,
                state: &mut State,
            ) -> Result<(), Error> {
                unit_atom(slot, atom, $unit_exp, $expecting, state)
            }

            fn expecting() -> Cow<'static, str> {
                Cow::Borrowed($expecting)
            }

            slot_atom_into!(T);
        }

        impl<T: TimeValue, F: UnitFormat> Serialize<T> for $name<F> {
            fn serialize<'a>(value: &'a T, _state: &mut State) -> Result<Emit<'a>, Error> {
                Ok(Emit::Atom(F::__atom(
                    value.__to_nanos()?,
                    10i128.pow($unit_exp),
                    T::__is_instant(),
                )))
            }

            begin_without_finish!(T);
        }
    };
}

time_unit! {
    /// A duration or timestamp as number of seconds.
    ///
    /// Durations are written as their length and timestamps as the time
    /// since the Unix epoch (`1970-01-01T00:00:00Z`).  `F` is how the
    /// number is written:
    ///
    /// * `i64` (the default): an integer.  The fraction of a second is
    ///   truncated, towards the past for timestamps and towards zero for
    ///   durations (like the seconds of the types themselves).
    /// * `f64`: a float with the fraction of a second (which loses
    ///   precision for large values).
    /// * `String`: an integer as string, truncated like `i64`.
    ///
    /// When deserializing, all of these are accepted independent of `F`:
    /// integers, floats and strings that contain a number (also with a
    /// fraction or an exponent, like `1.5` or `2e3`), as well as the value
    /// in the form it serializes as without the adapter.  Fractions are
    /// rounded to the nearest nanosecond, floats are read as the shortest
    /// decimal number that is the same float (`0.1` seconds are exactly 100
    /// milliseconds).
    ///
    /// The supported types are listed in [`TimeValue`].  [`Millis`],
    /// [`Micros`] and [`Nanos`] are the same for smaller units.
    ///
    /// ```
    /// use std::time::{Duration, SystemTime};
    /// use deser::adapters::{Millis, Seconds};
    /// use deser::{Deserialize, Serialize};
    ///
    /// #[derive(Serialize, Deserialize)]
    /// pub struct Session {
    ///     #[deser(as = Seconds)]
    ///     created: SystemTime,
    ///     #[deser(as = Seconds<f64>)]
    ///     timeout: Duration,
    ///     #[deser(as = Millis)]
    ///     latency: Duration,
    /// }
    ///
    /// let input = r#"{"created":1700000000,"timeout":1.5,"latency":250}"#;
    /// let session: Session = deser_json::from_str(input).unwrap();
    /// assert_eq!(session.timeout, Duration::from_millis(1500));
    /// assert_eq!(session.latency, Duration::from_millis(250));
    /// assert_eq!(deser_json::to_string(&session).unwrap(), input);
    /// ```
    Seconds, 9, "seconds";
}

time_unit! {
    /// A duration or timestamp as number of milliseconds.
    ///
    /// This is like [`Seconds`] in milliseconds.
    ///
    /// ```
    /// use std::time::SystemTime;
    /// use deser::adapters::Millis;
    /// use deser::Serialize;
    ///
    /// #[derive(Serialize)]
    /// pub struct Event {
    ///     #[deser(as = Millis)]
    ///     at: SystemTime,
    /// }
    /// ```
    Millis, 6, "milliseconds";
}

time_unit! {
    /// A duration or timestamp as number of microseconds.
    ///
    /// This is like [`Seconds`] in microseconds.
    Micros, 3, "microseconds";
}

time_unit! {
    /// A duration or timestamp as number of nanoseconds.
    ///
    /// This is like [`Seconds`] in nanoseconds.  Timestamps in nanoseconds
    /// exceed `i64` after the year 2262, integers that do not fit are
    /// written as 128 bit integers.
    Nanos, 0, "nanoseconds";
}
