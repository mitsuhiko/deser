//! Percent-encoding and decoding of `application/x-www-form-urlencoded`.
use std::borrow::Cow;

/// Text that was percent-decoded.
pub(crate) enum Decoded<'a> {
    /// The decoded text, borrowed if nothing was decoded.
    Text(Cow<'a, str>),
    /// The decoded bytes are not UTF-8.
    Bytes(Vec<u8>),
}

/// Percent-decodes a key or value.
///
/// `+` is decoded as space.  Like in the URL standard, `%` which is not
/// followed by two hex digits is kept as it is.
pub(crate) fn decode(input: &str) -> Decoded<'_> {
    let bytes = input.as_bytes();
    let first = match bytes.iter().position(|&b| b == b'%' || b == b'+') {
        Some(first) => first,
        None => return Decoded::Text(Cow::Borrowed(input)),
    };
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..first]);
    let mut pos = first;
    while pos < bytes.len() {
        match bytes[pos] {
            b'+' => {
                out.push(b' ');
                pos += 1;
            }
            b'%' => match (hex(bytes.get(pos + 1)), hex(bytes.get(pos + 2))) {
                (Some(hi), Some(lo)) => {
                    out.push(hi << 4 | lo);
                    pos += 3;
                }
                _ => {
                    out.push(b'%');
                    pos += 1;
                }
            },
            byte => {
                out.push(byte);
                pos += 1;
            }
        }
    }
    match String::from_utf8(out) {
        Ok(text) => Decoded::Text(Cow::Owned(text)),
        Err(err) => Decoded::Bytes(err.into_bytes()),
    }
}

#[inline]
fn hex(byte: Option<&u8>) -> Option<u8> {
    match *byte? {
        b @ b'0'..=b'9' => Some(b - b'0'),
        b @ b'a'..=b'f' => Some(b - b'a' + 10),
        b @ b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-encodes a key or value and appends it to the output.
///
/// This uses the percent-encode set of `application/x-www-form-urlencoded`:
/// ASCII alphanumerics and `*-._` are kept, space is encoded as `+` (or as
/// `%20`), everything else is percent-encoded.
pub(crate) fn encode(input: &[u8], space_as_plus: bool, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in input {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(byte as char)
            }
            b' ' if space_as_plus => out.push('+'),
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0xf) as usize] as char);
            }
        }
    }
}

#[cfg(test)]
fn decoded(input: &str) -> Result<String, Vec<u8>> {
    match decode(input) {
        Decoded::Text(text) => Ok(text.into_owned()),
        Decoded::Bytes(bytes) => Err(bytes),
    }
}

#[test]
fn test_decode() {
    assert!(matches!(decode("abc"), Decoded::Text(Cow::Borrowed("abc"))));
    assert_eq!(decoded("a+b%20c").unwrap(), "a b c");
    assert_eq!(decoded("%5Ba%5d").unwrap(), "[a]");
    assert_eq!(decoded("%C3%A4").unwrap(), "\u{e4}");
    assert_eq!(decoded("100%").unwrap(), "100%");
    assert_eq!(decoded("%2").unwrap(), "%2");
    assert_eq!(decoded("%zz%41").unwrap(), "%zzA");
    assert_eq!(decoded("%%41").unwrap(), "%A");
    assert_eq!(decoded("%FF").unwrap_err(), [255]);
}

#[test]
fn test_encode() {
    let mut out = String::new();
    encode("a b&c=d/ä[]*-._~".as_bytes(), true, &mut out);
    assert_eq!(out, "a+b%26c%3Dd%2F%C3%A4%5B%5D*-._%7E");
    out.clear();
    encode(b"a b", false, &mut out);
    assert_eq!(out, "a%20b");
}
