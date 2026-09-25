use ::time::{Month, OffsetDateTime, PrimitiveDateTime, UtcDateTime, UtcOffset};

use crate::error::Error;
use crate::ext::known::{impl_bridge, invalid, out_of_range, Bridge};
use crate::ext::{Date, Datetime, Duration, Offset, Time, Timestamp};

#[cold]
fn range_error(err: ::time::error::ComponentRange) -> Error {
    out_of_range(err.to_string()).with_source(err)
}

fn date_to_known(value: ::time::Date) -> Result<Date, Error> {
    match u16::try_from(value.year()) {
        Ok(year) if year <= 9999 => Ok(Date {
            year,
            month: u8::from(value.month()),
            day: value.day(),
        }),
        _ => Err(out_of_range("year out of range")),
    }
}

fn time_to_known(value: ::time::Time) -> Time {
    Time {
        hour: value.hour(),
        minute: value.minute(),
        second: value.second(),
        nanosecond: value.nanosecond(),
    }
}

fn date_from_known(value: Date) -> Result<::time::Date, Error> {
    let month = Month::try_from(value.month).map_err(range_error)?;
    ::time::Date::from_calendar_date(i32::from(value.year), month, value.day).map_err(range_error)
}

fn time_from_known(value: Time) -> Result<::time::Time, Error> {
    ::time::Time::from_hms_nano(
        value.hour,
        value.minute,
        // leap seconds are not supported, they are clamped
        value.second.min(59),
        value.nanosecond,
    )
    .map_err(range_error)
}

impl Bridge for ::time::Date {
    type Known = Datetime;

    const EXPECTING: &'static str = "local date";

    fn to_known(&self) -> Result<Datetime, Error> {
        date_to_known(*self).map(Datetime::from)
    }

    fn from_known(value: Datetime) -> Result<::time::Date, Error> {
        date_from_known(value.expect_local_date()?)
    }
}

impl Bridge for ::time::Time {
    type Known = Datetime;

    const EXPECTING: &'static str = "local time";

    fn to_known(&self) -> Result<Datetime, Error> {
        Ok(Datetime::from(time_to_known(*self)))
    }

    fn from_known(value: Datetime) -> Result<::time::Time, Error> {
        time_from_known(value.expect_local_time()?)
    }
}

impl Bridge for PrimitiveDateTime {
    type Known = Datetime;

    const EXPECTING: &'static str = "local date-time";

    fn to_known(&self) -> Result<Datetime, Error> {
        Ok(Datetime {
            date: Some(date_to_known(self.date())?),
            time: Some(time_to_known(self.time())),
            offset: None,
        })
    }

    fn from_known(value: Datetime) -> Result<PrimitiveDateTime, Error> {
        let (date, time) = value.expect_local_datetime()?;
        Ok(PrimitiveDateTime::new(
            date_from_known(date)?,
            time_from_known(time)?,
        ))
    }
}

impl Bridge for OffsetDateTime {
    type Known = Datetime;

    const EXPECTING: &'static str = "offset date-time";

    fn to_known(&self) -> Result<Datetime, Error> {
        let offset = self.offset();
        if offset.seconds_past_minute() != 0 {
            return Err(out_of_range("offsets with seconds are not supported"));
        }
        Ok(Datetime {
            date: Some(date_to_known(self.date())?),
            time: Some(time_to_known(self.time())),
            offset: Some(match offset.whole_minutes() {
                0 => Offset::Z,
                minutes => Offset::Custom { minutes },
            }),
        })
    }

    fn from_known(value: Datetime) -> Result<OffsetDateTime, Error> {
        let (date, time, offset) = value.expect_offset_datetime()?;
        let offset = UtcOffset::from_whole_seconds(i32::from(offset.minutes()) * 60)
            .map_err(|_| invalid("invalid offset"))?;
        Ok(OffsetDateTime::new_in_offset(
            date_from_known(date)?,
            time_from_known(time)?,
            offset,
        ))
    }
}

impl Bridge for UtcDateTime {
    type Known = Timestamp;

    fn to_known(&self) -> Result<Timestamp, Error> {
        let nanos = self.unix_timestamp_nanos();
        Ok(Timestamp {
            seconds: nanos.div_euclid(1_000_000_000) as i64,
            nanosecond: nanos.rem_euclid(1_000_000_000) as u32,
        })
    }

    fn from_known(value: Timestamp) -> Result<UtcDateTime, Error> {
        let nanos = i128::from(value.seconds) * 1_000_000_000 + i128::from(value.nanosecond);
        UtcDateTime::from_unix_timestamp_nanos(nanos).map_err(range_error)
    }
}

impl Bridge for ::time::Duration {
    type Known = Duration;

    fn to_known(&self) -> Result<Duration, Error> {
        Ok(Duration {
            seconds: self.whole_seconds(),
            nanosecond: self.subsec_nanoseconds(),
        })
    }

    fn from_known(value: Duration) -> Result<::time::Duration, Error> {
        if !value.is_valid() {
            return Err(invalid("invalid duration"));
        }
        Ok(::time::Duration::new(value.seconds, value.nanosecond))
    }
}

impl_bridge!(
    ::time::Date,
    ::time::Time,
    PrimitiveDateTime,
    OffsetDateTime,
    UtcDateTime,
    ::time::Duration,
);
