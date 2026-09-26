use std::fmt;
use std::str::FromStr;

use crate::descriptors::{Descriptor, NamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ext::Extension;
use crate::ext::known::{WellKnown, impl_well_known, invalid, out_of_range};

/// A calendar date (`1979-05-27`).
///
/// See [`Datetime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// The year (`0` to `9999`).
    pub year: u16,
    /// The month (`1` to `12`).
    pub month: u8,
    /// The day of the month (`1` to `31`).
    pub day: u8,
}

/// A time of the day (`07:32:00.999`).
///
/// See [`Datetime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    /// The hour (`0` to `23`).
    pub hour: u8,
    /// The minute (`0` to `59`).
    pub minute: u8,
    /// The second (`0` to `60`, `60` for leap seconds).
    pub second: u8,
    /// The fraction of the second in nanoseconds.
    pub nanosecond: u32,
}

/// The offset of an offset date-time.
///
/// See [`Datetime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Offset {
    /// UTC, written as `Z`.
    Z,
    /// An offset from UTC in minutes (`-07:00` is `-420`).
    Custom {
        /// The offset in minutes.
        minutes: i16,
    },
}

/// A date, a time of the day, or both with an optional offset.
///
/// This is a well-known extension type (see [`ext`](crate::ext)) for the
/// date and time values of [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339):
///
/// | Kind                                      | `date`  | `time`  | `offset` |
/// |-------------------------------------------|---------|---------|----------|
/// | offset date-time (`1979-05-27T07:32:00Z`) | `Some`  | `Some`  | `Some`   |
/// | local date-time (`1979-05-27T07:32:00`)   | `Some`  | `Some`  | `None`   |
/// | local date (`1979-05-27`)                 | `Some`  | `None`  | `None`   |
/// | local time (`07:32:00`)                   | `None`  | `Some`  | `None`   |
///
/// The fallback is the RFC 3339 representation as string.
///
/// Parsing is lenient: the separator between date and time can be `T`, `t`
/// or a space, `Z` can be lowercase and seconds can be omitted.  Formatting
/// always produces RFC 3339.  Digits of the fraction beyond nanoseconds are
/// truncated.
///
/// ```
/// use deser::ext::Datetime;
///
/// let dt: Datetime = "1979-05-27 07:32:00.5-07:00".parse().unwrap();
/// assert_eq!(dt.to_string(), "1979-05-27T07:32:00.5-07:00");
/// ```
///
/// With the `jiff`, `chrono` and `time` features the date and time types of
/// these crates serialize as [`Datetime`] (or [`Timestamp`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Datetime {
    /// The date.  `None` for local times.
    pub date: Option<Date>,
    /// The time.  `None` for local dates.
    pub time: Option<Time>,
    /// The offset.  Only set for offset date-times.
    pub offset: Option<Offset>,
}

/// An instant in time.
///
/// This is a well-known extension type (see [`ext`](crate::ext)) for
/// points in time independent of a time zone, such as Unix timestamps.  It
/// holds the time since the Unix epoch (`1970-01-01T00:00:00Z`), ignoring
/// leap seconds.  The nanoseconds are always positive: `-0.5` seconds are
/// `seconds: -1` and `nanosecond: 500_000_000`.
///
/// The fallback is the RFC 3339 representation in UTC as string.  Instants
/// outside of the years 0 to 9999 are written with an expanded year as in
/// ISO 8601 (`+10000-01-01T00:00:00Z`).
///
/// ```
/// use deser::ext::Timestamp;
///
/// let ts = Timestamp { seconds: 296638320, nanosecond: 0 };
/// assert_eq!(ts.to_string(), "1979-05-27T07:32:00Z");
/// assert_eq!("1979-05-27T00:32:00-07:00".parse::<Timestamp>().unwrap(), ts);
/// ```
///
/// [`std::time::SystemTime`] serializes as [`Timestamp`], as do the
/// timestamp types of `jiff`, `chrono` and `time` with the respective
/// features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    /// The seconds since the Unix epoch.
    pub seconds: i64,
    /// The fraction of the second in nanoseconds (`0` to `999_999_999`).
    pub nanosecond: u32,
}

impl Date {
    /// Returns `true` if this is a valid calendar date.
    pub fn is_valid(&self) -> bool {
        self.year <= 9999
            && (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= days_in_month(i64::from(self.year), self.month)
    }
}

impl Time {
    /// Returns `true` if this is a valid time.
    pub fn is_valid(&self) -> bool {
        self.hour <= 23 && self.minute <= 59 && self.second <= 60 && self.nanosecond < 1_000_000_000
    }
}

impl Offset {
    /// Returns the offset in minutes.
    pub fn minutes(&self) -> i16 {
        match *self {
            Offset::Z => 0,
            Offset::Custom { minutes } => minutes,
        }
    }

