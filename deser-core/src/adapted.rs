//! Values, sinks and emitters with an adapter.
//!
//! The implementations that are generic over adapters (like `Vec<A>` for
//! `Vec<T>`, see [`adapters`](crate::adapters)) create sinks and emitters
//! which use the functions of the adapter `A`.  Their trait objects (and
//! the trait objects of values that serialize with an adapter) have to
//! outlive the slot or the value, and the compiler requires the adapter to
//! outlive it as well.  It always does: it's either the type of the value
//! (which outlives it) or a marker type.  But this cannot be expressed: the
//! implementations only know that the value outlives the slot.
//!
//! [`Adapted`] separates the data from the adapter.  The data is a type
//! that is not generic over the adapter, the compiler checks that it
//! outlives the trait object.  The adapter is a marker of the wrapper, the
//! implementations of the traits for the wrapper use its functions.  The
//! functions here create the trait objects without requiring the adapter
//! to outlive them.  This is the only place where this happens, sinks and
//! emitters with adapters are written in safe code.  The
//! [`Slot`](crate::de::Slot) of values that are deserialized from atoms
//! is a wrapper like this as well (around the `Option<T>`).
//!
//! This is sound because no value of the adapter exists: the wrappers only
//! hold a `PhantomData<fn() -> A>`, which has no data and no drop glue,
//! and as the data is not generic over the adapter it cannot hold values
//! of it or data that borrows for its lifetimes either.  The functions of
//! the adapter cannot hold borrowed data or create data that borrows for
//! the lifetimes of the adapter without receiving such data.
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use crate::State;
use crate::arena::ArenaBox;
use crate::de::{Sink, SinkHandle, Slot};
use crate::ser::boxed::unsize;
use crate::ser::{Boxed, Emit, Erased, IndexedSeq, MapEmitter, SeqEmitter};

/// Data (a value, a sink or an emitter) together with an adapter.
///
/// The wrapper is transparent over the data, it can be created from a
/// reference to the data without copying it.  The adapter is only a
/// marker, it's never instantiated.  The wrapper dereferences to the data.
#[repr(transparent)]
pub(crate) struct Adapted<A, T: ?Sized> {
    _adapter: PhantomData<fn() -> A>,
    value: T,
}

impl<A, T: ?Sized> Adapted<A, T> {
    /// Wraps a reference to a value.
    #[inline(always)]
    pub(crate) fn new(value: &T) -> &Adapted<A, T> {
        // SAFETY: the wrapper is transparent over `T` (the marker is zero
        // sized and has an alignment of one).
        unsafe { &*(value as *const T as *const Adapted<A, T>) }
    }

    /// Returns the wrapped value.
    #[inline(always)]
    pub(crate) fn get(&self) -> &T {
        &self.value
    }
}

impl<A, T> Adapted<A, T> {
    /// Wraps a value.
    #[inline(always)]
    pub(crate) fn owned(value: T) -> Adapted<A, T> {
        Adapted {
            _adapter: PhantomData,
            value,
        }
    }

    /// Returns a value that serializes with the adapter.
    ///
    /// Unlike [`new`](Self::new) this does not require the adapter to
    /// outlive the value.
    #[inline(always)]
    pub(crate) fn erased<'a>(value: &'a T) -> &'a (dyn Erased + 'a)
    where
        Adapted<A, T>: Erased,
    {
        // like every type parameter, the adapter outlives this function
        let value: *const (dyn Erased + '_) = value as *const T as *const Adapted<A, T>;
        // SAFETY: the wrapper is transparent over `T`, which outlives
        // `'a`.  The adapter does not need to, see the module
        // documentation.
        unsafe {
            &*core::mem::transmute::<*const (dyn Erased + '_), *const (dyn Erased + 'a)>(value)
        }
    }

    /// Returns a sequence that provides the elements of a value with the
    /// adapter.
    ///
    /// Unlike [`new`](Self::new) this does not require the adapter to
    /// outlive the value.
    #[inline(always)]
    pub(crate) fn indexed<'a>(value: &'a T) -> &'a (dyn IndexedSeq + 'a)
    where
        Adapted<A, T>: IndexedSeq,
    {
        // like every type parameter, the adapter outlives this function
        let value: *const (dyn IndexedSeq + '_) = value as *const T as *const Adapted<A, T>;
        // SAFETY: see `erased`
        unsafe {
            &*core::mem::transmute::<*const (dyn IndexedSeq + '_), *const (dyn IndexedSeq + 'a)>(
                value,
            )
        }
    }
}

