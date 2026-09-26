//! Adapters for bytes and how bytes are represented in formats without
//! native bytes.
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
//! # Adapters
//!
//! How the bytes of an individual value are represented can be changed with
//! the adapters of this module.  They support `Vec<u8>`, `[u8; N]` and
//! `Cow<[u8]>` (see [`BytesBuf`]).
//!
//! * The encodings (such as [`Hex`]) are adapters which represent bytes as
//!   strings in the encoding in all formats, also in formats with native
//!   bytes.
//! * [`BytesFallback`] keeps bytes as bytes in formats with native bytes and
//!   only picks the representation for formats without them.  This can be
//!   an encoding (`BytesFallback<Hex>`) or sequences of integers
//!   (`BytesFallback<IntSeq>`).
//!
//! ```
//! use deser::adapters::bytes::{BytesFallback, Hex, IntSeq};
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct Blob {
//!     // base64 in JSON and TOML, bytes in CBOR
//!     data: Vec<u8>,
//!     // a hex string in all formats
//!     #[deser(as = Hex)]
//!     sha256: [u8; 32],
//!     // hex in JSON and TOML, bytes in CBOR
//!     #[deser(as = BytesFallback<Hex>)]
//!     signature: Vec<u8>,
//!     // `[1, 2]` in JSON and TOML, bytes in CBOR
//!     #[deser(as = BytesFallback<IntSeq>)]
//!     legacy: Vec<u8>,
//! }
//! ```
//!
//! When deserializing, all of these accept native bytes and strings in
//! their encoding.
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
//! Other encodings can be added by implementing [`BytesEncoding`].  They
//! are adapters like the encodings provided by deser.
use std::fmt;

use crate::State;
use crate::error::Error;

mod encodings;
mod impls;

pub use self::encodings::{Base64, Base64NoPad, Base64Url, Base64UrlNoPad, Hex, HexUpper};
pub use self::impls::{BytesBuf, BytesFallback, BytesFallbackFormat, IntSeq};

#[cfg(feature = "bytes-encoding")]
pub use self::encodings::{Base32, Base32Hex, Base32HexNoPad, Base32NoPad};

pub(crate) use self::encodings::decode_base64;

/// An encoding of bytes as string.
///
/// Encodings are types which are not instantiated.  They are used with
/// [`BytesFormat::encoded`] and every encoding is an adapter which
/// represents bytes as strings in the encoding (see the
/// [module documentation](self)).  With [`BytesFallback`] the encoding is
/// only used in formats without native bytes.
///
/// ```
/// use deser::adapters::bytes::BytesEncoding;
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
/// * Bytes can carry a format as fallback (see
///   [`Bytes::fallback`](crate::Bytes::fallback)) which takes precedence
///   over the configuration of the serializer.  The [`BytesFallback`]
///   adapter does this.
/// * The types that expect bytes decode strings with the format placed into
///   the [`State`].  The deserializers of formats without native bytes can
///   be configured to do this, otherwise lenient base64 is used.  Strings
///   are decoded as base64 for [`BytesFormat::SEQ`].
///
/// ```
/// use deser::adapters::bytes::{BytesFormat, Hex};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format() {
        assert_eq!(BytesFormat::default(), BytesFormat::BASE64);
        assert_eq!(BytesFormat::encoded::<Base64>(), BytesFormat::BASE64);
        assert_ne!(BytesFormat::encoded::<Hex>(), BytesFormat::BASE64);
        assert_ne!(BytesFormat::SEQ, BytesFormat::BASE64);
        assert_eq!(format!("{:?}", BytesFormat::SEQ), "BytesFormat(\"seq\")");
        assert_eq!(BytesFormat::SEQ.decode("AQ==").unwrap(), b"\x01");
    }
}
