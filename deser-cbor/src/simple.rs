use std::borrow::Cow;

use deser::State;
use deser::de::{Deserialize, Sink, SinkHandle};
use deser::ext::{ExtValue, Extension};
use deser::ser::{Chunk, Serialize};
use deser::{Atom, Error, ErrorKind};

/// A CBOR simple value.
///
/// The simple values `false`, `true`, `null` and `undefined` (20 to 23) map
/// onto the deser data model (`undefined` is deserialized as `null`).  All
/// other simple values are unassigned and passed through deser as extension
/// atoms of this type.  Their fallback is an unsigned integer.
///
/// ```
/// use deser_cbor::Simple;
///
/// let bytes = deser_cbor::to_vec(&Simple::new(16).unwrap()).unwrap();
/// assert_eq!(bytes, [0xf0]);
/// let value: Simple = deser_cbor::from_slice(&bytes).unwrap();
/// assert_eq!(value.value(), 16);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Simple(u8);

impl Simple {
    /// Creates a simple value.
    ///
    /// Returns `None` for the reserved values 24 to 31 which have no valid
    /// encoding.
    pub fn new(value: u8) -> Option<Simple> {
        if (24..32).contains(&value) {
            None
        } else {
            Some(Simple(value))
        }
    }

    /// Returns the numeric value.
    pub fn value(self) -> u8 {
        self.0
    }
}

impl Extension for Simple {
    fn name(&self) -> &str {
        "simple value"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::U64(u64::from(self.0))
    }
}

impl Serialize for Simple {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
    }
}

impl<'de> Deserialize<'de> for Simple {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(SimpleSink(out))
    }
}

struct SimpleSink<'a>(&'a mut Option<Simple>);

impl<'a, 'de> Sink<'de> for SimpleSink<'a> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("simple")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let simple = match atom {
            Atom::Ext(ref ext) if ext.is::<Simple>() => *ext.downcast_ref::<Simple>().unwrap(),
            Atom::Bool(false) => Simple(20),
            Atom::Bool(true) => Simple(21),
            Atom::Null => Simple(22),
            Atom::U64(value) => match u8::try_from(value).ok().and_then(Simple::new) {
                Some(simple) => simple,
                None => {
                    return Err(Error::new(
                        ErrorKind::OutOfRange,
                        "value out of range for simple value",
                    ));
                }
            },
            other => return self.unexpected_atom(other, state),
        };
        *self.0 = Some(simple);
        Ok(())
    }
}