    /// Returns `true` if the offset is less than 24 hours.
    pub fn is_valid(&self) -> bool {
        self.minutes().unsigned_abs() < 24 * 60
    }
}

impl Datetime {
    /// Returns `true` if the value is a valid date-time.
    ///
    /// The fields are public, so a [`Datetime`] can be constructed that
    /// is invalid (for instance an offset without date and time or February
    /// 30th).
    pub fn is_valid(&self) -> bool {
        match (self.date, self.time, self.offset) {
            (None, None, _) => false,
            (date, time, Some(offset)) => {
                date.is_some_and(|x| x.is_valid())
                    && time.is_some_and(|x| x.is_valid())
                    && offset.is_valid()
            }
            (date, time, None) => {
                date.is_none_or(|x| x.is_valid()) && time.is_none_or(|x| x.is_valid())
            }
        }
    }

    /// Returns a human readable name for the kind of date-time.
    ///
    /// This is one of `"offset date-time"`, `"local date-time"`,
    /// `"local date"` and `"local time"`.
    pub fn kind(&self) -> &'static str {
        match (self.date, self.time, self.offset) {
            (Some(_), Some(_), Some(_)) => "offset date-time",
            (Some(_), Some(_), None) => "local date-time",
            (Some(_), None, _) => "local date",
            (None, Some(_), _) => "local time",
            (None, None, _) => "invalid date-time",
        }
    }

    /// Creates an error for a date-time of the wrong kind.
    pub(crate) fn unexpected_kind(&self, expected: &str) -> Error {
        invalid(format!("unexpected {}, expected {}", self.kind(), expected))
    }

    /// Returns the date if this is a local date.
    #[allow(dead_code)]
    pub(crate) fn expect_local_date(&self) -> Result<Date, Error> {
        match (self.date, self.time) {
            (Some(date), None) => Ok(date),
            _ => Err(self.unexpected_kind("local date")),
        }
    }

    /// Returns the time if this is a local time.
    #[allow(dead_code)]
    pub(crate) fn expect_local_time(&self) -> Result<Time, Error> {
        match (self.date, self.time) {
            (None, Some(time)) => Ok(time),
            _ => Err(self.unexpected_kind("local time")),
        }
    }

    /// Returns date and time if this is a local date-time.
    #[allow(dead_code)]
    pub(crate) fn expect_local_datetime(&self) -> Result<(Date, Time), Error> {
        match (self.date, self.time, self.offset) {
            (Some(date), Some(time), None) => Ok((date, time)),
            _ => Err(self.unexpected_kind("local date-time")),
        }
    }

    /// Returns date, time and offset if this is an offset date-time.
    pub(crate) fn expect_offset_datetime(&self) -> Result<(Date, Time, Offset), Error> {
        match (self.date, self.time, self.offset) {
            (Some(date), Some(time), Some(offset)) => Ok((date, time, offset)),
            _ => Err(self.unexpected_kind("offset date-time")),
        }
    }
}

impl Timestamp {
    /// Creates a timestamp from seconds and nanoseconds that can be out of
    /// range or negative.
    pub(crate) fn normalized(seconds: i64, nanosecond: i64) -> Option<Timestamp> {
        let seconds = seconds.checked_add(nanosecond.div_euclid(1_000_000_000))?;
        Some(Timestamp {
            seconds,
            nanosecond: nanosecond.rem_euclid(1_000_000_000) as u32,
        })
    }

    /// Returns the time since the epoch in seconds as float.
    pub fn as_secs_f64(&self) -> f64 {
        self.seconds as f64 + f64::from(self.nanosecond) / 1e9
    }

    /// Converts the timestamp into an offset date-time in UTC.
    ///
    /// Returns `None` if the year is outside of `0` to `9999`.
    pub fn to_datetime(&self) -> Option<Datetime> {
        let (year, date, time) = self.to_civil();
        Some(Datetime {
            date: Some(Date {
                year: u16::try_from(year).ok().filter(|&x| x <= 9999)?,
                month: date.0,
                day: date.1,
            }),
            time: Some(time),
            offset: Some(Offset::Z),
        })
    }

