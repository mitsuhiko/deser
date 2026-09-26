//! Representations of bytes in formats without native bytes.
//!
//! Bytes (`Vec<u8>`, `[u8; N]`, `&[u8]` and `Cow<[u8]>`) are part of the
//! data model as [`Atom::Bytes`](crate::Atom::Bytes).  Formats which support
//! bytes natively (such as CBOR) use that, text formats like JSON and TOML
//! have to represent them differently.  In deser the convention is:
//!
//! * Formats without native bytes write bytes as base64 strings (RFC 4648,
//!   standard alphabet with padding).  They can be configured with a
//!   different [`BytesFormat`] (for instance to write sequences of integers).
//! * Types that expect bytes accept a string and decode it.  By default
//!   this is lenient base64: both the standard and the URL-safe alphabet
//!   are accepted and the padding is optional.  Sequences of integers are
//!   always accepted.
//!
//! How the bytes of an individual value are represented can be changed with
//! the adapters [`Encoded`](crate::adapters::Encoded),
//! [`EncodedStr`](crate::adapters::EncodedStr) and
//! [`ByteSeq`](crate::adapters::ByteSeq).  The encodings of this module
//! (such as [`Hex`]) are also adapters by themselves.
//!
//! ```
//! use deser::bytes::Hex;
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct Blob {
//!     // base64 in JSON and TOML, bytes in CBOR
//!     data: Vec<u8>,
//!     // hex in JSON and TOML, bytes in CBOR
//!     #[deser(as = Hex)]
//!     sha256: [u8; 32],
//! }
//! ```
//!
//! By default a value only changes how it's represented in formats without
//! native bytes.  To also use the encoding in formats with native bytes use
//! [`EncodedStr`](crate::adapters::EncodedStr).
//!
//! # Encodings
//!
//! These encodings are always available:
//!
//! | Encoding           | Description                                        |
//! |--------------------|----------------------------------------------------|
//! | [`Base64`]         | base64, standard alphabet with padding             |
//! | [`Base64NoPad`]    | base64, standard alphabet without padding          |
//! | [`Base64Url`]      | base64, URL-safe alphabet with padding             |
//! | [`Base64UrlNoPad`] | base64, URL-safe alphabet without padding          |
//! | [`Hex`]            | hexadecimal, lowercase                             |
//! | [`HexUpper`]       | hexadecimal, uppercase                             |
//!
//! All base64 encodings decode leniently like the default: both alphabets
//! are accepted and the padding is optional.  Both hex encodings accept
//! lowercase and uppercase digits.
//!
//! With the `bytes-encoding` feature more encodings are available:
//!
//! | Encoding           | Description                                        |
//! |--------------------|----------------------------------------------------|
//! | `Base32`           | base32 with padding                                |
//! | `Base32NoPad`      | base32 without padding                             |
//! | `Base32Hex`        | base32 with extended hex alphabet and padding      |
//! | `Base32HexNoPad`   | base32 with extended hex alphabet without padding  |
//!
//! Other encodings can be added by implementing [`BytesEncoding`].
use std::fmt;

use crate::error::{Error, ErrorKind};
use crate::State;

/// An encoding of bytes as string.
///
/// Encodings are types which are not instantiated.  They are used with
/// [`BytesFormat::encoded`] and the [`Encoded`](crate::adapters::Encoded)
/// and [`EncodedStr`](crate::adapters::EncodedStr) adapters.
///
/// ```
/// use deser::bytes::BytesEncoding;
/// use deser::{Error, ErrorKind};
///
/// /// Writes bytes as decimal numbers separated by dots.
/// pub struct Dotted;
///
/// impl BytesEncoding for Dotted {
///     const NAME: &'static str = "dotted";
///
///     fn encode(bytes: &[u8], out: &mut String) {
///         for (idx, byte) in bytes.iter().enumerate() {
///             if idx > 0 {
///                 out.push('.');
///             }
///             out.push_str(&byte.to_string());
///         }
///     }
///
///     fn decode(s: &str) -> Result<Vec<u8>, Error> {
///         if s.is_empty() {
///             return Ok(Vec::new());
///         }
///         s.split('.')
///             .map(|x| x.parse().map_err(|_| Error::new(ErrorKind::Unexpected, "invalid byte")))
///             .collect()
///     }
/// }
/// ```
pub trait BytesEncoding: 'static {
    /// The name of the encoding.
    ///
    /// The name is used in error messages and to compare [`BytesFormat`]s.
    const NAME: &'static str;

    /// Encodes bytes and appends them to the string.
    fn encode(bytes: &[u8], out: &mut String);

    /// Decodes a string.
    fn decode(s: &str) -> Result<Vec<u8>, Error>;
}

