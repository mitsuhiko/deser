//! References to values that are serialized with their type erased.
use alloc::boxed::Box;
use core::fmt;
use core::marker::PhantomData;

use crate::State;
use crate::error::Error;
use crate::event::ContainerShape;
use crate::ser::boxed::{self, Boxed};
use crate::ser::{Begin, Describe, Emit, PlainSink, Serialize};

/// A value that serializes with an adapter.
///
/// The wrapper is transparent over the value, it can be created from a
/// reference to the value without copying it.  The adapter is only a
/// marker, it's never instantiated.
#[repr(transparent)]
pub(crate) struct Adapted<A, T: ?Sized> {
    _marker: PhantomData<fn() -> A>,
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

    /// Returns a pointer to the wrapper of a value.
    ///
    /// Unlike a reference, the pointer does not require the adapter to
    /// outlive it.
    #[inline(always)]
    pub(crate) fn ptr(value: &T) -> *const Adapted<A, T> {
        value as *const T as *const Adapted<A, T>
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
    fn owned(value: T) -> Adapted<A, T> {
        Adapted {
            _marker: PhantomData,
            value,
        }
    }
}

/// A value that is serialized, with its type erased.
///
/// This is the trait object behind [`SerializeRef`] and
/// [`SerializeHandle`], it's implemented for [`Adapted`].
pub(crate) trait Erased: Sync {
    fn erased_serialize(&self, state: &mut State) -> Result<Emit<'_>, Error>;
    fn erased_finish(&self, state: &mut State) -> Result<(), Error>;
    fn erased_is_optional(&self) -> bool;
    fn erased_container_shape(&self) -> ContainerShape;
    fn erased_describe(&self, d: &mut dyn Describe);
    fn erased_begin(&self, state: &mut State) -> Result<Begin<'_>, Error>;
    fn erased_is_plain_value(&self) -> bool;
    fn erased_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error>;
    fn erased_plain_cost(&self, budget: usize) -> Option<usize>;
}

impl<A: Serialize<T>, T: ?Sized + Sync> Erased for Adapted<A, T> {
    fn erased_serialize(&self, state: &mut State) -> Result<Emit<'_>, Error> {
        A::serialize(&self.value, state)
    }

    fn erased_finish(&self, state: &mut State) -> Result<(), Error> {
        A::finish(&self.value, state)
    }

    fn erased_is_optional(&self) -> bool {
        A::is_optional(&self.value)
    }

    fn erased_container_shape(&self) -> ContainerShape {
        A::container_shape(&self.value)
    }

    fn erased_describe(&self, d: &mut dyn Describe) {
        A::describe(&self.value, d)
    }

    fn erased_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        A::__private_begin(&self.value, state)
    }

    fn erased_is_plain_value(&self) -> bool {
        A::__private_is_plain_value(&self.value)
    }

    fn erased_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        A::__private_emit_plain(&self.value, sink)
    }

    fn erased_plain_cost(&self, budget: usize) -> Option<usize> {
        A::__private_plain_cost(&self.value, budget)
    }
}

/// Converts a pointer to a wrapper into a trait object.
///
/// The compiler requires the wrapper, and with it the adapter, to outlive
/// the trait object (even a reference to it).  The adapter always does
/// (it's the type of the value or a marker type) but this cannot be
/// expressed for adapters that are type parameters, like the element
/// adapter of `Vec<A>`.
///
/// # Safety
///
/// The pointer must be valid for `'a`.  The parts of `S` that do not
/// outlive `'a` must be adapters that are only used for their functions
/// (which cannot hold borrowed data), `S` holds no values of them.
#[inline(always)]
pub(crate) unsafe fn erase_unbounded<'a, S: Erased>(value: *const S) -> &'a (dyn Erased + 'a) {
    // like every type parameter, `S` outlives this function
    let value: *const (dyn Erased + '_) = value;
    // SAFETY: guaranteed by the caller
    unsafe { &*core::mem::transmute::<*const (dyn Erased + '_), *const (dyn Erased + 'a)>(value) }
}

/// A reference to a value that is serialized.
///
/// This is the type erased form of a [`Serialize`] value (and an adapter
/// if it's serialized with one).  The serializers of the data formats
/// receive the values as references, for instance from the
/// [`SerializeDriver`](crate::ser::SerializeDriver).  The methods of the
/// reference invoke the ones of [`Serialize`].
///
/// ```
/// use deser::adapters::DisplayFromStr;
/// use deser::ser::SerializeRef;
///
/// let value = 42u32;
/// assert!(!SerializeRef::new(&value).is_optional());
///
/// // serializes as string
/// let value = SerializeRef::serialize_as::<DisplayFromStr, _>(&value);
/// ```
///
/// Deserialization has no equivalent: values are type erased by creating
/// their [`Sink`](crate::de::Sink) (see
/// [`Deserialize::deserialize_into`](crate::de::Deserialize::deserialize_into)),
/// which is already in the middle of deserializing them.
#[derive(Clone, Copy)]
pub struct SerializeRef<'a> {
    value: &'a (dyn Erased + 'a),
}

