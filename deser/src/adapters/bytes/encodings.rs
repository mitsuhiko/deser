//! The encodings of bytes as strings.
use crate::adapters::bytes::BytesEncoding;
use crate::error::{Error, ErrorKind};

#[cold]
fn invalid(name: &str) -> Error {
    Error::new(ErrorKind::Unexpected, format!("invalid {} string", name))
}

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

const INVALID: u8 = 0xff;

/// Maps characters of both alphabets to their values.
static BASE64_VALUES: [u8; 256] = {
    let mut table = [INVALID; 256];
    let mut idx = 0;
    while idx < 64 {
        table[STANDARD[idx] as usize] = idx as u8;
        table[URL_SAFE[idx] as usize] = idx as u8;
        idx += 1;
    }
    table
};

fn encode_base64(bytes: &[u8], alphabet: &[u8; 64], pad: bool, out: &mut String) {
    let char_at = |value: u32| alphabet[(value & 0x3f) as usize] as char;
    out.reserve(bytes.len().div_ceil(3) * 4);
    let (chunks, remainder) = bytes.as_chunks::<3>();
    for &[a, b, c] in chunks {
        let n = (a as u32) << 16 | (b as u32) << 8 | c as u32;
        out.push(char_at(n >> 18));
        out.push(char_at(n >> 12));
        out.push(char_at(n >> 6));
        out.push(char_at(n));
    }
    match *remainder {
        [a] => {
            let n = (a as u32) << 16;
            out.push(char_at(n >> 18));
            out.push(char_at(n >> 12));
            if pad {
                out.push_str("==");
            }
        }
        [a, b] => {
            let n = (a as u32) << 16 | (b as u32) << 8;
            out.push(char_at(n >> 18));
            out.push(char_at(n >> 12));
            out.push(char_at(n >> 6));
            if pad {
                out.push('=');
            }
        }
        _ => {}
    }
}

/// Decodes base64 leniently.
///
/// Both alphabets are accepted (also mixed) and the padding is optional.
/// If there is padding it has to be complete.  Unused bits have to be zero.
pub(crate) fn decode_base64(s: &str) -> Result<Vec<u8>, Error> {
    let input = s.as_bytes();
    let padding = input
        .iter()
        .rev()
        .take(2)
        .take_while(|&&b| b == b'=')
        .count();
    let data = &input[..input.len() - padding];
    if data.len() % 4 == 1 || (padding > 0 && data.len() % 4 + padding != 4) {
        return Err(invalid("base64"));
    }

    let value = |b: u8| match BASE64_VALUES[b as usize] {
        INVALID => Err(invalid("base64")),
        value => Ok(value as u32),
    };
    let mut out = Vec::with_capacity(data.len() / 4 * 3 + 2);
    let (chunks, remainder) = data.as_chunks::<4>();
    for &[a, b, c, d] in chunks {
        let n = value(a)? << 18 | value(b)? << 12 | value(c)? << 6 | value(d)?;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    match *remainder {
        [a, b] => {
            let n = value(a)? << 18 | value(b)? << 12;
            if n & 0xffff != 0 {
                return Err(invalid("base64"));
            }
            out.push((n >> 16) as u8);
        }
        [a, b, c] => {
            let n = value(a)? << 18 | value(b)? << 12 | value(c)? << 6;
            if n & 0xff != 0 {
                return Err(invalid("base64"));
            }
            out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8]);
        }
        _ => {}
    }
    Ok(out)
}

fn encode_hex(bytes: &[u8], digits: &[u8; 16], out: &mut String) {
    out.reserve(bytes.len() * 2);
    for &byte in bytes {
        out.push(digits[(byte >> 4) as usize] as char);
        out.push(digits[(byte & 0xf) as usize] as char);
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, Error> {
    fn value(b: u8) -> Result<u8, Error> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => Err(invalid("hex")),
        }
    }
    let (pairs, []) = s.as_bytes().as_chunks::<2>() else {
        return Err(invalid("hex"));
    };
    pairs
        .iter()
        .map(|&[hi, lo]| Ok(value(hi)? << 4 | value(lo)?))
        .collect()
}