/// How bytes are represented in formats without native bytes.
///
/// This is used in three places:
///
/// * The serializers of formats without native bytes (JSON and TOML) can be
///   configured with a format.  It's used for all bytes that do not request
///   a format.  The default is [`BytesFormat::BASE64`].
/// * Values request a format with [`Descriptor::bytes_format`](crate::Descriptor::bytes_format)
///   which takes precedence over the configuration of the serializer.  The
///   [`Encoded`](crate::adapters::Encoded) and
///   [`ByteSeq`](crate::adapters::ByteSeq) adapters do this.
/// * The types that expect bytes decode strings with the format placed into
///   the [`State`].  The deserializers of formats without native bytes can
///   be configured to do this, otherwise lenient base64 is used.  Strings
///   are decoded as base64 for [`BytesFormat::SEQ`].
///
/// ```
/// use deser::bytes::{BytesFormat, Hex};
///
/// const HEX: BytesFormat = BytesFormat::encoded::<Hex>();
/// assert_eq!(HEX.encode(b"\x01\xff").as_deref(), Some("01ff"));
/// assert_eq!(HEX.decode("01FF").unwrap(), b"\x01\xff");
/// assert_eq!(BytesFormat::SEQ.encode(b"\x01\xff"), None);
/// ```
///
/// Two formats are equal if they are both sequences or encodings with the
/// same [name](BytesEncoding::NAME).
#[derive(Clone, Copy)]
pub struct BytesFormat(Repr);

#[derive(Clone, Copy)]
enum Repr {
    Encoded {
        name: &'static str,
        encode: fn(&[u8], &mut String),
        decode: fn(&str) -> Result<Vec<u8>, Error>,
    },
    Seq,
}

impl BytesFormat {
    /// Bytes are strings in base64 with the standard alphabet and padding.
    ///
    /// This is the default.
    pub const BASE64: BytesFormat = BytesFormat::encoded::<Base64>();

    /// Bytes are sequences of integers.
    ///
    /// This is how `serde_json` represents bytes.
    pub const SEQ: BytesFormat = BytesFormat(Repr::Seq);

    /// Bytes are strings in the given encoding.
    pub const fn encoded<E: BytesEncoding>() -> BytesFormat {
        BytesFormat(Repr::Encoded {
            name: E::NAME,
            encode: E::encode,
            decode: E::decode,
        })
    }

    /// Returns the name of the format.
    ///
    /// This is the [name](BytesEncoding::NAME) of the encoding or `seq`.
    pub fn name(&self) -> &'static str {
        match self.0 {
            Repr::Encoded { name, .. } => name,
            Repr::Seq => "seq",
        }
    }

    /// Returns `true` if bytes are sequences of integers.
    pub fn is_seq(&self) -> bool {
        matches!(self.0, Repr::Seq)
    }

    /// Encodes bytes as string.
    ///
    /// Returns `None` if bytes are sequences of integers.
    pub fn encode(&self, bytes: &[u8]) -> Option<String> {
        match self.0 {
            Repr::Encoded { encode, .. } => {
                let mut rv = String::new();
                encode(bytes, &mut rv);
                Some(rv)
            }
            Repr::Seq => None,
        }
    }

    /// Decodes bytes from a string.
    ///
    /// For sequences of integers the string is decoded as base64.
    pub fn decode(&self, s: &str) -> Result<Vec<u8>, Error> {
        match self.0 {
            Repr::Encoded { decode, .. } => decode(s),
            Repr::Seq => decode_base64(s),
        }
    }
}

