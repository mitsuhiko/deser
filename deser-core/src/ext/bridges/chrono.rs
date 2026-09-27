use ::chrono::{
    DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Utc,
};

use crate::error::Error;
use crate::ext::known::{Bridge, impl_bridge, invalid, out_of_range};
use crate::ext::{Date, Datetime, Duration, Offset, Time, Timestamp};

fn date_to_known(value: NaiveDate) -> Result<Date, Error> {
    match u16::try_from(value.year()) {
        Ok(year) if year <= 9999 => Ok(Date {
            year,
            month: value.month() as u8,
            day: value.day() as u8,
        }),
        _ => Err(out_of_range("year out of range")),
    }
}

fn time_to_known(value: NaiveTime) -> Time {
    // chrono represents leap seconds with nanoseconds beyond one second
    let (second, nanosecond) = if value.nanosecond() >= 1_000_000_000 {
        (60, value.nanosecond() - 1_000_000_000)
    } else {
        (value.second() as u8, value.nanosecond())
    };
    Time {
        hour: value.hour() as u8,
        minute: value.minute() as u8,
        second,
        nanosecond,
    }
}

fn date_from_known(value: Date) -> Result<NaiveDate, Error> {
    NaiveDate::from_ymd_opt(i32::from(value.year), value.month.into(), value.day.into())
        .ok_or_else(|| invalid("invalid date"))
}

fn time_from_known(value: Time) -> Result<NaiveTime, Error> {
    let (second, nanosecond) = if value.second == 60 {
        (59, value.nanosecond + 1_000_000_000)
    } else {
        (value.second, value.nanosecond)
    };
    NaiveTime::from_hms_nano_opt(
        value.hour.into(),
        value.minute.into(),
        second.into(),
        nanosecond,
    )
    .ok_or_else(|| invalid("invalid time"))
}

fn datetime_to_known(value: NaiveDateTime) -> Result<Datetime, Error> {
    Ok(Datetime {
        date: Some(date_to_known(value.date())?),
        time: Some(time_to_known(value.time())),
        offset: None,
    })
}

impl Bridge for NaiveDate {
    type Known = Datetime;

    const EXPECTING: &'static str = "local date";

    fn to_known(&self) -> Result<Datetime, Error> {
        date_to_known(*self).map(Datetime::from)
    }

    fn from_known(value: Datetime) -> Result<NaiveDate, Error> {
        date_from_known(value.expect_local_date()?)
    }
}

impl Bridge for NaiveTime {
    type Known = Datetime;

    const EXPECTING: &'static str = "local time";

    fn to_known(&self) -> Result<Datetime, Error> {
        Ok(Datetime::from(time_to_known(*self)))
    }

    fn from_known(value: Datetime) -> Result<NaiveTime, Error> {
        time_from_known(value.expect_local_time()?)
    }
}

impl Bridge for NaiveDateTime {
    type Known = Datetime;

    const EXPECTING: &'static str = "local date-time";

    fn to_known(&self) -> Result<Datetime, Error> {
        datetime_to_known(*self)
    }

    fn from_known(value: Datetime) -> Result<NaiveDateTime, Error> {
        let (date, time) = value.expect_local_datetime()?;
        Ok(NaiveDateTime::new(
            date_from_known(date)?,
            time_from_known(time)?,
        ))
    }
}

impl Bridge for DateTime<FixedOffset> {
    type Known = Datetime;

    const EXPECTING: &'static str = "offset date-time";

    fn to_known(&self) -> Result<Datetime, Error> {
        let seconds = self.offset().local_minus_utc();
        if seconds % 60 != 0 {
            return Err(out_of_range("offsets with seconds are not supported"));
        }
        let mut rv = datetime_to_known(self.naive_local())?;
        rv.offset = Some(match seconds / 60 {
            0 => Offset::Z,
            minutes => Offset::Custom {
                minutes: minutes as i16,
            },
        });
        Ok(rv)
    }

    fn from_known(value: Datetime) -> Result<DateTime<FixedOffset>, Error> {
        let (date, time, offset) = value.expect_offset_datetime()?;
        let offset = FixedOffset::east_opt(i32::from(offset.minutes()) * 60)
            .ok_or_else(|| invalid("invalid offset"))?;
        NaiveDateTime::new(date_from_known(date)?, time_from_known(time)?)
            .and_local_timezone(offset)
            .single()
            .ok_or_else(|| out_of_range("date-time out of range"))
    }
}

impl Bridge for DateTime<Utc> {
    type Known = Timestamp;

    fn to_known(&self) -> Result<Timestamp, Error> {
        Ok(Timestamp {
            seconds: self.timestamp(),
            // leap seconds are clamped
            nanosecond: self.timestamp_subsec_nanos().min(999_999_999),
        })
    }

    fn from_known(value: Timestamp) -> Result<DateTime<Utc>, Error> {
        DateTime::from_timestamp(value.seconds, value.nanosecond)
            .ok_or_else(|| out_of_range("timestamp out of range"))
    }
}

impl Bridge for TimeDelta {
    type Known = Duration;

    fn to_known(&self) -> Result<Duration, Error> {
        Ok(Duration {
            seconds: self.num_seconds(),
            nanosecond: self.subsec_nanos(),
        })
    }

    fn from_known(value: Duration) -> Result<TimeDelta, Error> {
        TimeDelta::try_seconds(value.seconds)
            .and_then(|x| x.checked_add(&TimeDelta::nanoseconds(value.nanosecond.into())))
            .ok_or_else(|| out_of_range("duration out of range"))
    }
}

impl_bridge!(
    NaiveDate,
    NaiveTime,
    NaiveDateTime,
    DateTime<FixedOffset>,
    DateTime<Utc>,
    TimeDelta,
);
