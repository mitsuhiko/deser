use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use crate::error::Error;
use crate::ext::known::{Bridge, impl_bridge, out_of_range};
use crate::ext::{Duration, Timestamp};

impl Bridge for StdDuration {
    type Known = Duration;

    fn to_known(&self) -> Result<Duration, Error> {
        Ok(Duration {
            seconds: i64::try_from(self.as_secs())
                .map_err(|_| out_of_range("duration out of range"))?,
            nanosecond: self.subsec_nanos() as i32,
        })
    }

    fn from_known(value: Duration) -> Result<StdDuration, Error> {
        if value.is_negative() {
            return Err(out_of_range("duration cannot be negative"));
        }
        Ok(StdDuration::new(
            value.seconds as u64,
            value.nanosecond as u32,
        ))
    }
}

impl Bridge for SystemTime {
    type Known = Timestamp;

    fn to_known(&self) -> Result<Timestamp, Error> {
        let range_error = || out_of_range("system time out of range");
        match self.duration_since(UNIX_EPOCH) {
            Ok(duration) => Ok(Timestamp {
                seconds: i64::try_from(duration.as_secs()).map_err(|_| range_error())?,
                nanosecond: duration.subsec_nanos(),
            }),
            Err(err) => {
                let duration = err.duration();
                let seconds = i64::try_from(duration.as_secs()).map_err(|_| range_error())?;
                Timestamp::normalized(-seconds, -i64::from(duration.subsec_nanos()))
                    .ok_or_else(range_error)
            }
        }
    }

    fn from_known(value: Timestamp) -> Result<SystemTime, Error> {
        let rv = if value.seconds >= 0 {
            UNIX_EPOCH.checked_add(StdDuration::new(value.seconds as u64, value.nanosecond))
        } else {
            UNIX_EPOCH
                .checked_sub(StdDuration::from_secs(value.seconds.unsigned_abs()))
                .and_then(|x| x.checked_add(StdDuration::from_nanos(value.nanosecond.into())))
        };
        rv.ok_or_else(|| out_of_range("timestamp out of range for system time"))
    }
}

impl_bridge!(StdDuration, SystemTime);