impl<'a> SerializeRef<'a> {
    /// Creates a reference to a value.
    #[inline(always)]
    pub fn new<T: Serialize>(value: &'a T) -> SerializeRef<'a> {
        SerializeRef {
            value: Adapted::<T, T>::new(value),
        }
    }

    /// Creates a reference to a value that is serialized with an adapter.
    ///
    /// See [`adapters`](crate::adapters).
    #[inline(always)]
    pub fn serialize_as<A: Serialize<T>, T: Sync>(value: &'a T) -> SerializeRef<'a> {
        SerializeRef {
            // SAFETY: the wrapper is valid for 'a (it's the value), it only
            // holds a marker of the adapter
            value: unsafe { erase_unbounded(Adapted::<A, T>::ptr(value)) },
        }
    }

    /// Creates a reference from a trait object.
    #[inline(always)]
    pub(crate) fn from_dyn(value: &'a (dyn Erased + 'a)) -> SerializeRef<'a> {
        SerializeRef { value }
    }

    /// Returns the trait object.
    #[inline(always)]
    pub(crate) fn as_dyn(self) -> &'a (dyn Erased + 'a) {
        self.value
    }

    /// Serializes the value (see [`Serialize::serialize`]).
    #[inline]
    pub fn serialize(self, state: &mut State) -> Result<Emit<'a>, Error> {
        self.value.erased_serialize(state)
    }

    /// Invoked after the serialization finished (see [`Serialize::finish`]).
    #[inline]
    pub fn finish(self, state: &mut State) -> Result<(), Error> {
        self.value.erased_finish(state)
    }

    /// Checks if the value is optional (see [`Serialize::is_optional`]).
    #[inline]
    pub fn is_optional(self) -> bool {
        self.value.erased_is_optional()
    }

    /// Returns the shape of the value (see [`Serialize::container_shape`]).
    #[inline]
    pub fn container_shape(self) -> ContainerShape {
        self.value.erased_container_shape()
    }

    /// Describes the Rust shape of the value (see [`Serialize::describe`]).
    pub fn describe(self, d: &mut dyn Describe) {
        self.value.erased_describe(d)
    }

    /// Begins the serialization (see `Serialize::__private_begin`).
    #[inline]
    pub(crate) fn begin(self, state: &mut State) -> Result<Begin<'a>, Error> {
        self.value.erased_begin(state)
    }

    /// Returns `true` if the value is plain (see
    /// `Serialize::__private_is_plain_value`).
    #[inline]
    pub(crate) fn is_plain_value(self) -> bool {
        self.value.erased_is_plain_value()
    }

    /// Emits the events of a plain value (see
    /// `Serialize::__private_emit_plain`).
    #[inline]
    pub(crate) fn emit_plain(self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        self.value.erased_emit_plain(sink)
    }

    /// Returns the budget that is left after emitting the plain value (see
    /// `Serialize::__private_plain_cost`).
    #[inline]
    pub(crate) fn plain_cost(self, budget: usize) -> Option<usize> {
        self.value.erased_plain_cost(budget)
    }
}

impl fmt::Debug for SerializeRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerializeRef").finish_non_exhaustive()
    }
}

impl<'a, T: Serialize> From<&'a T> for SerializeRef<'a> {
    #[inline(always)]
    fn from(value: &'a T) -> SerializeRef<'a> {
        SerializeRef::new(value)
    }
}

impl Serialize for SerializeRef<'_> {
    #[inline]
    fn serialize<'b>(value: &'b Self, state: &mut State) -> Result<Emit<'b>, Error> {
        value.serialize(state)
    }

    #[inline]
    fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
        (*value).finish(state)
    }

    #[inline]
    fn is_optional(value: &Self) -> bool {
        (*value).is_optional()
    }

    #[inline]
    fn container_shape(value: &Self) -> ContainerShape {
        (*value).container_shape()
    }

    fn describe(value: &Self, d: &mut dyn Describe) {
        (*value).describe(d)
    }

    #[inline]
    fn __private_begin<'b>(value: &'b Self, state: &mut State) -> Result<Begin<'b>, Error> {
        value.begin(state)
    }

    #[inline]
    fn __private_is_plain_value(value: &Self) -> bool {
        value.is_plain_value()
    }

    #[inline]
    fn __private_emit_plain(value: &Self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        value.emit_plain(sink)
    }

    #[inline]
    fn __private_plain_cost(value: &Self, budget: usize) -> Option<usize> {
        value.plain_cost(budget)
    }
}

