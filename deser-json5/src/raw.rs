// @generated from deser-template-json/src/raw.rs by
// deser-template-json/generate.py.  Do not edit.
//! Raw values of the dialect.
//!
use alloc::vec::Vec;
use core::str;

use deser_core::de::{self, DeserializeDriver};
use deser_core::ext::{Raw, RawFormat, RawFormatId, RawFormatInfo, TextRawFormat};
use deser_core::ser::SerializeRef;
use deser_core::{Atom, Error, State, Text};

use crate::de::Deserializer;
use crate::parser::Cursor;

/// The JSON5 format of [`RawJson5`] values.
pub struct Json5;

/// JSON5 text of a value.
///
/// The JSON5 of values deserialized from JSON5 is kept as it is.  Other
/// values are encoded as JSON, which is valid JSON5.  See [`Raw`] for more
/// information.
pub type RawJson5<'a> = Raw<'a, Json5>;

pub(crate) type Dialect = Json5;

const NAME: &str = "json5";

/// The identity of the format of the raw values of this dialect.
///
/// The parser and the serializer declare the raw values they pass on with
/// it, the description (with the functions) is only referred to by the raw
/// values.
pub(crate) static ID: RawFormatId = RawFormatId::new(NAME, true);

/// The description of the format of the raw values of this dialect.
static FORMAT: RawFormatInfo = {
    let mut info = RawFormatInfo::new(&ID, replay, encode, fallback);
    info.set_data(&SCANNER);
    info
};

/// Skips a raw value while validating it (see `parser::raw_value`).
///
/// The parser gets it from the description of the format, so that only
/// programs with raw values of the dialect contain it.
pub(crate) struct Scanner(pub(crate) ScanFn);

/// Skips a value at the cursor (see `parser::skip_raw`).
type ScanFn =
    for<'i> fn(&mut Cursor<'i>, &mut Vec<u8>, bool, usize, &mut State) -> Result<bool, Error>;

/// The scanner of the raw values of this dialect.
static SCANNER: Scanner = Scanner(crate::parser::skip_raw);

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
