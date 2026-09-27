use std::fmt;
use std::str::FromStr;

use crate::error::Error;
use crate::event::Atom;
use crate::ext::Extension;
use crate::ext::datetime::write_fraction;
use crate::ext::known::{WellKnown, impl_well_known, invalid, out_of_range};

/// A signed, exact length of time.
///
/// This is a well-known extension type (see [`ext`](crate::ext)).  The
/// seconds and nanoseconds always have the same sign: `-1.5` seconds are
/// `seconds: -1` and `nanosecond: -500_000_000`.
///
/// The fallback is the ISO 8601 representation as string (`PT1H30M`,
/// `-PT0.5S`).  Only hours, minutes and seconds are used and accepted as
/// days, months and years are not of a fixed length.  Fractions are only
/// supported for seconds.
///
/// ```
/// use deser::ext::Duration;
///
/// let duration: Duration = "PT1H0.5S".parse().unwrap();
/// assert_eq!(duration, Duration { seconds: 3600, nanosecond: 500_000_000 });
/// assert_eq!(duration.to_string(), "PT1H0.5S");
/// ```
///
/// [`std::time::Duration`] serializes as [`Duration`], as do the duration
/// types of `jiff`, `chrono` and `time` with the respective features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration {
    /// The whole seconds.
    pub seconds: i64,
    /// The fraction of the second in nanoseconds (`-999_999_999` to
    /// `999_999_999`).
    pub nanosecond: i32,
}

impl Duration {
    /// Returns `true` if seconds and nanoseconds are in range and have the
    /// same sign.
    pub fn is_valid(&self) -> bool {
        self.nanosecond.unsigned_abs() < 1_000_000_000
            && !(self.seconds > 0 && self.nanosecond < 0)
            && !(self.seconds < 0 && self.nanosecond > 0)
    }

    /// Returns `true` if the duration is negative.
    pub fn is_negative(&self) -> bool {
        self.seconds < 0 || self.nanosecond < 0
    }

    /// Returns the duration in seconds as float.
    pub fn as_secs_f64(&self) -> f64 {
        self.seconds as f64 + f64::from(self.nanosecond) / 1e9
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_negative() {
            f.write_str("-")?;
        }
        f.write_str("PT")?;
        let seconds = self.seconds.unsigned_abs();
        let nanosecond = self.nanosecond.unsigned_abs();
        let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        if hours != 0 {
            write!(f, "{}H", hours)?;
        }
        if minutes != 0 {
            write!(f, "{}M", minutes)?;
        }
        if seconds != 0 || nanosecond != 0 || (hours == 0 && minutes == 0) {
            write!(f, "{}", seconds)?;
            write_fraction(f, nanosecond)?;
            f.write_str("S")?;
        }
        Ok(())
    }
}

impl FromStr for Duration {
    type Err = Error;