impl<A, T: ?Sized> Deref for Adapted<A, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<A, T: ?Sized> DerefMut for Adapted<A, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

/// Returns a handle to the slot of the `Option<T>`, its adapter is `A`.
///
/// The sink is the [`Slot`], which is transparent over the option like
/// [`Adapted`] is over its data.  The adapter does not need to outlive
/// the handle.
#[inline(always)]
pub(crate) fn slot_sink<'a, 'de, T, A>(out: &'a mut Option<T>) -> SinkHandle<'a, 'de>
where
    Slot<T, A>: Sink<'de>,
{
    // like every type parameter, the adapter outlives this function
    let sink: *mut (dyn Sink<'de> + '_) = out as *mut Option<T> as *mut Slot<T, A>;
    // SAFETY: the slot is a transparent wrapper around the option which
    // is borrowed for `'a`.  The adapter does not need to outlive it, see
    // the module documentation.
    SinkHandle::to(unsafe {
        &mut *core::mem::transmute::<*mut (dyn Sink<'de> + '_), *mut (dyn Sink<'de> + 'a)>(sink)
    })
}

/// Moves a sink into the arena of the state, its adapter is `A`.
///
/// The sink is `S` (which outlives the handle), the adapter does not need
/// to outlive the handle.
#[inline(always)]
pub(crate) fn arena_sink<'a, 'de, A, S: 'a>(sink: S, state: &mut State) -> SinkHandle<'a, 'de>
where
    Adapted<A, S>: Sink<'de>,
{
    let sink = ArenaBox::into_raw(ArenaBox::new(
        Adapted::<A, S>::owned(sink),
        &mut state.arena,
    ));
    // like every type parameter, the adapter outlives this function
    let sink: *mut (dyn Sink<'de> + '_) = sink.as_ptr();
    // SAFETY: the pointer comes from the box.  The sink outlives `'a`, the
    // adapter does not need to, see the module documentation.
    let sink = unsafe {
        ArenaBox::from_raw(core::mem::transmute::<
            *mut (dyn Sink<'de> + '_),
            *mut (dyn Sink<'de> + 'a),
        >(sink))
    };
    SinkHandle::from_arena(sink)
}

/// Emits a sequence with an emitter in the arena of the state, its adapter
/// is `A`.
///
/// The emitter is `E` (which outlives the `Emit`), the adapter does not
/// need to outlive the `Emit`.
#[inline(always)]
pub(crate) fn seq_emitter<'a, A, E: 'a>(emitter: E, state: &mut State) -> Emit<'a>
where
    Adapted<A, E>: SeqEmitter,
{
    // like every type parameter, the adapter outlives this function
    let emitter = unsize(Boxed::arena(Adapted::<A, E>::owned(emitter), state), |x| {
        x as *mut (dyn SeqEmitter + '_)
    });
    // SAFETY: the emitter outlives `'a`, the adapter does not need to, see
    // the module documentation.
    Emit::Seq(unsafe {
        core::mem::transmute::<Boxed<dyn SeqEmitter + '_>, Boxed<dyn SeqEmitter + 'a>>(emitter)
    })
}

/// Emits a map with an emitter in the arena of the state, its adapters are
/// `A`.
///
/// See [`seq_emitter`].
#[inline(always)]
pub(crate) fn map_emitter<'a, A, E: 'a>(emitter: E, state: &mut State) -> Emit<'a>
where
    Adapted<A, E>: MapEmitter,
{
    // like every type parameter, the adapter outlives this function
    let emitter = unsize(Boxed::arena(Adapted::<A, E>::owned(emitter), state), |x| {
        x as *mut (dyn MapEmitter + '_)
    });
    // SAFETY: see `seq_emitter`
    Emit::Map(unsafe {
        core::mem::transmute::<Boxed<dyn MapEmitter + '_>, Boxed<dyn MapEmitter + 'a>>(emitter)
    })
}
