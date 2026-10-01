//! The adapters and encodings for bytes (see [`adapters`](super#bytes)).
use alloc::string::String;
use alloc::vec::Vec;

use crate::BytesFormat;
use crate::State;
use crate::error::Error;

mod encodings;
mod impls;

pub use self::encodings::{Base64, Base64NoPad, Base64Url, Base64UrlNoPad};
pub use self::impls::{BytesBuf, BytesFallback, BytesFallbackFormat, IntSeq};

#[allow(unused_imports)]
pub(crate) use self::impls::{BytesBufImpl, encoded_expecting, encoded_handle, encoding_adapter};

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
///             .map(|x| {
///                 x.parse().map_err(|_| {
///                     Error::new(ErrorKind::Unexpected, "invalid byte")
///                 })
///             })
///             .collect()
///     }
/// }
/// ```
pub trait BytesEncoding: Send + Sync + 'static {
    /// The name of the encoding.
    ///
    /// The name is used in error messages and to compare [`BytesFormat`]s.
    const NAME: &'static str;

    /// Encodes bytes and appends them to the string.
    fn encode(bytes: &[u8], out: &mut String);

    /// Decodes a string.
    fn decode(s: &str) -> Result<Vec<u8>, Error>;
}

/// Decodes a string into bytes with the format in the state.
pub(crate) fn decode_str(s: &str, state: &State) -> Result<Vec<u8>, Error> {
    BytesFormat::of(state).decode(s)
}
