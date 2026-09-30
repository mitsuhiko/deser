use alloc::borrow::Cow;

use deser_core::State;
use deser_core::de::{Deserialize, Sink, SinkHandle};
use deser_core::ext::{ExtValue, Extension};
use deser_core::ser::{Chunk, Serialize};
use deser_core::{Atom, Error};

/// A UID of a property list.
///
/// UIDs are references between the objects of archives created with
/// `NSKeyedArchiver`.  Binary property lists have a type for them, the
/// text formats write them as dictionaries with a single `CF$UID` key
/// (`<dict><key>CF$UID</key><integer>1</integer></dict>`) which are read
/// back as UIDs.
///
/// UIDs are passed through deser as extension atoms of this type.  Their
/// fallback is the integer, so a UID deserializes into integer types if the
/// distinction is not of interest.  Values of this type accept integers as
/// well.
///
/// ```
/// use deser_plist::{Format, SerializerConfig, Uid};
///
/// let config = SerializerConfig::new().format(Format::Binary);
/// let bytes = config.to_vec(&vec![Uid::new(1), Uid::new(2)]).unwrap();
/// let value: Vec<Uid> = deser_plist::from_slice(&bytes).unwrap();
/// assert_eq!(value, [Uid::new(1), Uid::new(2)]);
/// let value: Vec<u64> = deser_plist::from_slice(&bytes).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uid(u64);

impl Uid {
    /// Creates a UID.
    pub const fn new(value: u64) -> Uid {
        Uid(value)
    }

    /// Returns the value of the UID.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for Uid {
    fn from(value: u64) -> Uid {
        Uid(value)
    }
}

impl From<Uid> for u64 {
    fn from(value: Uid) -> u64 {
        value.0
    }
}

impl Extension for Uid {
    fn name(&self) -> &str {
        "plist uid"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::U64(self.0)
    }
}

impl Serialize for Uid {
    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(value))))
    }
}

impl<'de> Deserialize<'de> for Uid {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(UidSink(out), state)
    }
}

struct UidSink<'a>(&'a mut Option<Uid>);

impl<'a, 'de> Sink<'de> for UidSink<'a> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("plist uid")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let value = match atom {
            Atom::Ext(ref ext) => match ext.downcast_ref::<Uid>() {
                Some(&value) => value,
                None => return self.unexpected_atom(atom, state),
            },
            Atom::U64(value) => Uid(value),
            other => return self.unexpected_atom(other, state),
        };
        *self.0 = Some(value);
        Ok(())
    }
}
