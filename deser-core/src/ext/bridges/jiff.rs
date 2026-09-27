use ::jiff::civil;
use ::jiff::tz::{self, TimeZone};
use ::jiff::{SignedDuration, Zoned};

use crate::error::Error;
use crate::ext::known::{Bridge, impl_bridge, invalid, out_of_range};
use crate::ext::{Date, Datetime, Duration, Offset, Time, Timestamp};

#[cold]
fn jiff_error(err: ::jiff::Error) -> Error {
    invalid(err.to_string()).with_source(err)
}

fn date_to_known(value: civil::Date) -> Result<Date, Error> {
    match u16::try_from(value.year()) {
        Ok(year) => Ok(Date {
            year,
            month: value.month() as u8,
            day: value.day() as u8,
        }),
        Err(_) => Err(out_of_range("negative years are not supported")),
    }
}

fn time_to_known(value: civil::Time) -> Time {
    Time {
        hour: value.hour() as u8,
        minute: value.minute() as u8,
        second: value.second() as u8,
        nanosecond: value.subsec_nanosecond() as u32,
    }
}

fn date_from_known(value: Date) -> Result<civil::Date, Error> {
    let year = i16::try_from(value.year).map_err(|_| out_of_range("year out of range"))?;
    civil::Date::new(year, value.month as i8, value.day as i8).map_err(jiff_error)
}

fn time_from_known(value: Time) -> Result<civil::Time, Error> {
    civil::Time::new(
        value.hour as i8,
        value.minute as i8,
        // jiff does not support leap seconds, they are clamped
        value.second.min(59) as i8,
        value.nanosecond as i32,
    )
    .map_err(jiff_error)
}

impl Bridge for civil::Date {
    type Known = Datetime;

    const EXPECTING: &'static str = "local date";

    fn to_known(&self) -> Result<Datetime, Error> {
        date_to_known(*self).map(Datetime::from)
    }

    fn from_known(value: Datetime) -> Result<civil::Date, Error> {
        date_from_known(value.expect_local_date()?)
    }

    fn parse_fallback(value: &str) -> Option<civil::Date> {
        value.parse().ok()
    }
}

impl Bridge for civil::Time {
    type Known = Datetime;

    const EXPECTING: &'static str = "local time";

    fn to_known(&self) -> Result<Datetime, Error> {
        Ok(Datetime::from(time_to_known(*self)))
    }

    fn from_known(value: Datetime) -> Result<civil::Time, Error> {
        time_from_known(value.expect_local_time()?)
    }

    fn parse_fallback(value: &str) -> Option<civil::Time> {
        value.parse().ok()
    }
}

impl Bridge for civil::DateTime {
    type Known = Datetime;

    const EXPECTING: &'static str = "local date-time";

    fn to_known(&self) -> Result<Datetime, Error> {
        Ok(Datetime {
            date: Some(date_to_known(self.date())?),
            time: Some(time_to_known(self.time())),
            offset: None,
        })
    }

    fn from_known(value: Datetime) -> Result<civil::DateTime, Error> {
        let (date, time) = value.expect_local_datetime()?;
        Ok(civil::DateTime::from_parts(
            date_from_known(date)?,
            time_from_known(time)?,
        ))
    }

    fn parse_fallback(value: &str) -> Option<civil::DateTime> {
        value.parse().ok()
    }
}

impl Bridge for Zoned {
    type Known = Datetime;

    const EXPECTING: &'static str = "offset date-time";

    /// Converts into an offset date-time, the time zone is lost.
    fn to_known(&self) -> Result<Datetime, Error> {
        let seconds = self.offset().seconds();
        if seconds % 60 != 0 {
            return Err(out_of_range("offsets with seconds are not supported"));
        }
        Ok(Datetime {
            date: Some(date_to_known(self.date())?),
            time: Some(time_to_known(self.time())),
            offset: Some(match seconds / 60 {
                0 => Offset::Z,
                minutes => Offset::Custom {
                    minutes: minutes as i16,
                },
            }),
        })
    }

    /// Converts an offset date-time into a zoned date-time with a fixed
    /// offset.
    fn from_known(value: Datetime) -> Result<Zoned, Error> {
        let (date, time, offset) = value.expect_offset_datetime()?;
        let datetime = civil::DateTime::from_parts(date_from_known(date)?, time_from_known(time)?);
        let offset =
            tz::Offset::from_seconds(i32::from(offset.minutes()) * 60).map_err(jiff_error)?;
        TimeZone::fixed(offset)
            .to_zoned(datetime)
            .map_err(jiff_error)
    }

    fn parse_fallback(value: &str) -> Option<Zoned> {
        value.parse().ok()
    }
}

impl Bridge for ::jiff::Timestamp {
    type Known = Timestamp;

    fn to_known(&self) -> Result<Timestamp, Error> {
        let nanos = self.as_nanosecond();
        Ok(Timestamp {
            seconds: nanos.div_euclid(1_000_000_000) as i64,
            nanosecond: nanos.rem_euclid(1_000_000_000) as u32,
        })
    }

    fn from_known(value: Timestamp) -> Result<::jiff::Timestamp, Error> {
        let nanos = i128::from(value.seconds) * 1_000_000_000 + i128::from(value.nanosecond);
        ::jiff::Timestamp::from_nanosecond(nanos).map_err(jiff_error)
    }

    fn parse_fallback(value: &str) -> Option<::jiff::Timestamp> {
        value.parse().ok()
    }
}

impl Bridge for SignedDuration {
    type Known = Duration;

    fn to_known(&self) -> Result<Duration, Error> {
        Ok(Duration {
            seconds: self.as_secs(),
            nanosecond: self.subsec_nanos(),
        })
    }

    fn from_known(value: Duration) -> Result<SignedDuration, Error> {
        if !value.is_valid() {
            return Err(invalid("invalid duration"));
        }
        Ok(SignedDuration::new(value.seconds, value.nanosecond))
    }

    fn parse_fallback(value: &str) -> Option<SignedDuration> {
        value.parse().ok()
    }
}

impl_bridge!(
    civil::Date,
    civil::Time,
    civil::DateTime,
    Zoned,
    ::jiff::Timestamp,
    SignedDuration,
);
