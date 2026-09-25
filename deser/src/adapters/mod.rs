//! Adapters to customize how values are serialized and deserialized.
//!
//! An adapter is a type that knows how to serialize or deserialize a value of
//! *another* type.  Adapters implement [`SerializeAs`] and [`DeserializeAs`]
//! for the types they support.  They are typically zero sized marker types
//! which are never instantiated.
//!
//! Adapters compose: the standard containers are adapters for the same
//! container holding other types.  For instance `Vec<U>` is an adapter for
//! `Vec<T>` if `U` is an adapter for `T` and `Option<U>` is an adapter for
//! `Option<T>`.  [`Same`] is the adapter that uses the regular
//! [`Serialize`] and [`Deserialize`] implementations.
//!
//! With the derive, adapters are selected with `#[deser(as = ...)]`.  In the
//! attribute `_` can be used as a shorthand for [`Same`]:
//!
//! ```
//! use std::collections::BTreeMap;
//! use std::net::IpAddr;
//! use deser::{Deserialize, Serialize};
//! use deser::adapters::DisplayFromStr;
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct Config {
//!     #[deser(as = DisplayFromStr)]
//!     listen: IpAddr,
//!     #[deser(as = Option<DisplayFromStr>)]
//!     upstream: Option<IpAddr>,
//!     #[deser(as = BTreeMap<_, Vec<DisplayFromStr>>)]
//!     aliases: BTreeMap<String, Vec<IpAddr>>,
//! }
//! ```
//!
//! Missing fields are handled by the adapter (see
//! [`DeserializeAs::initial_value_as`]) so in the example above `upstream` is
//! optional as `Option<U>` makes missing values `None`.
//!
//! To use an adapter outside of the derive, the [`As`] wrapper can be used.
//! It holds a value and serializes and deserializes it with an adapter.
//!
//! # Provided Adapters
//!
//! * [`Same`]: uses [`Serialize`] and [`Deserialize`] of the type itself.
//! * [`DisplayFromStr`]: serializes with [`Display`](std::fmt::Display) and
//!   deserializes with [`FromStr`](std::str::FromStr).
//! * [`FromInto`] and [`TryFromInto`]: convert from and into another type.
//! * [`DefaultOnError`]: uses the [`Default`] if a value cannot be
//!   deserialized.
//! * [`VecSkipError`] and [`MapSkipError`]: skip elements and entries that
//!   cannot be deserialized.
//! * The standard containers: `Option<U>`, `Box<U>`, `Vec<U>`, `[U]`,
//!   `[U; N]`, `BTreeMap<K, V>`, `HashMap<K, V>`, `BTreeSet<U>`,
//!   `HashSet<U>` and tuples.
//!
//! # Implementing Adapters
//!
//! Adapters are implemented like [`Deserialize`] and [`Serialize`] except
//! that the value is not `Self`.  This example serializes a byte vector
//! into a hex string:
//!
//! ```
//! use deser::adapters::{DeserializeAs, SerializeAs};
//! use deser::de::{Sink, SinkHandle};
//! use deser::ser::Chunk;
//! use deser::{make_slot_wrapper, Atom, Error, ErrorKind, State};
//!
//! pub struct Hex;
//!
//! impl SerializeAs<Vec<u8>> for Hex {
//!     fn serialize_as<'a>(value: &'a Vec<u8>, _state: &mut State) -> Result<Chunk<'a>, Error> {
//!         let hex: String = value.iter().map(|x| format!("{:02x}", x)).collect();
//!         Ok(Chunk::Atom(Atom::Str(hex.into())))
//!     }
//! }
//!
//! make_slot_wrapper!(HexSlot);
//!
//! impl Sink for HexSlot<Vec<u8>> {
//!     fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
//!         match atom {
//!             Atom::Str(ref s) if s.len() % 2 == 0 => {
//!                 let bytes = (0..s.len())
//!                     .step_by(2)
//!                     .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
//!                     .collect::<Result<Vec<_>, _>>()
//!                     .map_err(|_| Error::new(ErrorKind::Unexpected, "invalid hex"))?;
//!                 **self = Some(bytes);
//!                 Ok(())
//!             }
//!             other => self.unexpected_atom(other, state),
//!         }
//!     }
//! }
//!
//! impl DeserializeAs<Vec<u8>> for Hex {
//!     fn deserialize_into_as(out: &mut Option<Vec<u8>>) -> SinkHandle<'_> {
//!         HexSlot::make_handle(out)
//!     }
//! }
//!
//! #[derive(deser::Serialize, deser::Deserialize)]
//! pub struct Blob {
//!     #[deser(as = Hex)]
//!     data: Vec<u8>,
//!     #[deser(as = Vec<Hex>)]
//!     parts: Vec<Vec<u8>>,
//! }
//! ```
//!
//! Adapters need to be `'static`.  This is the case for all types that do not
//! hold references.
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::de::{atom_into_handle, Deserialize, OwnedSink, SinkHandle};
use crate::descriptors::{Descriptor, NullDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ser::{Begin, Chunk, Serialize};
use crate::State;

mod ser_impls;
mod stock;

pub use self::stock::{
    DefaultOnError, DisplayFromStr, FromInto, MapSkipError, TryFromInto, VecSkipError,
};

/// Deserializes a value of type `T` on behalf of it.
///
/// This is the equivalent of [`Deserialize`] for adapters.  See the
/// [module documentation](self) for more information.
pub trait DeserializeAs<T>: 'static {
    /// Creates a sink that deserializes the value into the given slot.
    ///
    /// See [`Deserialize::deserialize_into`].
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_>;

    /// Provides the value of a missing struct field.
    ///
    /// See [`Deserialize::initial_value`].
    fn initial_value_as() -> Option<T> {
        None
    }

    #[doc(hidden)]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        atom_into_handle(Self::deserialize_into_as(out), atom, state)
    }

    #[doc(hidden)]
    fn __private_is_bytes_as() -> bool {
        false
    }

    #[doc(hidden)]
    fn __private_vec_from_bytes_as(bytes: Vec<u8>) -> Option<Vec<T>> {
        let _ = bytes;
        None
    }

    #[doc(hidden)]
    fn __private_array_from_bytes_as<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
        let _ = bytes;
        None
    }
}

