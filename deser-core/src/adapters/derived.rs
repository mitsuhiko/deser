//! The derived implementation of a type as adapter (see [`Derived`]).
use std::borrow::Cow;

use crate::State;
use crate::adapters::{DeserializeAs, SerializeAs};
use crate::de::{SinkHandle, atom_into_handle, borrowed_atom_into_handle, update};
use crate::error::Error;
use crate::event::{Atom, ContainerShape};
use crate::ser::{Begin, Chunk, Describe, PlainSink};

/// The adapter that uses the derived implementation of a type.
///
/// Adapters on a type (see [container
/// adapters](crate::derive#container-adapters)) replace its derived
/// implementation.  Adapters which wrap another adapter can wrap the
/// derived implementation with `Derived`, which is written as `_` in the
/// attribute.  This is how values are checked or converted after the
/// derived implementation deserialized them:
///
/// ```
/// use deser::Deserialize;
/// use deser::adapters::{DeserializeAs, Derived};
/// use deser::de::SinkHandle;
///
/// /// Deserializes with `A`, missing values are the default.
/// pub struct DefaultIfMissing<A>(std::marker::PhantomData<A>);
///
/// impl<'de, T: Default, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for DefaultIfMissing<A> {
///     fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
///         A::deserialize_into_as(out)
///     }
///
///     fn initial_value_as() -> Option<T> {
///         Some(T::default())
///     }
/// }
///
/// #[derive(Deserialize, Default)]
/// #[deser(deserialize_as = DefaultIfMissing<_>)]
/// struct Limits {
///     max_connections: u32,
/// }
///
/// #[derive(Deserialize)]
/// struct Config {
///     limits: Limits,
/// }
///
/// // `{}`: the limits are missing
/// let mut config = None::<Config>;
/// let mut driver = deser::de::DeserializeDriver::new(&mut config);
/// driver.emit(deser::Event::map_start()).unwrap();
/// driver.emit(deser::Event::MapEnd).unwrap();
/// drop(driver);
/// assert_eq!(config.unwrap().limits.max_connections, 0);
/// ```
///
/// The derived implementation is only available through this adapter for
/// the directions that have an adapter with `_`.
pub struct Derived;

/// The derived implementation of `Deserialize`.
///
/// This mirrors [`Deserialize`](crate::Deserialize), the derive implements
/// it instead if the type has an adapter which uses [`Derived`].  Not
/// public API.
#[doc(hidden)]
pub trait DerivedDeserialize<'de>: Sized + Send {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de>;

    fn initial_value() -> Option<Self> {
        None
    }

    fn deserialize_update(value: &mut Self) -> SinkHandle<'_, 'de> {
        update::replace_handle_with(value, <Self as DerivedDeserialize<'de>>::deserialize_into)
    }

    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        atom_into_handle(
            <Self as DerivedDeserialize<'de>>::deserialize_into(out),
            atom,
            state,
        )
    }

    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        borrowed_atom_into_handle(
            <Self as DerivedDeserialize<'de>>::deserialize_into(out),
            atom,
            state,
        )
    }

    fn __private_is_bytes() -> bool {
        false
    }

    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<Self>> {
        let _ = bytes;
        None
    }

    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[Self; N]> {
        let _ = bytes;
        None
    }
}

/// The derived implementation of `Serialize`.
///
/// This mirrors [`Serialize`](crate::Serialize), the derive implements it
/// instead if the type has an adapter which uses [`Derived`].  Not public
/// API.
#[doc(hidden)]
pub trait DerivedSerialize: Sync {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error>;

    fn finish(&self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn is_optional(&self) -> bool {
        false
    }

    fn describe(&self, d: &mut dyn Describe) {
        let _ = d;
    }

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
    }

    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        let shape = DerivedSerialize::container_shape(self);
        Ok(Begin::chunk(
            DerivedSerialize::serialize(self, state)?,
            shape,
            true,
        ))
    }

    fn __private_is_plain() -> bool
    where
        Self: Sized,
    {
        false
    }

    fn __private_is_plain_value(&self) -> bool
    where
        Self: Sized,
    {
        <Self as DerivedSerialize>::__private_is_plain()
    }

    fn __private_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        let _ = sink;
        unreachable!("not a plain value")
    }

    fn __private_slice_as_bytes(_val: &[Self]) -> Option<Cow<'_, [u8]>>
    where
        Self: Sized,
    {
        None
    }
}

impl<'de, T: DerivedDeserialize<'de>> DeserializeAs<'de, T> for Derived {
    #[inline]
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        T::deserialize_into(out)
    }

    #[inline]
    fn initial_value_as() -> Option<T> {
        T::initial_value()
    }

    #[inline]
    fn deserialize_update_as(value: &mut T) -> SinkHandle<'_, 'de>
    where
        T: Send,
    {
        T::deserialize_update(value)
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
}

impl<T: DerivedSerialize + ?Sized> SerializeAs<T> for Derived {
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
