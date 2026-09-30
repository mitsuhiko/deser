// @generated from deser-template-json/src/raw.rs by
// deser-template-json/generate.py.  Do not edit.
//! Raw values of the dialect.
//!
use alloc::vec::Vec;
use core::str;

use deser_core::de::{self, DeserializeDriver};
use deser_core::ext::{Raw, RawFormat, RawFormatInfo, TextRawFormat};
use deser_core::{Atom, Error, Serialize, Text};

use crate::de::Deserializer;

/// The JSON format of [`RawJson`] values.
pub struct Json;

/// JSON text of a value.
///
/// The JSON of values deserialized from JSON is kept as it is, other
/// values are encoded as JSON.  When serialized with JSON, the text is
/// written as it is.  See [raw values](crate#raw-values) and
/// [`Raw`] for more information.
pub type RawJson<'a> = Raw<'a, Json>;

/// The format of the raw values of this dialect.
pub(crate) type Dialect = Json;

/// The name of the format of the raw values of this dialect.
const NAME: &str = "json";

/// The description of the format of the raw values of this dialect.
pub(crate) static FORMAT: RawFormatInfo = RawFormatInfo::new(NAME, true, replay, encode, fallback);

impl RawFormat for Dialect {
    #[inline(always)]
    fn info() -> &'static RawFormatInfo {
        &FORMAT
    }
}

// SAFETY: the parser validates strings as UTF-8, everything else is ASCII,
// and the encoder writes JSON text.
unsafe impl TextRawFormat for Dialect {}

/// Parses a raw value.
fn replay<'de>(input: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
    // SAFETY: the input of raw values of text formats is valid UTF-8
    let input = unsafe { str::from_utf8_unchecked(input) };
    de::Deserializer::drive(&mut Deserializer::from_str(input), driver)
}

/// Encodes a value as JSON, which all dialects can read.
fn encode(value: &dyn Serialize) -> Result<Vec<u8>, Error> {
    crate::to_string(value).map(|text| text.into_bytes())
}

/// Returns the fallback atom of a raw value.
///
/// Null and booleans are their value, everything else is its text.
fn fallback(input: &[u8]) -> Atom<'_> {
    match input {
        b"null" => Atom::Null,
        b"true" => Atom::Bool(true),
        b"false" => Atom::Bool(false),
        // SAFETY: the input of raw values of text formats is valid UTF-8
        _ => Atom::Str(Text::borrowed(unsafe { str::from_utf8_unchecked(input) })),
    }
}
