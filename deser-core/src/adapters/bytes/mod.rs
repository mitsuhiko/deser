//! The adapters and encodings for bytes (see [`adapters`](super#bytes)).
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::State;
use crate::error::Error;

mod encodings;
mod impls;

pub use self::encodings::{Base64, Base64NoPad, Base64Url, Base64UrlNoPad};
pub use self::impls::{BytesBuf, BytesFallback, BytesFallbackFormat, IntSeq};

#[allow(unused_imports)]
pub(crate) use self::impls::{BytesBufImpl, encoded_handle, encoding_adapter};

pub(crate) use self::encodings::decode_base64;

/// An encoding of bytes as string.
///
/// Encodings are types which are not instantiated.  They are used with
/// [`BytesFormat::encoded`] and every encoding is an adapter which
/// represents bytes as strings in the encoding (see the
/// [adapters documentation](super#bytes)).  With [`BytesFallback`] the encoding is
/// only used in formats without native bytes.
///
/// ```
/// use deser::adapters::BytesEncoding;
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
/// use deser::adapters::{Base64UrlNoPad, BytesFormat};
///
/// const URL_SAFE: BytesFormat = BytesFormat::encoded::<Base64UrlNoPad>();
/// assert_eq!(URL_SAFE.encode(b"\xfb\xff").as_deref(), Some("-_8"));
/// // decoding is lenient
/// assert_eq!(URL_SAFE.decode("+/8=").unwrap(), b"\xfb\xff");
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
        assert_ne!(BytesFormat::encoded::<Base64Url>(), BytesFormat::BASE64);
        assert_ne!(BytesFormat::SEQ, BytesFormat::BASE64);
        assert_eq!(format!("{:?}", BytesFormat::SEQ), "BytesFormat(\"seq\")");
        assert_eq!(BytesFormat::SEQ.decode("AQ==").unwrap(), b"\x01");
    }
}
