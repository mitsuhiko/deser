//! Helpers shared by the readers and writers.
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use deser_core::de::DeserializeDriver;
use deser_core::ext::Timestamp;
use deser_core::{Error, ErrorKind, Event, State};

/// Receives the events of a reader.
///
/// Events which borrow from the input are passed to
/// [`emit_input`](Self::emit_input), which passes them on borrowed if the
/// input lives long enough.
pub(crate) trait Out<'i> {
    fn state_mut(&mut self) -> &mut State;
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error>;
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error>;

    /// Emits an event for the input range `start..end`.
    #[inline(always)]
    fn emit_at<'e, E: Into<Event<'e>>>(
        &mut self,
        start: usize,
        end: usize,
        event: E,
    ) -> Result<(), Error> {
        self.state_mut().set_input_range(start, end);
        self.emit(event)
    }

    /// Emits a borrowed event for the input range `start..end`.
    #[inline(always)]
    fn emit_input_at(&mut self, start: usize, end: usize, event: Event<'i>) -> Result<(), Error> {
        self.state_mut().set_input_range(start, end);
        self.emit_input(event)
    }
}

/// Passes events of the input on borrowed.
pub(crate) struct Borrowing<'a, 'd, 'i>(pub &'a mut DeserializeDriver<'d, 'i>);

impl<'i> Out<'i> for Borrowing<'_, '_, 'i> {
    #[inline(always)]
    fn state_mut(&mut self) -> &mut State {
        self.0.state_mut()
    }

    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        self.0.emit(event)
    }

    #[inline(always)]
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error> {
        self.0.emit_borrowed(event)
    }
}

/// Passes events of the input on as data that is only valid for the call.
///
/// This is used for input that was converted (UTF-16 text).
pub(crate) struct Copying<'a, 'd, 'de>(pub &'a mut DeserializeDriver<'d, 'de>);

impl<'i> Out<'i> for Copying<'_, '_, '_> {
    #[inline(always)]
    fn state_mut(&mut self) -> &mut State {
        self.0.state_mut()
    }

    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        self.0.emit(event)
    }

    #[inline(always)]
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error> {
        self.0.emit(event)
    }
}

#[cold]
pub(crate) fn syntax_error(offset: usize, msg: &str) -> Error {
    Error::new(ErrorKind::Syntax, format!("syntax error: {}", msg)).with_offset(offset)
}

#[cold]
pub(crate) fn eof_error(offset: usize) -> Error {
    Error::new(ErrorKind::EndOfFile, "unexpected end of input").with_offset(offset)
}

/// The seconds between the Unix epoch and the epoch of property lists
/// (`2001-01-01T00:00:00Z`).
const PLIST_EPOCH: i64 = 978_307_200;

/// Converts the seconds since the epoch of property lists into a
/// timestamp.
///
/// The fraction is rounded to microseconds: the dates are stored as `f64`
/// which cannot hold more precision for current dates, and this makes
/// fractions such as `0.1` come out as expected.
pub(crate) fn timestamp_from_plist(value: f64) -> Option<Timestamp> {
    // the range keeps the conversions below exact
    const LIMIT: f64 = 9_007_199_254_740_992.0; // 2^53
    if !(value > -LIMIT && value < LIMIT) {
        return None;
    }
    let mut seconds = value as i64;
    if seconds as f64 > value {
        seconds -= 1;
    }
    let micros = (value - seconds as f64) * 1e6;
    let mut micros = (micros + 0.5) as u32;
    if micros >= 1_000_000 {
        seconds += 1;
        micros -= 1_000_000;
    }
    Some(Timestamp {
        seconds: seconds.checked_add(PLIST_EPOCH)?,
        nanosecond: micros * 1000,
    })
}

/// Converts a timestamp into the seconds since the epoch of property
/// lists.
pub(crate) fn timestamp_to_plist(value: &Timestamp) -> f64 {
    let seconds = i128::from(value.seconds) - i128::from(PLIST_EPOCH);
    seconds as f64 + f64::from(value.nanosecond) / 1e9
}

