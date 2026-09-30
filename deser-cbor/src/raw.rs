//! Raw values of CBOR.
use alloc::vec::Vec;

use deser_core::de::{self, DeserializeDriver};
use deser_core::ext::{Raw, RawFormat, RawFormatInfo};
use deser_core::{Atom, Bytes, Error, Serialize};

use crate::de::Deserializer;

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

/// The description of the format of raw CBOR values.
pub(crate) static FORMAT: RawFormatInfo =
    RawFormatInfo::new("cbor", false, replay, encode, fallback);

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
fn encode(value: &dyn Serialize) -> Result<Vec<u8>, Error> {
    crate::to_vec(value)
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