    fn from_str(s: &str) -> Result<Duration, Error> {
        let (negative, rest) = match s.as_bytes().first() {
            Some(b'-') => (true, &s[1..]),
            Some(b'+') => (false, &s[1..]),
            _ => (false, s),
        };
        let rest = rest
            .strip_prefix("PT")
            .or_else(|| rest.strip_prefix("pt"))
            .ok_or_else(|| invalid("invalid duration, expected ISO 8601 duration (PT...)"))?;
        if rest.is_empty() {
            return Err(invalid("invalid duration"));
        }

        let overflow = || out_of_range("duration out of range");
        let mut seconds: i64 = 0;
        let mut nanosecond: i64 = 0;
        // the units have to be in order: hours, minutes, seconds
        let mut last_unit = 0;
        let mut rest = rest.as_bytes();
        while !rest.is_empty() {
            let int_len = rest.iter().take_while(|x| x.is_ascii_digit()).count();
            if int_len == 0 {
                return Err(invalid("invalid duration"));
            }
            let value: i64 = std::str::from_utf8(&rest[..int_len])
                .unwrap()
                .parse()
                .map_err(|_| overflow())?;
            rest = &rest[int_len..];
            let mut fraction = None;
            if let Some(b'.' | b',') = rest.first() {
                let frac_len = rest[1..].iter().take_while(|x| x.is_ascii_digit()).count();
                if frac_len == 0 {
                    return Err(invalid("invalid duration"));
                }
                let mut nanos = 0i64;
                for (idx, &c) in rest[1..1 + frac_len].iter().enumerate() {
                    // digits beyond nanoseconds are truncated
                    if idx < 9 {
                        nanos = nanos * 10 + i64::from(c - b'0');
                    }
                }
                for _ in frac_len..9 {
                    nanos *= 10;
                }
                fraction = Some(nanos);
                rest = &rest[1 + frac_len..];
            }
            let (unit, factor) = match rest.first() {
                Some(b'H' | b'h') => (1, 3600),
                Some(b'M' | b'm') => (2, 60),
                Some(b'S' | b's') => (3, 1),
                _ => return Err(invalid("invalid duration unit")),
            };
            if unit <= last_unit || (fraction.is_some() && unit != 3) {
                return Err(invalid("invalid duration"));
            }
            last_unit = unit;
            rest = &rest[1..];
            seconds = value
                .checked_mul(factor)
                .and_then(|x| x.checked_add(seconds))
                .ok_or_else(overflow)?;
            nanosecond = fraction.unwrap_or(0);
        }

        if negative {
            seconds = -seconds;
            nanosecond = -nanosecond;
        }
        Ok(Duration {
            seconds,
            nanosecond: nanosecond as i32,
        })
    }
}

impl Extension for Duration {
    fn name(&self) -> &str {
        "duration"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(self.to_string().into())
    }
}

impl WellKnown for Duration {
    const EXPECTING: &'static str = "duration";

    /// Accepts durations, strings and numbers (seconds).
    fn from_atom(atom: &Atom) -> Result<Option<Duration>, Error> {
        Ok(Some(match *atom {
            Atom::Ext(ref ext) => match ext.downcast_ref::<Duration>() {
                Some(value) => *value,
                None => return Ok(None),
            },
            Atom::Str(ref value) => value.parse()?,
            Atom::U64(value) => Duration {
                seconds: i64::try_from(value).map_err(|_| out_of_range("duration out of range"))?,
                nanosecond: 0,
            },
            Atom::I64(value) => Duration {
                seconds: value,
                nanosecond: 0,
            },
            Atom::F64(value) => {
                if !value.is_finite() || value.abs() >= 9.2e18 {
                    return Err(out_of_range("duration out of range"));
                }
                let seconds = value.trunc();
                Duration {
                    seconds: seconds as i64,
                    nanosecond: ((value - seconds) * 1e9).round() as i32,
                }
            }
            _ => return Ok(None),
        }))
    }
}

impl_well_known!(Duration);

#[test]
fn test_duration_format() {
    let cases = [
        (0, 0, "PT0S"),
        (1, 500_000_000, "PT1.5S"),
        (-1, -500_000_000, "-PT1.5S"),
        (0, -1, "-PT0.000000001S"),
        (3600, 0, "PT1H"),
        (3661, 0, "PT1H1M1S"),
        (90, 0, "PT1M30S"),
        (i64::MAX, 999_999_999, "PT2562047788015215H30M7.999999999S"),
        (
            -i64::MAX,
            -999_999_999,
            "-PT2562047788015215H30M7.999999999S",
        ),
    ];
    for (seconds, nanosecond, expected) in cases {
        let duration = Duration {
            seconds,
            nanosecond,
        };
        assert_eq!(duration.to_string(), expected);
        assert_eq!(expected.parse::<Duration>().unwrap(), duration);
    }
    assert_eq!(
        "pt1m".parse::<Duration>().unwrap(),
        Duration {
            seconds: 60,
            nanosecond: 0
        }
    );
    for invalid in [
        "", "P", "PT", "P1D", "PT1S1M", "PT1.5M", "PT.5S", "PT1", "PTS", "1S",
    ] {
        assert!(invalid.parse::<Duration>().is_err(), "{}", invalid);
    }
}
