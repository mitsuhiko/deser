//! Raw values of CBOR.
use alloc::vec::Vec;

use deser_core::de::{self, DeserializeDriver};
use deser_core::ext::{Raw, RawFormat, RawFormatId, RawFormatInfo};
use deser_core::ser::SerializeRef;
use deser_core::{Atom, Bytes, Error};

use crate::de::Deserializer;
use crate::parser::Cursor;

/// The CBOR format of [`RawCbor`] values.
pub struct Cbor;

/// The CBOR encoding of a value.
///
/// Values deserialized from CBOR keep their encoding as it is (including
/// their tags), other values are encoded as CBOR.  When serialized with
/// CBOR (unless canonical output is requested), the encoding is written as
/// it is.  See [raw values](crate#raw-values) and [`Raw`] for more
/// information.
pub type RawCbor<'a> = Raw<'a, Cbor>;

/// The identity of the format of raw CBOR values.
///
/// The parser and the serializer declare the raw values they pass on with
/// it, the description (with the functions) is only referred to by the raw
/// values.
pub(crate) static ID: RawFormatId = RawFormatId::new("cbor", false);

/// The description of the format of raw CBOR values.
static FORMAT: RawFormatInfo = {
    let mut info = RawFormatInfo::new(&ID, replay, encode, fallback);
    info.set_data(&SCANNER);
    info
};

/// Skips an item while validating it (see `Parser::raw_values`).
///
/// The parser gets it from the description of the format, so that only
/// programs with raw values of the format contain it.
pub(crate) struct Scanner(pub(crate) ScanFn);

/// Skips the item at the cursor (see `parser::skip_raw`).
type ScanFn = for<'i> fn(&mut Cursor<'i>, &mut Vec<u8>) -> Result<(), Error>;

/// The scanner of raw values.
static SCANNER: Scanner = Scanner(crate::parser::skip_raw);

impl RawFormat for Cbor {
    #[inline(always)]
    fn info() -> &'static RawFormatInfo {
        &FORMAT
    }
}

/// Parses a raw value.
fn replay<'de>(input: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
    de::Deserializer::drive(&mut Deserializer::from_slice(input), driver)
}

/// Encodes a value.
fn encode(value: SerializeRef<'_>) -> Result<Vec<u8>, Error> {
    crate::to_vec(&value)
}

/// Returns the fallback atom of a raw value.
///
/// Null (and undefined) and booleans are their value, everything else is
/// its encoding as bytes.
fn fallback(input: &[u8]) -> Atom<'_> {
    match input {
        [0xf6 | 0xf7] => Atom::Null,
        [0xf4] => Atom::Bool(false),
        [0xf5] => Atom::Bool(true),
        _ => Atom::Bytes(Bytes::borrowed(input)),
    }
}