    /// Converts the timestamp into the year, month and day and time in UTC.
    fn to_civil(self) -> (i64, (u8, u8), Time) {
        let days = self.seconds.div_euclid(86400);
        let secs = self.seconds.rem_euclid(86400);
        let (year, month, day) = civil_from_days(days);
        (
            year,
            (month, day),
            Time {
                hour: (secs / 3600) as u8,
                minute: (secs / 60 % 60) as u8,
                second: (secs % 60) as u8,
                nanosecond: self.nanosecond,
            },
        )
    }

    /// Creates a timestamp from a civil date-time with an offset.
    fn from_civil(year: i64, date: Date, time: Time, offset: Offset) -> Option<Timestamp> {
        let days = days_from_civil(year, date.month, date.day);
        let seconds = days.checked_mul(86400)?.checked_add(
            i64::from(time.hour) * 3600
                    + i64::from(time.minute) * 60
                    // leap seconds are clamped
                    + i64::from(time.second.min(59))
                - i64::from(offset.minutes()) * 60,
        )?;
        Some(Timestamp {
            seconds,
            nanosecond: time.nanosecond,
        })
    }
}

impl TryFrom<Datetime> for Timestamp {
    type Error = Error;

    /// Converts an offset date-time into a timestamp.
    ///
    /// Leap seconds are clamped to the previous second.
    fn try_from(value: Datetime) -> Result<Timestamp, Error> {
        let (date, time, offset) = value.expect_offset_datetime()?;
        if !value.is_valid() {
            return Err(invalid("invalid date-time"));
        }
        Timestamp::from_civil(i64::from(date.year), date, time, offset)
            .ok_or_else(|| out_of_range("date-time out of range"))
    }
}

impl TryFrom<Timestamp> for Datetime {
    type Error = Error;

    /// Converts a timestamp into an offset date-time in UTC.
    fn try_from(value: Timestamp) -> Result<Datetime, Error> {
        value
            .to_datetime()
            .ok_or_else(|| out_of_range("timestamp out of range for date-time"))
    }
}

impl From<Date> for Datetime {
    fn from(date: Date) -> Datetime {
        Datetime {
            date: Some(date),
            time: None,
            offset: None,
        }
    }
}

impl From<Time> for Datetime {
    fn from(time: Time) -> Datetime {
        Datetime {
            date: None,
            time: Some(time),
            offset: None,
        }
    }
}

pub(crate) fn is_leap_year(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

pub(crate) fn days_in_month(year: i64, month: u8) -> u8 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Returns the days since the Unix epoch for a proleptic Gregorian date.
///
/// See <http://howardhinnant.github.io/date_algorithms.html>.
pub(crate) fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let (month, day) = (i64::from(month), i64::from(day));
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Returns the proleptic Gregorian date for days since the Unix epoch.
pub(crate) fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)?;
        write_fraction(f, self.nanosecond)
    }
}

/// Writes the fraction of a second without trailing zeros.
pub(crate) fn write_fraction(f: &mut fmt::Formatter<'_>, nanosecond: u32) -> fmt::Result {
    if nanosecond != 0 {
        let mut digits = 9;
        let mut value = nanosecond;
        while value.is_multiple_of(10) {
            value /= 10;
            digits -= 1;
        }
        write!(f, ".{:0width$}", value, width = digits)?;
    }
    Ok(())
}

impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Offset::Z => f.write_str("Z"),
            Offset::Custom { minutes } => {
                let sign = if minutes < 0 { '-' } else { '+' };
                let minutes = minutes.unsigned_abs();
                write!(f, "{}{:02}:{:02}", sign, minutes / 60, minutes % 60)
            }
        }
    }
}

impl fmt::Display for Datetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref date) = self.date {
            write!(f, "{}", date)?;
            if self.time.is_some() {
                f.write_str("T")?;
            }
        }
        if let Some(ref time) = self.time {
            write!(f, "{}", time)?;
        }
        if let Some(ref offset) = self.offset {
            write!(f, "{}", offset)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Datetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Datetime({})", self)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, (month, day), time) = self.to_civil();
        if (0..=9999).contains(&year) {
            write!(f, "{:04}", year)?;
        } else {
            // expanded year representation of ISO 8601
            write!(
                f,
                "{}{:06}",
                if year < 0 { '-' } else { '+' },
                year.unsigned_abs()
            )?;
        }
        write!(f, "-{:02}-{:02}T{}Z", month, day, time)
    }
}

impl FromStr for Datetime {
    type Err = Error;

    fn from_str(s: &str) -> Result<Datetime, Error> {
        let (year, dt) = parse_datetime(s, false)?;
        debug_assert!(year.is_none());
        Ok(dt)
    }
}

impl FromStr for Timestamp {
    type Err = Error;

