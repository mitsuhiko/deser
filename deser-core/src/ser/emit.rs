use crate::event::{Atom, Bytes};
use crate::ser::boxed::unsize;
use crate::ser::{Boxed, MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};
use crate::{State, Text};
use alloc::string::String;

/// Describes how a value is emitted, returned by
/// [`Serialize::serialize`](crate::ser::Serialize::serialize).
///
/// A value is either emitted as an atom, by a stateful emitter or by
/// forwarding to another value.  For instance `Emit::Atom(Atom::Bool(true))`
/// is emitted to a serializer directly.  On the other hand an `Emit::Map`
/// contains a stateful emitter that keeps yielding values until it's done
/// walking over the map.
///
/// The emitters are typically allocated in the arena of the state
/// with [`Emit::seq`], [`Emit::map`] and [`Emit::structure`] (see
/// [`Boxed`]).  They are the serialization equivalent of the
/// [`Sink`](crate::de::Sink)s of deserialization: they hold the state of a
/// value that is being serialized.
pub enum Emit<'a> {
    Atom(Atom<'a>),
    Struct(Boxed<dyn StructEmitter + 'a>),
    Map(Boxed<dyn MapEmitter + 'a>),
    Seq(Boxed<dyn SeqEmitter + 'a>),
    /// Serializes another value in place of this one.
    ///
    /// The driver serializes the value in the handle as if it was produced
    /// instead of the value that returned the `Emit`.  This is useful to
    /// serialize a value by converting it into another value first as the
    /// handle can own that value:
    ///
    /// ```
    /// use deser::ser::{Emit, Serialize, SerializeHandle};
    /// use deser::{Error, State};
    ///
    /// struct Point(u32, u32);
    ///
    /// impl Serialize for Point {
    ///     fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
    ///         // serialize as a vector
    ///         Ok(Emit::Forward(SerializeHandle::arena(
    ///             vec![value.0, value.1],
    ///             state,
    ///         )))
    ///     }
    /// }
    /// ```
    ///
    /// The [`container_shape`](crate::ser::Serialize::container_shape) of the
    /// value that returned the `Emit` is not used, the forwarded value provides
    /// its own.
    /// [`finish`](crate::ser::Serialize::finish) is invoked on the forwarded
    /// value first and then on the value that returned the `Emit`.
    ///
    /// Values that forward cannot be flattened into structs.
    Forward(SerializeHandle<'a>),
}

impl<'a> Emit<'a> {
    /// Emits a sequence with an emitter in the arena of the
    /// serialization.
    #[inline(always)]
    pub fn seq<E: SeqEmitter + 'a>(emitter: E, state: &mut State) -> Emit<'a> {
        Emit::Seq(unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn SeqEmitter + 'a)
        }))
    }

    /// Emits a map with an emitter in the arena of the state.
    #[inline(always)]
    pub fn map<E: MapEmitter + 'a>(emitter: E, state: &mut State) -> Emit<'a> {
        Emit::Map(unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn MapEmitter + 'a)
        }))
    }

    /// Emits a struct with an emitter in the arena of the
    /// serialization.
    #[inline(always)]
    pub fn structure<E: StructEmitter + 'a>(emitter: E, state: &mut State) -> Emit<'a> {
        Emit::Struct(unsize(Boxed::arena(emitter, state), |x| {
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
    pub(crate) unsafe fn seq_unbounded<E: SeqEmitter>(emitter: E, state: &mut State) -> Emit<'a> {
        // like every type parameter, `E` outlives this function
        let emitter = unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn SeqEmitter + '_)
        });
        // SAFETY: guaranteed by the caller
        Emit::Seq(unsafe {
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
    pub(crate) unsafe fn map_unbounded<E: MapEmitter>(emitter: E, state: &mut State) -> Emit<'a> {
        // like every type parameter, `E` outlives this function
        let emitter = unsize(Boxed::arena(emitter, state), |x| {
            x as *mut (dyn MapEmitter + '_)
        });
        // SAFETY: guaranteed by the caller
        Emit::Map(unsafe {
            core::mem::transmute::<Boxed<dyn MapEmitter + '_>, Boxed<dyn MapEmitter + 'a>>(emitter)
        })
    }
}

impl<'a> From<Atom<'a>> for Emit<'a> {
    fn from(atom: Atom<'a>) -> Self {
        Emit::Atom(atom)
    }
}

macro_rules! impl_from {
    ($ty:ty, $atom:ident) => {
        impl From<$ty> for Emit<'static> {
            fn from(value: $ty) -> Self {
                Emit::Atom(Atom::$atom(value as _))
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

impl From<f64> for Emit<'static> {
    fn from(value: f64) -> Self {
        Emit::Atom(Atom::F64(value))
    }
}

impl From<f32> for Emit<'static> {
    fn from(value: f32) -> Self {
        Emit::Atom(Atom::F32(value))
    }
}

impl From<()> for Emit<'static> {
    fn from(_: ()) -> Emit<'static> {
        Emit::Atom(Atom::Null)
    }
}

impl<'a> From<&'a str> for Emit<'a> {
    fn from(value: &'a str) -> Emit<'a> {
        Emit::Atom(Atom::Str(Text::borrowed(value)))
    }
}

impl<'a> From<&'a [u8]> for Emit<'a> {
    fn from(value: &'a [u8]) -> Emit<'a> {
        Emit::Atom(Atom::Bytes(Bytes::borrowed(value)))
    }
}

impl From<String> for Emit<'static> {
    fn from(value: String) -> Emit<'static> {
        Emit::Atom(Atom::Str(Text::owned(value)))
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<Emit<'static>>() == 32);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<Emit<'static>>() == 24);