/// Formats a timestamp as a date of XML property lists.
///
/// These dates have no fraction, it's truncated.
pub(crate) fn format_xml_date(value: &Timestamp) -> String {
    Timestamp {
        seconds: value.seconds,
        nanosecond: 0,
    }
    .to_string()
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes bytes as base64 with padding.
pub(crate) fn encode_base64(data: &[u8], out: &mut String) {
    let (chunks, rest) = data.as_chunks::<3>();
    for chunk in chunks {
        let n = u32::from(chunk[0]) << 16 | u32::from(chunk[1]) << 8 | u32::from(chunk[2]);
        for shift in [18, 12, 6, 0] {
            out.push(BASE64_ALPHABET[(n >> shift) as usize & 63] as char);
        }
    }
    match *rest {
        [a] => {
            let n = u32::from(a) << 16;
            out.push(BASE64_ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(BASE64_ALPHABET[(n >> 12) as usize & 63] as char);
            out.push_str("==");
        }
        [a, b] => {
            let n = u32::from(a) << 16 | u32::from(b) << 8;
            out.push(BASE64_ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(BASE64_ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(BASE64_ALPHABET[(n >> 6) as usize & 63] as char);
            out.push('=');
        }
        _ => {}
    }
}

/// Decodes base64 as found in `<data>` elements.
///
/// Whitespace is ignored anywhere, the padding is optional and decoding
/// stops at the first `=`.  Leftover bits are ignored.  Returns the offset
/// of an invalid character as error.
pub(crate) fn decode_base64(text: &str) -> Result<Vec<u8>, usize> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for (idx, c) in text.bytes().enumerate() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                // only padding and whitespace may follow
                return match text[idx..]
                    .bytes()
                    .position(|c| c != b'=' && !c.is_ascii_whitespace())
                {
                    Some(pos) => Err(idx + pos),
                    None => Ok(out),
                };
            }
            c if c.is_ascii_whitespace() => continue,
            _ => return Err(idx),
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// Decodes UTF-16 text with a byte order mark.
///
/// Returns `None` if the input does not start with a UTF-16 byte order
/// mark, the error is the offset of invalid data.
pub(crate) fn decode_utf16_text(input: &[u8]) -> Option<Result<String, usize>> {
    let big_endian = if input.starts_with(b"\xfe\xff") {
        true
    } else if input.starts_with(b"\xff\xfe") {
        false
    } else {
        return None;
    };
    let body = &input[2..];
    let (units, rest) = body.as_chunks::<2>();
    if !rest.is_empty() {
        return Some(Err(input.len() - 1));
    }
    let units = units.iter().map(|&unit| {
        if big_endian {
            u16::from_be_bytes(unit)
        } else {
            u16::from_le_bytes(unit)
        }
    });
    let mut out = String::with_capacity(body.len() / 2);
    for (idx, c) in char::decode_utf16(units).enumerate() {
        match c {
            Ok(c) => out.push(c),
            Err(_) => return Some(Err(2 + idx * 2)),
        }
    }
    Some(Ok(out))
}

#[test]
fn test_base64() {
    for len in 0..20 {
        let data: Vec<u8> = (0..len as u8).map(|x| x.wrapping_mul(37)).collect();
        let mut encoded = String::new();
        encode_base64(&data, &mut encoded);
        assert_eq!(decode_base64(&encoded).unwrap(), data);
        let spaced: String = encoded
            .chars()
            .flat_map(|c| [c, '\n'])
            .filter(|&c| c != '=')
            .collect();
        assert_eq!(decode_base64(&spaced).unwrap(), data);
    }
    assert_eq!(decode_base64("AA=x"), Err(3));
    assert_eq!(decode_base64("A*"), Err(1));
}

#[test]
fn test_timestamps() {
    let ts = timestamp_from_plist(0.0).unwrap();
    assert_eq!(ts.to_string(), "2001-01-01T00:00:00Z");
    let ts = timestamp_from_plist(-0.5).unwrap();
    assert_eq!(ts.to_string(), "2000-12-31T23:59:59.5Z");
    let ts = timestamp_from_plist(1.1).unwrap();
    assert_eq!(ts.nanosecond, 100_000_000);
    assert_eq!(timestamp_to_plist(&ts), 1.1);
    assert_eq!(timestamp_from_plist(f64::NAN), None);
    assert_eq!(timestamp_from_plist(f64::INFINITY), None);
}