/// A handle to a value that is serialized.
///
/// During serialization it's common to be in a situation where one needs
/// to return a locally constructed [`Serialize`].  This is where
/// [`SerializeHandle`] comes in.  The handle either borrows the value (see
/// [`to`](Self::to) and [`SerializeRef`]) or owns it (see
/// [`arena`](Self::arena) and [`heap`](Self::heap)).
///
/// Unlike the [`SinkHandle`](crate::de::SinkHandle) of deserialization,
/// which holds a sink that is already deserializing a value, this holds a
/// value that is not serialized yet.  The serialization equivalent of a
/// sink is an emitter in a [`Emit`].  The constructors line up:
/// [`to`](Self::to) borrows, [`arena`](Self::arena) and
/// [`heap`](Self::heap) own in the same way for both handles.
pub struct SerializeHandle<'a>(pub(crate) HandleInner<'a>);

pub(crate) enum HandleInner<'a> {
    Borrowed(SerializeRef<'a>),
    // owned values are `Send` so that the serialization can move between
    // threads
    Owned(Boxed<dyn Erased + Send + 'a>),
}

impl<'a> SerializeHandle<'a> {
    /// Creates a borrowed handle to a value.
    #[inline(always)]
    pub fn to<S: Serialize>(value: &'a S) -> SerializeHandle<'a> {
        SerializeHandle(HandleInner::Borrowed(SerializeRef::new(value)))
    }

    /// Creates an owned handle to a value in the arena of the state.
    ///
    /// This is how owned values are typically created (for instance for
    /// [`Emit::Forward`]), see [`Boxed`].
    #[inline(always)]
    pub fn arena<S: Serialize + Send + 'a>(value: S, state: &mut State) -> SerializeHandle<'a> {
        SerializeHandle(HandleInner::Owned(boxed::unsize(
            Boxed::arena(Adapted::<S, S>::owned(value), state),
            |x| x as *mut (dyn Erased + Send + 'a),
        )))
    }

    /// Creates an owned handle to a value on the heap.
    ///
    /// Unlike [`arena`](Self::arena) the value does not need a state and is
    /// independent of any serialization.
    pub fn heap<S: Serialize + Send + 'a>(value: S) -> SerializeHandle<'a> {
        SerializeHandle(HandleInner::Owned(Boxed::from(
            Box::new(Adapted::<S, S>::owned(value)) as Box<dyn Erased + Send + 'a>,
        )))
    }

    /// Returns a reference to the value.
    #[inline(always)]
    pub fn get(&self) -> SerializeRef<'_> {
        match self.0 {
            HandleInner::Borrowed(value) => value,
            HandleInner::Owned(ref value) => SerializeRef::from_dyn(&**value),
        }
    }
}

impl<'a> From<SerializeRef<'a>> for SerializeHandle<'a> {
    #[inline(always)]
    fn from(value: SerializeRef<'a>) -> SerializeHandle<'a> {
        SerializeHandle(HandleInner::Borrowed(value))
    }
}

impl fmt::Debug for SerializeHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerializeHandle").finish_non_exhaustive()
    }
}

impl Serialize for SerializeHandle<'_> {
    #[inline]
    fn serialize<'b>(value: &'b Self, state: &mut State) -> Result<Emit<'b>, Error> {
        value.get().serialize(state)
    }

    #[inline]
    fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
        value.get().finish(state)
    }

    #[inline]
    fn is_optional(value: &Self) -> bool {
        value.get().is_optional()
    }

    #[inline]
    fn container_shape(value: &Self) -> ContainerShape {
        value.get().container_shape()
    }

    fn describe(value: &Self, d: &mut dyn Describe) {
        value.get().describe(d)
    }

    #[inline]
    fn __private_begin<'b>(value: &'b Self, state: &mut State) -> Result<Begin<'b>, Error> {
        value.get().begin(state)
    }

    #[inline]
    fn __private_is_plain_value(value: &Self) -> bool {
        value.get().is_plain_value()
    }

    #[inline]
    fn __private_emit_plain(value: &Self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        value.get().emit_plain(sink)
    }

    #[inline]
    fn __private_plain_cost(value: &Self, budget: usize) -> Option<usize> {
        value.get().plain_cost(budget)
    }
}
