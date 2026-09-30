//! Raw values of MessagePack.
use alloc::vec::Vec;

use deser_core::de::{self, DeserializeDriver};
use deser_core::ext::{Raw, RawFormat, RawFormatInfo};
use deser_core::ser::SerializeRef;
use deser_core::{Atom, Bytes, Error};

use crate::de::Deserializer;

/// The MessagePack format of [`RawMsgpack`] values.
pub struct Msgpack;

/// The MessagePack encoding of a value.
///
/// Values deserialized from MessagePack keep their encoding as it is
/// (including extensions and integers that are not encoded in their
/// shortest form), other values are encoded as MessagePack.  When
/// serialized with MessagePack (unless canonical output is requested), the
/// encoding is written as it is.  See [raw values](crate#raw-values) and
/// [`Raw`] for more information.
pub type RawMsgpack<'a> = Raw<'a, Msgpack>;

/// The description of the format of raw MessagePack values.
pub(crate) static FORMAT: RawFormatInfo =
    RawFormatInfo::new("msgpack", false, replay, encode, fallback);

impl RawFormat for Msgpack {
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
/// Nil and booleans are their value, everything else is its encoding as
/// bytes.
fn fallback(input: &[u8]) -> Atom<'_> {
    match input {
        [0xc0] => Atom::Null,
        [0xc2] => Atom::Bool(false),
        [0xc3] => Atom::Bool(true),
        _ => Atom::Bytes(Bytes::borrowed(input)),
    }
}
