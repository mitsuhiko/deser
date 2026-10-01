use crate::event::{Atom, Bytes};
use crate::ser::boxed::unsize;
use crate::ser::{Boxed, MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};
use crate::{State, Text};
use alloc::string::String;

/// A chunk represents the minimum state necessary to serialize a value.
///
/// Chunks are of two types: atomic primitives and stateful emitters.
/// For instance `Chunk::Bool(true)` is an atomic primitive.  It can be emitted
/// to a serializer directly.  On the other hand a `Chunk::Map` contains a
/// stateful emitter that keeps yielding values until it's done walking over
/// the map.
///
/// The emitters are typically allocated in the arena of the state
/// with [`Chunk::seq`], [`Chunk::map`] and [`Chunk::structure`] (see
/// [`Boxed`]).  They are the serialization equivalent of the
/// [`Sink`](crate::de::Sink)s of deserialization: they hold the state of a
/// value that is being serialized.
pub enum Chunk<'a> {
    Atom(Atom<'a>),
    Struct(Boxed<dyn StructEmitter + 'a>),
    Map(Boxed<dyn MapEmitter + 'a>),
    Seq(Boxed<dyn SeqEmitter + 'a>),
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
    ///     fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
    ///         // serialize as a vector
    ///         Ok(Chunk::Forward(SerializeHandle::arena(
    ///             vec![value.0, value.1],
    ///             state,
    ///         )))
    ///     }
    /// }
    /// ```
    ///
    /// The [`container_shape`](crate::ser::Serialize::container_shape) of the
    /// value that returned the chunk is not used, the forwarded value provides
    /// its own.
    /// [`finish`](crate::ser::Serialize::finish) is invoked on the forwarded
    /// value first and then on the value that returned the chunk.
    ///
    /// Forwarding chunks cannot be flattened into structs.
    Forward(SerializeHandle<'a>),
}

impl<'a> Chunk<'a> {
    /// Creates a chunk of a sequence emitter in the arena of the
    /// serialization.
    #[inline(always)]
    pub fn seq<E: SeqEmitter + 'a>(emitter: E, state: &mut State) -> Chunk<'a> {
        Chunk::Seq(unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn SeqEmitter + 'a)
        }))
    }

    /// Creates a chunk of a map emitter in the arena of the state.
    #[inline(always)]
    pub fn map<E: MapEmitter + 'a>(emitter: E, state: &mut State) -> Chunk<'a> {
        Chunk::Map(unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn MapEmitter + 'a)
        }))
    }

    /// Creates a chunk of a struct emitter in the arena of the
    /// serialization.
    #[inline(always)]
    pub fn structure<E: StructEmitter + 'a>(emitter: E, state: &mut State) -> Chunk<'a> {
        Chunk::Struct(unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn StructEmitter + 'a)
        }))
    }

    /// Like [`seq`](Self::seq) but the emitter does not need to outlive
    /// `'a`.
    ///
    /// This is for the emitters of sequences which are generic over
    /// adapters (see [`erase_unbounded`](crate::ser::erase_unbounded)).
    ///
    /// # Safety
    ///
    /// The parts of `E` that do not outlive `'a` must be adapters that are
    /// only used for their functions, `E` holds no values of them.
    #[inline(always)]
    pub(crate) unsafe fn seq_unbounded<E: SeqEmitter>(emitter: E, state: &mut State) -> Chunk<'a> {
        // like every type parameter, `E` outlives this function
        let emitter = unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn SeqEmitter + '_)
        });
        // SAFETY: guaranteed by the caller
        Chunk::Seq(unsafe {
            core::mem::transmute::<Boxed<dyn SeqEmitter + '_>, Boxed<dyn SeqEmitter + 'a>>(emitter)
        })
    }

    /// Like [`map`](Self::map) but the emitter does not need to outlive
    /// `'a`.
    ///
    /// # Safety
    ///
    /// See [`seq_unbounded`](Self::seq_unbounded).
    #[inline(always)]
    pub(crate) unsafe fn map_unbounded<E: MapEmitter>(emitter: E, state: &mut State) -> Chunk<'a> {
        // like every type parameter, `E` outlives this function
        let emitter = unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn MapEmitter + '_)
        });
        // SAFETY: guaranteed by the caller
        Chunk::Map(unsafe {
            core::mem::transmute::<Boxed<dyn MapEmitter + '_>, Boxed<dyn MapEmitter + 'a>>(emitter)
        })
    }
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
impl_from!(usize, U64);
impl_from!(isize, I64);
impl_from!(bool, Bool);
impl_from!(char, Char);

impl From<f64> for Chunk<'static> {
    fn from(value: f64) -> Self {
        Chunk::Atom(Atom::F64(value))
    }
}

impl From<f32> for Chunk<'static> {
    fn from(value: f32) -> Self {
        Chunk::Atom(Atom::F32(value))
    }
}

impl From<()> for Chunk<'static> {
    fn from(_: ()) -> Chunk<'static> {
        Chunk::Atom(Atom::Null)
    }
}

impl<'a> From<&'a str> for Chunk<'a> {
    fn from(value: &'a str) -> Chunk<'a> {
        Chunk::Atom(Atom::Str(Text::borrowed(value)))
    }
}

impl<'a> From<&'a [u8]> for Chunk<'a> {
    fn from(value: &'a [u8]) -> Chunk<'a> {
        Chunk::Atom(Atom::Bytes(Bytes::borrowed(value)))
    }
}

impl From<String> for Chunk<'static> {
    fn from(value: String) -> Chunk<'static> {
        Chunk::Atom(Atom::Str(Text::owned(value)))
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<Chunk<'static>>() == 32);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<Chunk<'static>>() == 24);
