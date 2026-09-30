#![cfg(not(hjson))]
//! Raw values of the dialect.
//!
//# Hjson has none: the text of multiline strings depends on the column
//# they start in, so values cannot be taken out of their document.
use alloc::vec::Vec;
use core::str;

use deser_core::de::{self, DeserializeDriver};
use deser_core::ext::{Raw, RawFormat, RawFormatInfo, TextRawFormat};
use deser_core::ser::SerializeRef;
use deser_core::{Atom, Error, Text};

use crate::de::Deserializer;

/// The JSON format of [`RawJson`] values.
#[cfg(not(comments))]
pub struct Json;

/// JSON text of a value.
///
/// The JSON of values deserialized from JSON is kept as it is, other
/// values are encoded as JSON.  When serialized with JSON, the text is
/// written as it is.  See [raw values](crate#raw-values) and
/// [`Raw`] for more information.
#[cfg(not(comments))]
pub type RawJson<'a> = Raw<'a, Json>;

/// The JSONC format of [`RawJsonc`] values.
#[cfg(all(comments, not(json5)))]
pub struct Jsonc;

/// JSONC text of a value.
///
/// The JSONC of values deserialized from JSONC is kept as it is, including
/// the comments in it.  Other values are encoded as JSON, which is valid
/// JSONC.  See [`Raw`] for more information.
#[cfg(all(comments, not(json5)))]
pub type RawJsonc<'a> = Raw<'a, Jsonc>;

/// The JSON5 format of [`RawJson5`] values.
#[cfg(json5)]
pub struct Json5;

/// JSON5 text of a value.
///
/// The JSON5 of values deserialized from JSON5 is kept as it is.  Other
/// values are encoded as JSON, which is valid JSON5.  See [`Raw`] for more
/// information.
#[cfg(json5)]
pub type RawJson5<'a> = Raw<'a, Json5>;

/// The format of the raw values of this dialect.
#[cfg(not(comments))]
pub(crate) type Dialect = Json;
#[cfg(all(comments, not(json5)))]
pub(crate) type Dialect = Jsonc;
#[cfg(json5)]
pub(crate) type Dialect = Json5;

/// The name of the format of the raw values of this dialect.
#[cfg(not(comments))]
const NAME: &str = "json";
#[cfg(all(comments, not(json5)))]
const NAME: &str = "jsonc";
#[cfg(json5)]
const NAME: &str = "json5";

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
fn encode(value: SerializeRef<'_>) -> Result<Vec<u8>, Error> {
    crate::to_string(&value).map(|text| text.into_bytes())
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
