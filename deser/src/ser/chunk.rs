use std::borrow::Cow;

use crate::event::Atom;
use crate::ser::{MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};

/// A chunk represents the minimum state necessary to serialize a value.
///
/// Chunks are of two types: atomic primitives and stateful emitters.
/// For instance `Chunk::Bool(true)` is an atomic primitive.  It can be emitted
/// to a serializer directly.  On the other hand a `Chunk::Map` contains a
/// stateful emitter that keeps yielding values until it's done walking over
/// the map.
pub enum Chunk<'a> {
    Atom(Atom<'a>),
    Struct(Box<dyn StructEmitter + 'a>),
    Map(Box<dyn MapEmitter + 'a>),
    Seq(Box<dyn SeqEmitter + 'a>),
    /// Serializes another value in place of this one.
    ///
    /// The driver serializes the value in the handle as if it was produced
    /// instead of the value that returned the chunk.  This is useful to
    /// serialize a value by converting it into another value first as the
    /// handle can own that value:
    ///
    /// ```
    /// use deser::ser::{Chunk, Serialize, SerializeHandle};
    /// use deser::{Error, State};
    ///
    /// struct Point(u32, u32);
    ///
    /// impl Serialize for Point {
    ///     fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
    ///         // serialize as a vector
    ///         Ok(Chunk::Forward(SerializeHandle::boxed(vec![self.0, self.1])))
    ///     }
    /// }
    /// ```
    ///
    /// The [`descriptor`](crate::ser::Serialize::descriptor) of the value that
    /// returned the chunk is not used, the forwarded value provides its own.
    /// [`finish`](crate::ser::Serialize::finish) is invoked on the forwarded
    /// value first and then on the value that returned the chunk.
    ///
    /// Forwarding chunks cannot be flattened into structs.
    Forward(SerializeHandle<'a>),
}

impl<'a> From<Atom<'a>> for Chunk<'a> {
    fn from(atom: Atom<'a>) -> Self {
        Chunk::Atom(atom)
    }
}

macro_rules! impl_from {
    ($ty:ty, $atom:ident) => {
        impl From<$ty> for Chunk<'static> {
            fn from(value: $ty) -> Self {
                Chunk::Atom(Atom::$atom(value as _))
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

impl From<()> for Chunk<'static> {
    fn from(_: ()) -> Chunk<'static> {
        Chunk::Atom(Atom::Null)
    }
}

impl<'a> From<&'a str> for Chunk<'a> {
    fn from(value: &'a str) -> Chunk<'a> {
        Chunk::Atom(Atom::Str(Cow::Borrowed(value)))
    }
}

impl<'a> From<&'a [u8]> for Chunk<'a> {
    fn from(value: &'a [u8]) -> Chunk<'a> {
        Chunk::Atom(Atom::Bytes(Cow::Borrowed(value)))
    }
}

impl From<String> for Chunk<'static> {
    fn from(value: String) -> Chunk<'static> {
        Chunk::Atom(Atom::Str(Cow::Owned(value)))
    }
}