macro_rules! encoding {
    ($(#[$meta:meta])* $ty:ident, $name:expr, |$bytes:ident, $out:ident| $encode:expr, $decode:expr) => {
        $(#[$meta])*
        pub struct $ty;

        impl BytesEncoding for $ty {
            const NAME: &'static str = $name;

            fn encode($bytes: &[u8], $out: &mut String) {
                $encode
            }

            fn decode(s: &str) -> Result<Vec<u8>, Error> {
                $decode(s)
            }
        }
    };
}

encoding!(
    /// Base64 with the standard alphabet and padding (RFC 4648 section 4).
    ///
    /// This is the default representation of bytes.  Decoding is lenient
    /// (see [module documentation](crate::adapters::bytes)).
    Base64,
    "base64",
    |bytes, out| encode_base64(bytes, STANDARD, true, out),
    decode_base64
);

encoding!(
    /// Base64 with the standard alphabet without padding.
    ///
    /// Decoding is lenient (see [module documentation](crate::adapters::bytes)).
    Base64NoPad,
    "base64-nopad",
    |bytes, out| encode_base64(bytes, STANDARD, false, out),
    decode_base64
);

encoding!(
    /// Base64 with the URL-safe alphabet and padding (RFC 4648 section 5).
    ///
    /// Decoding is lenient (see [module documentation](crate::adapters::bytes)).
    Base64Url,
    "base64url",
    |bytes, out| encode_base64(bytes, URL_SAFE, true, out),
    decode_base64
);

encoding!(
    /// Base64 with the URL-safe alphabet without padding.
    ///
    /// Decoding is lenient (see [module documentation](crate::adapters::bytes)).
    Base64UrlNoPad,
    "base64url-nopad",
    |bytes, out| encode_base64(bytes, URL_SAFE, false, out),
    decode_base64
);

encoding!(
    /// Hexadecimal with lowercase digits.
    ///
    /// Lowercase and uppercase digits are accepted when decoding.
    Hex,
    "hex",
    |bytes, out| encode_hex(bytes, b"0123456789abcdef", out),
    decode_hex
);

encoding!(
    /// Hexadecimal with uppercase digits.
    ///
    /// Lowercase and uppercase digits are accepted when decoding.
    HexUpper,
    "hex-upper",
    |bytes, out| encode_hex(bytes, b"0123456789ABCDEF", out),
    decode_hex
);

#[cfg(feature = "bytes-encoding")]
mod extra {
    use super::{BytesEncoding, Error, ErrorKind};

    fn decode(encoding: &data_encoding::Encoding, name: &str, s: &str) -> Result<Vec<u8>, Error> {
        encoding.decode(s.as_bytes()).map_err(|err| {
            Error::new(
                ErrorKind::Unexpected,
                format!("invalid {} string: {}", name, err),
            )
        })
    }

    macro_rules! data_encoding {
        ($(#[$meta:meta])* $ty:ident, $name:expr, $encoding:ident) => {
            $(#[$meta])*
            pub struct $ty;

            impl BytesEncoding for $ty {
                const NAME: &'static str = $name;

                fn encode(bytes: &[u8], out: &mut String) {
                    data_encoding::$encoding.encode_append(bytes, out);
                }

                fn decode(s: &str) -> Result<Vec<u8>, Error> {
                    decode(&data_encoding::$encoding, Self::NAME, s)
                }
            }
        };
    }

    data_encoding!(
        /// Base32 with padding (RFC 4648 section 6).
        ///
        /// Requires the `bytes-encoding` feature.
        Base32,
        "base32",
        BASE32
    );

    data_encoding!(
        /// Base32 without padding.
        ///
        /// Requires the `bytes-encoding` feature.
        Base32NoPad,
        "base32-nopad",
        BASE32_NOPAD
    );

    data_encoding!(
        /// Base32 with the extended hex alphabet and padding (RFC 4648
        /// section 7).
        ///
        /// Requires the `bytes-encoding` feature.
        Base32Hex,
        "base32hex",
        BASE32HEX
    );

    data_encoding!(
        /// Base32 with the extended hex alphabet without padding.
        ///
        /// Requires the `bytes-encoding` feature.
        Base32HexNoPad,
        "base32hex-nopad",
        BASE32HEX_NOPAD
    );
}

#[cfg(feature = "bytes-encoding")]
pub use self::extra::{Base32, Base32Hex, Base32HexNoPad, Base32NoPad};

#[cfg(test)]
mod tests {
    use super::*;

    fn encode<E: BytesEncoding>(bytes: &[u8]) -> String {
        let mut rv = String::new();
        E::encode(bytes, &mut rv);
        rv
    }

    #[test]
    fn test_base64_encode() {
        // RFC 4648 section 10
        for (bytes, expected) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode::<Base64>(bytes), expected);
            assert_eq!(encode::<Base64NoPad>(bytes), expected.trim_end_matches('='));
            assert_eq!(decode_base64(expected).unwrap(), bytes);
            assert_eq!(
                decode_base64(expected.trim_end_matches('=')).unwrap(),
                bytes
            );
        }
        assert_eq!(encode::<Base64>(b"\xfb\xff"), "+/8=");
        assert_eq!(encode::<Base64Url>(b"\xfb\xff"), "-_8=");
        assert_eq!(encode::<Base64UrlNoPad>(b"\xfb\xff"), "-_8");
    }

    #[test]
    fn test_base64_decode() {
        assert_eq!(decode_base64("+/8=").unwrap(), b"\xfb\xff");
        assert_eq!(decode_base64("-_8").unwrap(), b"\xfb\xff");
        assert_eq!(decode_base64("+_8").unwrap(), b"\xfb\xff");
        for invalid in [
            "Z", "Zg=", "Zg===", "Zm9v=", "Zm9v==", "Z===", "Zh==", "Zm9=", "Zm 9v", "Zm9v\n", "=",
            "==", "Zg==Zg==",
        ] {
            assert!(decode_base64(invalid).is_err(), "{:?}", invalid);
        }
    }

    #[test]
    fn test_hex() {
        assert_eq!(encode::<Hex>(b"\x00\x1f\xab"), "001fab");
        assert_eq!(encode::<HexUpper>(b"\x00\x1f\xab"), "001FAB");
        assert_eq!(decode_hex("001fAB").unwrap(), b"\x00\x1f\xab");
        assert_eq!(decode_hex("").unwrap(), b"");
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("0g").is_err());
    }

    #[cfg(feature = "bytes-encoding")]
    #[test]
    fn test_data_encoding() {
        assert_eq!(encode::<Base32>(b"foo"), "MZXW6===");
        assert_eq!(encode::<Base32NoPad>(b"foo"), "MZXW6");
        assert_eq!(Base32Hex::decode("CPNMU===").unwrap(), b"foo");
        assert_eq!(Base32HexNoPad::decode("CPNMU").unwrap(), b"foo");
        assert_eq!(
            Base32::decode("x").unwrap_err().to_string(),
            "Unexpected: invalid base32 string: invalid length at 0"
        );
    }
}
