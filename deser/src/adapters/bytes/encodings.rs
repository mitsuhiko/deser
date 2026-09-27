//! The encodings of bytes as strings.
use crate::adapters::bytes::BytesEncoding;
use crate::error::{Error, ErrorKind};

#[cold]
fn invalid(name: &str) -> Error {
    Error::new(ErrorKind::Unexpected, format!("invalid {} string", name))
}

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

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
    let char_at = |value: u32| alphabet[(value & 0x3f) as usize];
    let (chunks, remainder) = bytes.as_chunks::<3>();
    let tail_len = match remainder.len() {
        0 => 0,
        _ if pad => 4,
        len => len + 1,
    };
    let start = out.len();
    // SAFETY: the string only gets ASCII characters (the characters of the
    // alphabet, `=` and the zeros from `resize`), it stays valid UTF-8 also
    // if this panics.
    let buf = unsafe { out.as_mut_vec() };
    buf.resize(start + chunks.len() * 4 + tail_len, 0);
    let (body, tail) = buf[start..].split_at_mut(chunks.len() * 4);
    for (&[a, b, c], dst) in chunks.iter().zip(body.as_chunks_mut::<4>().0) {
        let n = (a as u32) << 16 | (b as u32) << 8 | c as u32;
        *dst = [
            char_at(n >> 18),
            char_at(n >> 12),
            char_at(n >> 6),
            char_at(n),
        ];
    }
    let quad = match *remainder {
        [a] => {
            let n = (a as u32) << 16;
            [char_at(n >> 18), char_at(n >> 12), b'=', b'=']
        }
        [a, b] => {
            let n = (a as u32) << 16 | (b as u32) << 8;
            [char_at(n >> 18), char_at(n >> 12), char_at(n >> 6), b'=']
        }
        _ => return,
    };
    tail.copy_from_slice(&quad[..tail_len]);
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
    let (chunks, remainder) = input[..input.len() - padding].as_chunks::<4>();
    if remainder.len() == 1 || (padding > 0 && remainder.len() + padding != 4) {
        return Err(invalid("base64"));
    }

    // values are below 64, `INVALID` is not
    let value = |b: u8| BASE64_VALUES[b as usize];
    let mut out = vec![0; chunks.len() * 3 + remainder.len().saturating_sub(1)];
    let (body, tail) = out.split_at_mut(chunks.len() * 3);
    for (&[a, b, c, d], dst) in chunks.iter().zip(body.as_chunks_mut::<3>().0) {
        let (a, b, c, d) = (value(a), value(b), value(c), value(d));
        if a | b | c | d >= 64 {
            return Err(invalid("base64"));
        }
        let n = (a as u32) << 18 | (b as u32) << 12 | (c as u32) << 6 | d as u32;
        *dst = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
    }
    match *remainder {
        [a, b] => {
            let (a, b) = (value(a), value(b));
            if a | b >= 64 || b & 0xf != 0 {
                return Err(invalid("base64"));
            }
            tail[0] = a << 2 | b >> 4;
        }
        [a, b, c] => {
            let (a, b, c) = (value(a), value(b), value(c));
            if a | b | c >= 64 || c & 0x3 != 0 {
                return Err(invalid("base64"));
            }
            tail.copy_from_slice(&[a << 2 | b >> 4, b << 4 | c >> 2]);
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

/// Maps hex digits (both cases) to their values.
static HEX_VALUES: [u8; 256] = {
    let mut table = [INVALID; 256];
    let mut idx = 0;
    while idx < 16 {
        table[HEX_LOWER[idx] as usize] = idx as u8;
        table[HEX_UPPER[idx] as usize] = idx as u8;
        idx += 1;
    }
    table
};

fn decode_hex(s: &str) -> Result<Vec<u8>, Error> {
    let (pairs, []) = s.as_bytes().as_chunks::<2>() else {
        return Err(invalid("hex"));
    };
    let mut out = Vec::with_capacity(pairs.len());
    for &[hi, lo] in pairs {
        let (hi, lo) = (HEX_VALUES[hi as usize], HEX_VALUES[lo as usize]);
        // digits are below 16, `INVALID` is not
        if (hi | lo) >= 16 {
            return Err(invalid("hex"));
        }
        out.push(hi << 4 | lo);
    }
    Ok(out)
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
    /// (see [adapters documentation](crate::adapters#bytes)).
    Base64,
    "base64",
    |bytes, out| encode_base64(bytes, STANDARD, true, out),
    decode_base64
);

encoding!(
    /// Base64 with the standard alphabet without padding.
    ///
    /// Decoding is lenient (see [adapters documentation](crate::adapters#bytes)).
    Base64NoPad,
    "base64-nopad",
    |bytes, out| encode_base64(bytes, STANDARD, false, out),
    decode_base64
);

encoding!(
    /// Base64 with the URL-safe alphabet and padding (RFC 4648 section 5).
    ///
    /// Decoding is lenient (see [adapters documentation](crate::adapters#bytes)).
    Base64Url,
    "base64url",
    |bytes, out| encode_base64(bytes, URL_SAFE, true, out),
    decode_base64
);

encoding!(
    /// Base64 with the URL-safe alphabet without padding.
    ///
    /// Decoding is lenient (see [adapters documentation](crate::adapters#bytes)).
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
    |bytes, out| encode_hex(bytes, HEX_LOWER, out),
    decode_hex
);

encoding!(
    /// Hexadecimal with uppercase digits.
    ///
    /// Lowercase and uppercase digits are accepted when decoding.
    HexUpper,
    "hex-upper",
    |bytes, out| encode_hex(bytes, HEX_UPPER, out),
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

    /// Encodes base64 bit by bit.
    fn reference_base64(bytes: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
        let bits: Vec<bool> = bytes
            .iter()
            .flat_map(|byte| (0..8).rev().map(move |idx| byte >> idx & 1 == 1))
            .collect();
        let mut rv: String = bits
            .chunks(6)
            .map(|chunk| {
                let value = (0..6).fold(0, |acc, idx| {
                    acc << 1 | chunk.get(idx).copied().unwrap_or(false) as usize
                });
                alphabet[value] as char
            })
            .collect();
        while pad && !rv.len().is_multiple_of(4) {
            rv.push('=');
        }
        rv
    }

    #[test]
    fn test_base64_roundtrip() {
        let data: Vec<u8> = (0..=255).rev().chain(0..=255).collect();
        for len in (0..10).chain([254, 255, 256, 511, 512]) {
            let bytes = &data[..len];
            for (encoded, alphabet, pad) in [
                (encode::<Base64>(bytes), STANDARD, true),
                (encode::<Base64NoPad>(bytes), STANDARD, false),
                (encode::<Base64Url>(bytes), URL_SAFE, true),
                (encode::<Base64UrlNoPad>(bytes), URL_SAFE, false),
            ] {
                assert_eq!(encoded, reference_base64(bytes, alphabet, pad));
                assert_eq!(decode_base64(&encoded).unwrap(), bytes);
            }
        }

        // encoding appends
        let mut out = String::from("x");
        Base64::encode(b"f", &mut out);
        Base64UrlNoPad::encode(b"\xfb\xff", &mut out);
        assert_eq!(out, "xZg==-_8");
    }

    #[test]
    fn test_base64_invalid_chars() {
        let valid = encode::<Base64>(&[0xa5; 8]);
        for idx in 0..valid.len() - 1 {
            for invalid in [b' ', b'.', b'\n', b'=', 0xc3] {
                let mut bytes = valid.clone().into_bytes();
                bytes[idx] = invalid;
                let s = String::from_utf8_lossy(&bytes);
                assert!(decode_base64(&s).is_err(), "{s:?}");
            }
        }
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
        for invalid in ["0/", "0:", "@0", "G0", "0`", "g0", "\u{ff}"] {
            assert!(decode_hex(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(decode_hex("09afAF").unwrap(), b"\x09\xaf\xaf");
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