    /// Parses an offset date-time.
    ///
    /// Years outside of 0 to 9999 can be given in the expanded
    /// representation of ISO 8601 (a sign and at least six digits).
    fn from_str(s: &str) -> Result<Timestamp, Error> {
        match parse_datetime(s, true)? {
            (Some(year), dt) => {
                let (date, time, offset) = dt.expect_offset_datetime()?;
                Timestamp::from_civil(year, date, time, offset)
                    .ok_or_else(|| out_of_range("timestamp out of range"))
            }
            (None, dt) => Timestamp::try_from(dt),
        }
    }
}

/// Parses a date-time.
///
/// If `expanded_years` is set, years with a sign and at least six digits
/// are accepted.  These are returned separately, the date of the returned
/// value then has the year `2000`.
fn parse_datetime(s: &str, expanded_years: bool) -> Result<(Option<i64>, Datetime), Error> {
    let bytes = s.as_bytes();
    let mut pos = 0;
    let mut rv = Datetime {
        date: None,
        time: None,
        offset: None,
    };
    let mut expanded_year = None;

    let has_date = if expanded_years && matches!(bytes.first(), Some(b'+' | b'-')) {
        let digits = bytes[1..].iter().take_while(|x| x.is_ascii_digit()).count();
        if digits < 6 {
            return Err(invalid("invalid expanded year"));
        }
        let year: i64 = s[1..1 + digits]
            .parse()
            .map_err(|_| out_of_range("year out of range"))?;
        expanded_year = Some(if bytes[0] == b'-' { -year } else { year });
        pos = 1 + digits;
        true
    } else {
        bytes.get(4) == Some(&b'-')
    };

    if has_date {
        let year = match expanded_year {
            Some(_) => 2000,
            None => digits(bytes, &mut pos, 4)? as u16,
        };
        expect(bytes, &mut pos, b'-')?;
        let month = digits(bytes, &mut pos, 2)? as u8;
        expect(bytes, &mut pos, b'-')?;
        let day = digits(bytes, &mut pos, 2)? as u8;
        let date = Date { year, month, day };
        let valid_day = match expanded_year {
            Some(year) => {
                (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)
            }
            None => date.is_valid(),
        };
        if !valid_day {
            return Err(invalid("invalid date"));
        }
        rv.date = Some(date);
        match bytes.get(pos) {
            Some(b'T' | b't' | b' ') => pos += 1,
            None => return Ok((expanded_year, rv)),
            _ => return Err(invalid("invalid date-time")),
        }
    }

    let mut time = Time {
        hour: digits(bytes, &mut pos, 2)? as u8,
        minute: {
            expect(bytes, &mut pos, b':')?;
            digits(bytes, &mut pos, 2)? as u8
        },
        second: 0,
        nanosecond: 0,
    };
    if bytes.get(pos) == Some(&b':') {
        pos += 1;
        time.second = digits(bytes, &mut pos, 2)? as u8;
        if bytes.get(pos) == Some(&b'.') {
            pos += 1;
            let start = pos;
            let mut nanos = 0u32;
            while let Some(&c) = bytes.get(pos).filter(|x| x.is_ascii_digit()) {
                // digits beyond nanoseconds are truncated
                if pos - start < 9 {
                    nanos = nanos * 10 + u32::from(c - b'0');
                }
                pos += 1;
            }
            if pos == start {
                return Err(invalid("expected digits after decimal point"));
            }
            for _ in (pos - start)..9 {
                nanos *= 10;
            }
            time.nanosecond = nanos;
        }
    }
    if !time.is_valid() {
        return Err(invalid("invalid time"));
    }
    rv.time = Some(time);

    if rv.date.is_some() {
        match bytes.get(pos) {
            Some(b'Z' | b'z') => {
                pos += 1;
                rv.offset = Some(Offset::Z);
            }
            Some(&sign @ (b'+' | b'-')) => {
                pos += 1;
                let hours = digits(bytes, &mut pos, 2)?;
                expect(bytes, &mut pos, b':')?;
                let minutes = digits(bytes, &mut pos, 2)?;
                if hours > 23 || minutes > 59 {
                    return Err(invalid("invalid offset"));
                }
                let minutes = (hours * 60 + minutes) as i16;
                rv.offset = Some(Offset::Custom {
                    minutes: if sign == b'-' { -minutes } else { minutes },
                });
            }
            _ => {}
        }
    }

    if pos != bytes.len() {
        return Err(invalid("unexpected characters after date-time"));
    }
    Ok((expanded_year, rv))
}