impl Default for BytesFormat {
    fn default() -> BytesFormat {
        BytesFormat::BASE64
    }
}

impl fmt::Debug for BytesFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BytesFormat").field(&self.name()).finish()
    }
}

impl PartialEq for BytesFormat {
    fn eq(&self, other: &Self) -> bool {
        match (self.0, other.0) {
            (Repr::Encoded { name: a, .. }, Repr::Encoded { name: b, .. }) => a == b,
            (Repr::Seq, Repr::Seq) => true,
            _ => false,
        }
    }
}

impl Eq for BytesFormat {}

/// Decodes a string into bytes with the format in the state.
pub(crate) fn decode_str(s: &str, state: &State) -> Result<Vec<u8>, Error> {
    match state.get::<BytesFormat>() {
        Some(format) => format.decode(s),
        None => decode_base64(s),
    }
}

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

// `as_chunks` would be nicer but requires Rust 1.88
#[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
fn encode_base64(bytes: &[u8], alphabet: &[u8; 64], pad: bool, out: &mut String) {
    let char_at = |value: u32| alphabet[(value & 0x3f) as usize] as char;
    out.reserve(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32;
        out.push(char_at(n >> 18));
        out.push(char_at(n >> 12));
        out.push(char_at(n >> 6));
        out.push(char_at(n));
    }
    match *chunks.remainder() {
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
// `as_chunks` would be nicer but requires Rust 1.88
#[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
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
    let mut chunks = data.chunks_exact(4);
    for chunk in &mut chunks {
        let n = value(chunk[0])? << 18
            | value(chunk[1])? << 12
            | value(chunk[2])? << 6
            | value(chunk[3])?;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    match *chunks.remainder() {
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

// `as_chunks` would be nicer but requires Rust 1.88
#[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
fn decode_hex(s: &str) -> Result<Vec<u8>, Error> {
    fn value(b: u8) -> Result<u8, Error> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => Err(invalid("hex")),
        }
    }
    let input = s.as_bytes();
    if !input.len().is_multiple_of(2) {
        return Err(invalid("hex"));
    }
    input
        .chunks_exact(2)
        .map(|pair| Ok(value(pair[0])? << 4 | value(pair[1])?))
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
    /// (see [module documentation](self)).
    Base64,
    "base64",
    |bytes, out| encode_base64(bytes, STANDARD, true, out),
    decode_base64
);

encoding!(
    /// Base64 with the standard alphabet without padding.
    ///
    /// Decoding is lenient (see [module documentation](self)).
    Base64NoPad,
    "base64-nopad",
    |bytes, out| encode_base64(bytes, STANDARD, false, out),
    decode_base64
);

encoding!(
    /// Base64 with the URL-safe alphabet and padding (RFC 4648 section 5).
    ///
    /// Decoding is lenient (see [module documentation](self)).
    Base64Url,
    "base64url",
    |bytes, out| encode_base64(bytes, URL_SAFE, true, out),
    decode_base64
);

encoding!(
    /// Base64 with the URL-safe alphabet without padding.
    ///
    /// Decoding is lenient (see [module documentation](self)).
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

    #[test]
    fn test_format() {
        assert_eq!(BytesFormat::default(), BytesFormat::BASE64);
        assert_eq!(BytesFormat::encoded::<Base64>(), BytesFormat::BASE64);
        assert_ne!(BytesFormat::encoded::<Hex>(), BytesFormat::BASE64);
        assert_ne!(BytesFormat::SEQ, BytesFormat::BASE64);
        assert_eq!(format!("{:?}", BytesFormat::SEQ), "BytesFormat(\"seq\")");
        assert_eq!(BytesFormat::SEQ.decode("AQ==").unwrap(), b"\x01");
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
