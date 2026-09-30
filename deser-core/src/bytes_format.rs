use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::State;
use crate::adapters::bytes::decode_base64;
use crate::adapters::{Base64, BytesEncoding};
use crate::error::Error;

/// How bytes are represented in formats without native bytes.
///
/// This is used in three places:
///
/// * The serializers of formats without native bytes (JSON and TOML) can be
///   configured with a format.  It's used for all bytes that do not request
///   a format.  The default is [`BytesFormat::BASE64`].
/// * Bytes can carry a format as fallback (see
///   [`Bytes::fallback`](crate::Bytes::fallback)) which takes precedence
///   over the configuration of the serializer.  The
///   [`BytesFallback`](crate::adapters::BytesFallback) adapter does this.
/// * The types that expect bytes decode strings with the format placed into
///   the [`State`] (see [`set`](Self::set)).  The deserializers of formats
///   without native bytes can be configured to do this, otherwise lenient
///   base64 is used.  Strings are decoded as base64 for
///   [`BytesFormat::SEQ`].
///
/// ```
/// use deser::adapters::Base64UrlNoPad;
/// use deser::BytesFormat;
///
/// const URL_SAFE: BytesFormat = BytesFormat::encoded::<Base64UrlNoPad>();
/// assert_eq!(URL_SAFE.encode(b"\xfb\xff").as_deref(), Some("-_8"));
/// // decoding is lenient
/// assert_eq!(URL_SAFE.decode("+/8=").unwrap(), b"\xfb\xff");
/// assert_eq!(BytesFormat::SEQ.encode(b"\x01\xff"), None);
/// ```
///
/// Two formats are equal if they are both sequences or encodings with the
/// same [name](crate::adapters::BytesEncoding::NAME).
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

    /// Returns the format the types that expect bytes decode strings with.
    ///
    /// This is [`BytesFormat::BASE64`] unless the deserializer of the format
    /// [`set`](Self::set) a different one.
    #[inline]
    pub fn of(state: &State) -> BytesFormat {
        state.get::<BytesFormat>().copied().unwrap_or_default()
    }

    /// Sets the format the types that expect bytes decode strings with.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.get_mut::<BytesFormat>() = self;
    }

    /// Returns the name of the format.
    ///
    /// This is the [name](crate::adapters::BytesEncoding::NAME) of the encoding or `seq`.
    pub fn name(&self) -> &'static str {
        match self.0 {
            Repr::Encoded { name, .. } => name,
            Repr::Seq => "seq",
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::Base64Url;

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