fn digits(bytes: &[u8], pos: &mut usize, n: usize) -> Result<u32, Error> {
    let mut rv = 0;
    for _ in 0..n {
        match bytes.get(*pos) {
            Some(&c) if c.is_ascii_digit() => rv = rv * 10 + u32::from(c - b'0'),
            _ => return Err(invalid("invalid date-time")),
        }
        *pos += 1;
    }
    Ok(rv)
}

fn expect(bytes: &[u8], pos: &mut usize, c: u8) -> Result<(), Error> {
    if bytes.get(*pos) == Some(&c) {
        *pos += 1;
        Ok(())
    } else {
        Err(invalid("invalid date-time"))
    }
}

static DATETIME_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Datetime" };
static TIMESTAMP_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Timestamp" };

impl Extension for Datetime {
    fn name(&self) -> &str {
        "datetime"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(self.to_string().into())
    }
}

impl WellKnown for Datetime {
    const EXPECTING: &'static str = "datetime";

    fn descriptor() -> &'static dyn Descriptor {
        &DATETIME_DESCRIPTOR
    }

    /// Accepts date-times, timestamps and strings.
    fn from_atom(atom: &Atom) -> Result<Option<Datetime>, Error> {
        Ok(Some(match *atom {
            Atom::Ext(ref ext) => {
                if let Some(value) = ext.downcast_ref::<Datetime>() {
                    *value
                } else if let Some(value) = ext.downcast_ref::<Timestamp>() {
                    Datetime::try_from(*value)?
                } else {
                    return Ok(None);
                }
            }
            Atom::Str(ref value) => value.parse()?,
            _ => return Ok(None),
        }))
    }
}

impl_well_known!(Datetime);

impl Extension for Timestamp {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(self.to_string().into())
    }
}

impl WellKnown for Timestamp {
    const EXPECTING: &'static str = "timestamp";

    fn descriptor() -> &'static dyn Descriptor {
        &TIMESTAMP_DESCRIPTOR
    }

    /// Accepts timestamps, offset date-times, strings and numbers (seconds
    /// since the epoch).
    fn from_atom(atom: &Atom) -> Result<Option<Timestamp>, Error> {
        Ok(Some(match *atom {
            Atom::Ext(ref ext) => {
                if let Some(value) = ext.downcast_ref::<Timestamp>() {
                    *value
                } else if let Some(value) = ext.downcast_ref::<Datetime>() {
                    Timestamp::try_from(*value)?
                } else {
                    return Ok(None);
                }
            }
            Atom::Str(ref value) => value.parse()?,
            Atom::U64(value) => Timestamp {
                seconds: i64::try_from(value)
                    .map_err(|_| out_of_range("timestamp out of range"))?,
                nanosecond: 0,
            },
            Atom::I64(value) => Timestamp {
                seconds: value,
                nanosecond: 0,
            },
            Atom::F64(value) => {
                if !value.is_finite() || value.abs() >= 9.2e18 {
                    return Err(out_of_range("timestamp out of range"));
                }
                let seconds = value.floor();
                Timestamp {
                    seconds: seconds as i64,
                    nanosecond: (((value - seconds) * 1e9).round() as u32).min(999_999_999),
                }
            }
            _ => return Ok(None),
        }))
    }
}

impl_well_known!(Timestamp);

#[test]
fn test_civil_roundtrip() {
    for days in [
        -1_000_000i64,
        -719468,
        -1,
        0,
        1,
        11016,
        2_932_896,
        10_000_000,
    ] {
        let (y, m, d) = civil_from_days(days);
        assert_eq!(days_from_civil(y, m, d), days);
    }
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(days_from_civil(2000, 3, 1), 11017);
}

#[test]
fn test_timestamp_format() {
    let ts = Timestamp {
        seconds: -1,
        nanosecond: 500_000_000,
    };
    assert_eq!(ts.to_string(), "1969-12-31T23:59:59.5Z");
    assert_eq!(ts.to_string().parse::<Timestamp>().unwrap(), ts);
    let far = Timestamp {
        seconds: 253402300800,
        nanosecond: 0,
    };
    assert_eq!(far.to_string(), "+010000-01-01T00:00:00Z");
    assert_eq!(far.to_string().parse::<Timestamp>().unwrap(), far);
    let before = Timestamp {
        seconds: -62167219201,
        nanosecond: 0,
    };
    assert_eq!(before.to_string(), "-000001-12-31T23:59:59Z");
    assert_eq!(before.to_string().parse::<Timestamp>().unwrap(), before);
    assert!("1979-05-27T07:32:00".parse::<Timestamp>().is_err());
    assert!("+10000-01-01T00:00:00Z".parse::<Timestamp>().is_err());
}
