use std::borrow::Cow;

use crate::error::{Error, ErrorKind};

/// An atom is a primitive value for serialization and deserialization.
///
/// Atoms are values that are sent directly to a serializer or deserializer.
/// Examples for this are booleans or integers.  This is in contrast to
/// compound values like maps, structs or sequences.
///
/// Atoms are non exhaustive which means that new variants might appear
/// in the future.  Deser tries to build around this restriction for instance
/// through APIs like [`unexpected_atom`](crate::de::Sink::unexpected_atom) so that
/// one always have something to call.
#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
pub enum Atom<'a> {
    Null,
    Bool(bool),
    Str(Cow<'a, str>),
    Bytes(Cow<'a, [u8]>),
    Char(char),
    U64(u64),
    I64(i64),
    F64(f64),
}

impl<'a> Atom<'a> {
    /// Makes a static clone of the atom decoupling the lifetimes.
    pub fn to_static(&self) -> Atom<'static> {
        match *self {
            Atom::Null => Atom::Null,
            Atom::Bool(v) => Atom::Bool(v),
            Atom::Str(ref v) => Atom::Str(Cow::Owned(v.to_string())),
            Atom::Bytes(ref v) => Atom::Bytes(Cow::Owned(v.to_vec())),
            Atom::Char(v) => Atom::Char(v),
            Atom::U64(v) => Atom::U64(v),
            Atom::I64(v) => Atom::I64(v),
            Atom::F64(v) => Atom::F64(v),
        }
    }

    /// Returns the human readable name of the atom.
    pub fn name(&self) -> &str {
        match *self {
            Atom::Null => "null",
            Atom::Bool(_) => "bool",
            Atom::Str(_) => "string",
            Atom::Bytes(_) => "bytes",
            Atom::Char(_) => "char",
            Atom::U64(_) => "unsigned integer",
            Atom::I64(_) => "signed integer",
            Atom::F64(_) => "float",
        }
    }

    /// Creates an "unexpected" error.
    ///
    /// This is useful when implementing sinks that do not want to deal with an
    /// atom of a specific type.  The default implementation of a
    /// [`Sink`](crate::de::Sink) uses this method as follows:
    ///
    /// ```
    /// # use deser::{Atom, Error, de::{DeserializerState, Sink}};
    /// # struct MySink;
    /// impl Sink for MySink {
    ///     fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
    ///         Err(atom.unexpected_error(&self.expecting()))
    ///     }
    /// }
    /// ```
    pub fn unexpected_error(&self, expectation: &str) -> Error {
        Error::new(
            ErrorKind::Unexpected,
            format!("unexpected {}, expected {}", self.name(), expectation),
        )
    }
}

macro_rules! impl_from {
    ($ty:ty, $atom:ident) => {
        impl From<$ty> for Event<'static> {
            fn from(value: $ty) -> Self {
                Event::Atom(Atom::$atom(value as _))
            }
        }
    };
}

impl_from!(u64, U64);
impl_from!(i64, I64);
impl_from!(f64, F64);
impl_from!(usize, U64);
impl_from!(isize, I64);
impl_from!(bool, Bool);
impl_from!(char, Char);

impl From<()> for Event<'static> {
    fn from(_: ()) -> Event<'static> {
        Event::Atom(Atom::Null)
    }
}

impl<'a> From<&'a str> for Event<'a> {
    fn from(value: &'a str) -> Event<'a> {
        Event::Atom(Atom::Str(Cow::Borrowed(value)))
    }
}

impl<'a> From<Cow<'a, str>> for Event<'a> {
    fn from(value: Cow<'a, str>) -> Event<'a> {
        Event::Atom(Atom::Str(value))
    }
}

impl<'a> From<&'a [u8]> for Event<'a> {
    fn from(value: &'a [u8]) -> Event<'a> {
        Event::Atom(Atom::Bytes(Cow::Borrowed(value)))
    }
}

impl From<String> for Event<'static> {
    fn from(value: String) -> Event<'static> {
        Event::Atom(Atom::Str(Cow::Owned(value)))
    }
}

impl<'a> From<Atom<'a>> for Event<'a> {
    fn from(atom: Atom<'a>) -> Self {
        Event::Atom(atom)
    }
}

/// An event represents an atomic serialization and deserialization event.
///
/// ## Serialization
///
/// [`Event`] and [`Chunk`](crate::ser::Chunk) are two close relatives.  A chunk
/// is stateful whereas [`Event`] represents a single event from a chunk.
/// Atomic chunks directly create an event whereas compound chunks keep emitting
/// more chunks which again can produce events.  To go from chunks to events use
/// the [`SerializeDriver`](crate::ser::SerializeDriver) method.
///
/// ## Deserialization
///
/// During deserialization events are passed to a
/// [`DeserializeDriver`](crate::de::DeserializeDriver) to drive the deserialization.
#[derive(Debug, PartialEq, Clone)]
pub enum Event<'a> {
    Atom(Atom<'a>),
    MapStart,
    MapEnd,
    SeqStart,
    SeqEnd,
}

impl<'a> Event<'a> {
    /// Makes a static clone of the event decoupling the lifetimes.
    pub fn to_static(&self) -> Event<'static> {
        match *self {
            Event::Atom(ref atom) => Event::Atom(atom.to_static()),
            Event::MapStart => Event::MapStart,
            Event::MapEnd => Event::MapEnd,
            Event::SeqStart => Event::SeqStart,
            Event::SeqEnd => Event::SeqEnd,
        }
    }
}