/// Serializes a value of type `T` on behalf of it.
///
/// This is the equivalent of [`Serialize`] for adapters.  See the
/// [module documentation](self) for more information.
pub trait SerializeAs<T: ?Sized>: 'static {
    /// Serializes the value.
    ///
    /// See [`Serialize::serialize`].
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error>;

    /// Invoked after the serialization finished.
    ///
    /// See [`Serialize::finish`].
    fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
        let _ = value;
        let _ = state;
        Ok(())
    }

    /// Checks if the value represents an optional value.
    ///
    /// See [`Serialize::is_optional`].
    fn is_optional_as(value: &T) -> bool {
        let _ = value;
        false
    }

    /// Returns the descriptor of the value.
    ///
    /// See [`Serialize::descriptor`].
    fn descriptor_as(value: &T) -> &'static dyn Descriptor {
        let _ = value;
        &NullDescriptor
    }

    #[doc(hidden)]
    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        let descriptor = Self::descriptor_as(value);
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            descriptor,
            true,
        ))
    }

    #[doc(hidden)]
    fn __private_slice_as_bytes_as(val: &[T]) -> Option<Cow<'_, [u8]>>
    where
        T: Sized,
    {
        let _ = val;
        None
    }
}

/// The adapter that uses the type's own implementation.
///
/// This forwards to [`Serialize`] and [`Deserialize`].  In
/// `#[deser(as = ...)]` attributes `_` can be used instead.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::Deserialize;
/// use deser::adapters::DisplayFromStr;
///
/// #[derive(Deserialize)]
/// pub struct Ports {
///     // same as BTreeMap<deser::adapters::Same, DisplayFromStr>
///     #[deser(as = BTreeMap<_, DisplayFromStr>)]
///     ports: BTreeMap<String, u16>,
/// }
/// ```
pub struct Same;

impl<T: Deserialize> DeserializeAs<T> for Same {
    #[inline]
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_> {
        T::deserialize_into(out)
    }

    #[inline]
    fn initial_value_as() -> Option<T> {
        T::initial_value()
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        T::__private_atom_into(out, atom, state)
    }

    #[inline]
    fn __private_is_bytes_as() -> bool {
        T::__private_is_bytes()
    }

    #[inline]
    fn __private_vec_from_bytes_as(bytes: Vec<u8>) -> Option<Vec<T>> {
        T::__private_vec_from_bytes(bytes)
    }

    #[inline]
    fn __private_array_from_bytes_as<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
        T::__private_array_from_bytes(bytes)
    }
}

impl<T: Serialize + ?Sized> SerializeAs<T> for Same {
    #[inline]
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
        value.serialize(state)
    }

    #[inline]
    fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
        value.finish(state)
    }

    #[inline]
    fn is_optional_as(value: &T) -> bool {
        value.is_optional()
    }

    #[inline]
    fn descriptor_as(value: &T) -> &'static dyn Descriptor {
        value.descriptor()
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        value.__private_begin(state)
    }

    #[inline]
    fn __private_slice_as_bytes_as(val: &[T]) -> Option<Cow<'_, [u8]>>
    where
        T: Sized,
    {
        T::__private_slice_as_bytes(val)
    }
}

