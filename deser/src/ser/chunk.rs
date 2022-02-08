use std::borrow::Cow;

use crate::event::Atom;
use crate::ser::{MapEmitter, SeqEmitter, StructEmitter};

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
