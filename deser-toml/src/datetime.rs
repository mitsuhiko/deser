//! The TOML grammar for date-times.
//!
//! The values are represented by the well-known [`Datetime`] type of deser.
use deser::ext::{Date, Datetime, Offset, Time};

/// Returns `true` if a date or time starts at the given offset.
///
/// Numbers never start with four digits followed by a dash or two digits
/// followed by a colon.
pub(crate) fn is_datetime_start(bytes: &[u8], pos: usize) -> bool {
    let digits = |from: usize, n: usize| {
        bytes
            .get(from..from + n)
            .is_some_and(|x| x.iter().all(u8::is_ascii_digit))
    };
    (digits(pos, 4) && bytes.get(pos + 4) == Some(&b'-'))
        || (digits(pos, 2) && bytes.get(pos + 2) == Some(&b':'))
}

/// Parses a date-time at the given offset.
///
/// Returns the value and the offset after it.  Errors carry the offset
/// where the problem was detected.
pub(crate) fn parse_datetime(
    bytes: &[u8],
    start: usize,
) -> Result<(Datetime, usize), (usize, &'static str)> {
    let mut pos = start;
    let mut rv = Datetime {
        date: None,
        time: None,
        offset: None,
    };

    if bytes.get(pos + 4) == Some(&b'-') {
        let date = Date {
            year: digits(bytes, &mut pos, 4)? as u16,
            month: {
                expect(bytes, &mut pos, b'-', "expected '-' in date")?;
                digits(bytes, &mut pos, 2)? as u8
            },
            day: {
                expect(bytes, &mut pos, b'-', "expected '-' in date")?;
                digits(bytes, &mut pos, 2)? as u8
            },
        };
        if !date.is_valid() {
            return Err((start, "invalid date"));
        }
        rv.date = Some(date);

        // the delimiter between date and time is a `T` or a space.  A space
        // only starts a time if it's followed by one, otherwise it's the
        // whitespace after a local date.
        match bytes.get(pos) {
            Some(b'T' | b't') => pos += 1,
            Some(b' ') if is_datetime_start(bytes, pos + 1) => pos += 1,
            _ => return Ok((rv, pos)),
        }
    }

    let time_start = pos;
    let mut time = Time {
        hour: digits(bytes, &mut pos, 2)? as u8,
        minute: {
            expect(bytes, &mut pos, b':', "expected ':' in time")?;
            digits(bytes, &mut pos, 2)? as u8
        },
        second: 0,
        nanosecond: 0,
    };
    // seconds are optional since TOML 1.1
    if bytes.get(pos) == Some(&b':') {
        pos += 1;
        time.second = digits(bytes, &mut pos, 2)? as u8;
        if bytes.get(pos) == Some(&b'.') {
            pos += 1;
            let frac_start = pos;
            let mut nanos = 0u32;
            while let Some(&c) = bytes.get(pos).filter(|x| x.is_ascii_digit()) {
                // digits beyond nanoseconds are truncated
                if pos - frac_start < 9 {
                    nanos = nanos * 10 + u32::from(c - b'0');
                }
                pos += 1;
            }
            if pos == frac_start {
                return Err((pos, "expected digits after decimal point"));
            }
            for _ in (pos - frac_start)..9 {
                nanos *= 10;
            }
            time.nanosecond = nanos;
        }
    }
    if !time.is_valid() {
        return Err((time_start, "invalid time"));
    }
    rv.time = Some(time);

    // only date-times have offsets
    if rv.date.is_some() {
        match bytes.get(pos) {
            Some(b'Z' | b'z') => {
                pos += 1;
                rv.offset = Some(Offset::Z);
            }
            Some(&sign @ (b'+' | b'-')) => {
                let offset_start = pos;
                pos += 1;
                let hours = digits(bytes, &mut pos, 2)?;
                expect(bytes, &mut pos, b':', "expected ':' in offset")?;
                let minutes = digits(bytes, &mut pos, 2)?;
                if hours > 23 || minutes > 59 {
                    return Err((offset_start, "invalid offset"));
                }
                let minutes = (hours * 60 + minutes) as i16;
                rv.offset = Some(Offset::Custom {
                    minutes: if sign == b'-' { -minutes } else { minutes },
                });
            }
            _ => {}
        }
    }

    Ok((rv, pos))
}

fn digits(bytes: &[u8], pos: &mut usize, n: usize) -> Result<u32, (usize, &'static str)> {
    let mut rv = 0;
    for _ in 0..n {
        match bytes.get(*pos) {
            Some(&c) if c.is_ascii_digit() => rv = rv * 10 + u32::from(c - b'0'),
            _ => return Err((*pos, "expected digit in date-time")),
        }
        *pos += 1;
    }
    Ok(rv)
}

fn expect(
    bytes: &[u8],
    pos: &mut usize,
    c: u8,
    msg: &'static str,
) -> Result<(), (usize, &'static str)> {
    if bytes.get(*pos) == Some(&c) {
        *pos += 1;
        Ok(())
    } else {
        Err((*pos, msg))
    }
}