/// A reference to a value that serializes with an adapter.
///
/// This is a transparent wrapper which can be created from a reference to
/// the value without copying it.  It's the building block to serialize
/// values with adapters, for instance from emitters of container adapters.
///
/// ```
/// use deser::adapters::{DisplayFromStr, SerializeAsRef};
/// use deser::ser::SerializeHandle;
///
/// let value = 42u32;
/// let handle = SerializeHandle::to(SerializeAsRef::<DisplayFromStr, _>::new(&value));
/// # drop(handle);
/// ```
#[repr(transparent)]
pub struct SerializeAsRef<A, T: ?Sized> {
    _marker: PhantomData<fn() -> A>,
    value: T,
}

impl<A, T: ?Sized> SerializeAsRef<A, T> {
    /// Wraps a reference to a value.
    #[inline(always)]
    pub fn new(value: &T) -> &SerializeAsRef<A, T> {
        // SAFETY: the wrapper is transparent over `T` (the marker is zero
        // sized and has an alignment of one).
        unsafe { &*(value as *const T as *const SerializeAsRef<A, T>) }
    }

    /// Returns the wrapped value.
    #[inline(always)]
    pub fn get(&self) -> &T {
        &self.value
    }
}

impl<A: SerializeAs<T>, T: ?Sized> Serialize for SerializeAsRef<A, T> {
    #[inline]
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        A::serialize_as(&self.value, state)
    }

    #[inline]
    fn finish(&self, state: &mut State) -> Result<(), Error> {
        A::finish_as(&self.value, state)
    }

    #[inline]
    fn is_optional(&self) -> bool {
        A::is_optional_as(&self.value)
    }

    #[inline]
    fn descriptor(&self) -> &'static dyn Descriptor {
        A::descriptor_as(&self.value)
    }

    #[inline]
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        A::__private_begin_as(&self.value, state)
    }
}

/// Holds a value that serializes and deserializes with an adapter.
///
/// This implements [`Serialize`] and [`Deserialize`] with the adapter `A`.
/// It's useful to use adapters in places where `#[deser(as = ...)]` is not
/// available.  The wrapper dereferences to the value.
///
/// ```
/// use deser::adapters::{As, DisplayFromStr};
///
/// let values: Vec<As<u32, DisplayFromStr>> = vec![As::new(1), As::new(2)];
/// assert_eq!(*values[0], 1);
/// ```
#[repr(transparent)]
pub struct As<T, A> {
    value: T,
    _marker: PhantomData<fn() -> A>,
}

impl<T, A> As<T, A> {
    /// Wraps a value.
    #[inline(always)]
    pub fn new(value: T) -> As<T, A> {
        As {
            value,
            _marker: PhantomData,
        }
    }

    /// Returns the wrapped value.
    #[inline(always)]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T, A> From<T> for As<T, A> {
    fn from(value: T) -> As<T, A> {
        As::new(value)
    }
}

impl<T, A> Deref for As<T, A> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T, A> DerefMut for As<T, A> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: fmt::Debug, A> fmt::Debug for As<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.value, f)
    }
}

impl<T: Clone, A> Clone for As<T, A> {
    fn clone(&self) -> Self {
        As::new(self.value.clone())
    }
}

impl<T: Copy, A> Copy for As<T, A> {}

impl<T: Default, A> Default for As<T, A> {
    fn default() -> Self {
        As::new(T::default())
    }
}

impl<T: PartialEq, A> PartialEq for As<T, A> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq, A> Eq for As<T, A> {}

impl<T: PartialOrd, A> PartialOrd for As<T, A> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<T: Ord, A> Ord for As<T, A> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

impl<T: Hash, A> Hash for As<T, A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

impl<T, A: DeserializeAs<T>> Deserialize for As<T, A> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        crate::de::mapped::MappedSink::handle(out, OwnedSink::deserialize_as::<A>(), |value| {
            Ok(As::new(value))
        })
    }

    fn initial_value() -> Option<Self> {
        A::initial_value_as().map(As::new)
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut inner = None;
        A::__private_atom_into_as(&mut inner, atom, state)?;
        *out = inner.map(As::new);
        Ok(())
    }
}

impl<T, A: SerializeAs<T>> Serialize for As<T, A> {
    #[inline]
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        A::serialize_as(&self.value, state)
    }

    #[inline]
    fn finish(&self, state: &mut State) -> Result<(), Error> {
        A::finish_as(&self.value, state)
    }

    #[inline]
    fn is_optional(&self) -> bool {
        A::is_optional_as(&self.value)
    }

    #[inline]
    fn descriptor(&self) -> &'static dyn Descriptor {
        A::descriptor_as(&self.value)
    }

    #[inline]
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        A::__private_begin_as(&self.value, state)
    }
}
