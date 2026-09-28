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
//! `serialize_as` and `deserialize_as` select an adapter for one direction
//! only.  All three attributes can also be placed on structs, enums and
//! unions to serialize and deserialize the type itself with an adapter (see
//! [container adapters](crate::derive#container-adapters)).
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
//! * [`Borrowed`]: deserializes a `Cow<str>` or `Cow<[u8]>` borrowed from the
//!   data if possible.
//! * [`Flag`]: a `bool` which is set by giving its key (like `?recursive`
//!   in a query string).
//! * [`Separated`]: a sequence written as text with a separator (like
//!   `a,b,c` in an environment variable).
//! * [`TrimWhitespace`]: trims whitespace from strings before they are
//!   deserialized.
//! * [`SkipBlank`]: leaves no value for blank strings, which leaves them
//!   out of sequences.
//! * The adapters for bytes: the base64 encodings (for instance
//!   [`Base64Url`]) and [`BytesFallback`] (see [bytes](#bytes)).
//! * The standard containers: `Option<U>`, `Result<U, V>`, `Box<U>`,
//!   `Arc<U>`, `Vec<U>`, `VecDeque<U>`, `LinkedList<U>`, `BinaryHeap<U>`,
//!   `[U]`, `[U; N]`, `Box<[U]>`, `Arc<[U]>`, `BTreeMap<K, V>`,
//!   `HashMap<K, V>`, `BTreeSet<U>`, `HashSet<U>` and tuples.  With the
//!   features of the same names also the collections of `indexmap`,
//!   `hashbrown`, `smallvec` and `arrayvec` (for instance `IndexMap<K, V>`
//!   and `SmallVec<[U; N]>`).
//!
//! # Bytes
//!
//! Bytes (`Vec<u8>`, `[u8; N]`, `&[u8]` and `Cow<[u8]>`) are part of the
//! data model as [`Atom::Bytes`].  Formats which support
//! bytes natively (such as CBOR) use that, text formats like JSON and TOML
//! have to represent them differently.  In deser the convention is:
//!
//! * Formats without native bytes write bytes as base64 strings (RFC 4648,
//!   standard alphabet with padding).  They can be configured with a
//!   different [`BytesFormat`] (for instance to write sequences of integers).
//! * Types that expect bytes accept a string and decode it.  By default
//!   this is lenient base64: both the standard and the URL-safe alphabet
//!   are accepted and the padding is optional.  Sequences of integers are
//!   always accepted.
//!
//! ## Adapters
//!
//! How the bytes of an individual value are represented can be changed with
//! these adapters.  They support `Vec<u8>`, `[u8; N]` and
//! `Cow<[u8]>` (see [`BytesBuf`]).
//!
//! * The encodings (such as [`Base64Url`]) are adapters which represent
//!   bytes as strings in the encoding in all formats, also in formats with
//!   native bytes.
//! * [`BytesFallback`] keeps bytes as bytes in formats with native bytes and
//!   only picks the representation for formats without them.  This can be
//!   an encoding (`BytesFallback<Base64Url>`) or sequences of integers
//!   (`BytesFallback<IntSeq>`).
//!
//! ```
//! use deser::adapters::{Base64Url, Base64UrlNoPad, BytesFallback, IntSeq};
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct Blob {
//!     // base64 in JSON and TOML, bytes in CBOR
//!     data: Vec<u8>,
//!     // a URL-safe base64 string in all formats
//!     #[deser(as = Base64UrlNoPad)]
//!     token: [u8; 32],
//!     // URL-safe base64 in JSON and TOML, bytes in CBOR
//!     #[deser(as = BytesFallback<Base64Url>)]
//!     signature: Vec<u8>,
//!     // `[1, 2]` in JSON and TOML, bytes in CBOR
//!     #[deser(as = BytesFallback<IntSeq>)]
//!     legacy: Vec<u8>,
//! }
//! ```
//!
//! When deserializing, all of these accept native bytes and strings in
//! their encoding.
//!
//! ## Encodings
//!
//! deser provides the base64 encodings:
//!
//! | Encoding           | Description                                        |
//! |--------------------|----------------------------------------------------|
//! | [`Base64`]         | base64, standard alphabet with padding             |
//! | [`Base64NoPad`]    | base64, standard alphabet without padding          |
//! | [`Base64Url`]      | base64, URL-safe alphabet with padding             |
//! | [`Base64UrlNoPad`] | base64, URL-safe alphabet without padding          |
//!
//! They all decode leniently like the default: both alphabets are accepted
//! and the padding is optional.
//!
//! More encodings (hexadecimal and base32) are provided by
//! [`deser-encoding`](https://docs.rs/deser-encoding).  Other encodings can
//! be added by implementing [`BytesEncoding`].  They are adapters like the
//! encodings provided by deser.
//!
//! # Implementing Adapters
//!
//! Adapters are implemented like [`Deserialize`] and [`Serialize`] except
//! that the value is not `Self`.  This example serializes a byte vector
//! into a hex string (`deser-encoding` provides this as `Hex`, and for
//! bytes implementing [`BytesEncoding`] is less work):
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
//!     fn serialize_as<'a>(
//!         value: &'a Vec<u8>,
//!         _state: &mut State,
//!     ) -> Result<Chunk<'a>, Error> {
//!         let hex: String =
//!             value.iter().map(|x| format!("{:02x}", x)).collect();
//!         Ok(Chunk::Atom(Atom::Str(hex.into())))
//!     }
//! }
//!
//! make_slot_wrapper!(HexSlot);
//!
//! impl<'de> Sink<'de> for HexSlot<Vec<u8>> {
//!     fn atom(
//!         &mut self,
//!         atom: Atom,
//!         state: &mut State,
//!     ) -> Result<(), Error> {
//!         match atom {
//!             Atom::Str(ref s) if s.len() % 2 == 0 => {
//!                 let bytes = (0..s.len())
//!                     .step_by(2)
//!                     .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
//!                     .collect::<Result<Vec<_>, _>>()
//!                     .map_err(|_| {
//!                         Error::new(ErrorKind::Unexpected, "invalid hex")
//!                     })?;
//!                 **self = Some(bytes);
//!                 Ok(())
//!             }
//!             other => self.unexpected_atom(other, state),
//!         }
//!     }
//! }
//!
//! impl<'de> DeserializeAs<'de, Vec<u8>> for Hex {
//!     fn deserialize_into_as<'out>(
//!         out: &'out mut Option<Vec<u8>>,
//!         state: &mut State,
//!     ) -> SinkHandle<'out, 'de> {
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
use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use crate::State;
use crate::de::{Deserialize, OwnedSink, SinkHandle, atom_into_handle, borrowed_atom_into_handle};
use crate::error::Error;
use crate::event::{Atom, ContainerShape};
use crate::ser::{Begin, Chunk, Describe, Serialize};

pub(crate) mod bytes;
mod derived;
pub(crate) mod ser_impls;
mod stock;
mod text;

pub use self::bytes::{
    Base64, Base64NoPad, Base64Url, Base64UrlNoPad, BytesBuf, BytesEncoding, BytesFallback,
    BytesFallbackFormat, BytesFormat, IntSeq,
};
pub use self::derived::Derived;
#[doc(hidden)]
pub use self::derived::{DerivedDeserialize, DerivedSerialize};
pub(crate) use self::ser_impls::SerializeAsRef;
pub use self::stock::{
    Borrowed, DefaultOnError, DisplayFromStr, Flag, FromInto, MapSkipError, TryFromInto,
    VecSkipError,
};
pub use self::text::{Separated, SkipBlank, TrimWhitespace};
// used for the maps of other crates
#[allow(unused_imports)]
pub(crate) use self::stock::skip_map_sink;

/// Deserializes a value of type `T` on behalf of it.
///
/// This is the equivalent of [`Deserialize`] for adapters.  See the
/// [module documentation](self) for more information.
pub trait DeserializeAs<'de, T>: 'static {
    /// Creates a sink that deserializes the value into the given slot.
    ///
    /// See [`Deserialize::deserialize_into`].
    fn deserialize_into_as<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de>;

    /// Provides the value of a missing struct field.
    ///
    /// See [`Deserialize::initial_value`].
    fn initial_value_as() -> Option<T> {
        None
    }

    /// Creates a sink that updates an existing value.
    ///
    /// This is the adapter's version of
    /// [`Deserialize::deserialize_update`], the derive uses it to update
    /// fields with adapters.  The default implementation replaces the value
    /// with the deserialized one.
    fn deserialize_update_as<'out>(value: &'out mut T, state: &mut State) -> SinkHandle<'out, 'de>
    where
        T: Send,
        Self: Sized,
    {
        crate::de::update::replace_with(value, OwnedSink::deserialize_as::<Self>(state), state)
    }

    #[doc(hidden)]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        atom_into_handle(Self::deserialize_into_as(out, state), atom, state)
    }

    #[doc(hidden)]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        borrowed_atom_into_handle(Self::deserialize_into_as(out, state), atom, state)
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

    /// See [`Deserialize::__private_collects`].
    #[doc(hidden)]
    fn __private_collects_as() -> bool {
        false
    }

    /// See [`Deserialize::__private_collect_into`].
    #[doc(hidden)]
    fn __private_collect_into_as<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        Self::deserialize_into_as(out, state)
    }

    /// See [`Deserialize::__private_collect_update`].
    #[doc(hidden)]
    fn __private_collect_update_as<'out>(
        value: &'out mut T,
        first: bool,
        state: &mut State,
    ) -> SinkHandle<'out, 'de>
    where
        T: Send,
        Self: Sized,
    {
        let _ = first;
        Self::deserialize_update_as(value, state)
    }

    /// See [`Deserialize::__private_collect_empty`].
    #[doc(hidden)]
    fn __private_collect_empty_as() -> Option<T> {
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

    /// Returns the shape of the value if it's a map or sequence.
    ///
    /// See [`Serialize::container_shape`].
    fn container_shape_as(value: &T) -> ContainerShape {
        let _ = value;
        ContainerShape::new()
    }

    /// Describes the Rust shape of the value.
    ///
    /// See [`Serialize::describe`].
    fn describe_as(value: &T, d: &mut dyn Describe) {
        let _ = value;
        let _ = d;
    }

    #[doc(hidden)]
    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(Begin::chunk(Self::serialize_as(value, state)?, shape, true))
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

impl<'de, T: Deserialize<'de>> DeserializeAs<'de, T> for Same {
    #[inline]
    fn deserialize_into_as<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        T::deserialize_into(out, state)
    }

    #[inline]
    fn initial_value_as() -> Option<T> {
        T::initial_value()
    }

    #[inline]
    fn deserialize_update_as<'out>(value: &'out mut T, state: &mut State) -> SinkHandle<'out, 'de>
    where
        T: Send,
    {
        T::deserialize_update(value, state)
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
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        T::__private_borrowed_atom_into(out, atom, state)
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

    #[inline]
    fn __private_collects_as() -> bool {
        T::__private_collects()
    }

    #[inline]
    fn __private_collect_into_as<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        T::__private_collect_into(out, state)
    }

    #[inline]
    fn __private_collect_update_as<'out>(
        value: &'out mut T,
        first: bool,
        state: &mut State,
    ) -> SinkHandle<'out, 'de>
    where
        T: Send,
    {
        T::__private_collect_update(value, first, state)
    }

    #[inline]
    fn __private_collect_empty_as() -> Option<T> {
        T::__private_collect_empty()
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
    fn container_shape_as(value: &T) -> ContainerShape {
        value.container_shape()
    }

    fn describe_as(value: &T, d: &mut dyn Describe) {
        value.describe(d)
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

impl<'de, T: Send, A: DeserializeAs<'de, T>> Deserialize<'de> for As<T, A> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        crate::de::mapped::MappedSink::handle(
            out,
            OwnedSink::deserialize_as::<A>(state),
            |value| Ok(As::new(value)),
            state,
        )
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

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut inner = None;
        A::__private_borrowed_atom_into_as(&mut inner, atom, state)?;
        *out = inner.map(As::new);
        Ok(())
    }

    #[inline]
    fn __private_is_bytes() -> bool {
        A::__private_is_bytes_as()
    }

    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<Self>> {
        A::__private_vec_from_bytes_as(bytes).map(|x| x.into_iter().map(As::new).collect())
    }

    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[Self; N]> {
        A::__private_array_from_bytes_as::<N>(bytes).map(|x| x.map(As::new))
    }
}

impl<T: Sync, A: SerializeAs<T>> Serialize for As<T, A> {
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
    fn container_shape(&self) -> ContainerShape {
        A::container_shape_as(&self.value)
    }

    #[inline]
    fn describe(&self, d: &mut dyn Describe) {
        A::describe_as(&self.value, d)
    }

    #[inline]
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        A::__private_begin_as(&self.value, state)
    }

    #[inline]
    fn __private_slice_as_bytes(val: &[Self]) -> Option<Cow<'_, [u8]>> {
        // SAFETY: the wrapper is transparent over `T` (the marker is zero
        // sized and has an alignment of one).
        let val = unsafe { &*(val as *const [Self] as *const [T]) };
        A::__private_slice_as_bytes_as(val)
    }
}
