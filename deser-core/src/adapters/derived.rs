//! The derived implementation of a type as adapter (see [`Derived`]).
use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::State;
use crate::de::{Deserialize, SinkHandle, atom_into_handle, borrowed_atom_into_handle, update};
use crate::error::Error;
use crate::event::{Atom, ContainerShape};
use crate::ser::{Begin, Describe, Emit, PlainSink, Serialize};

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
/// use deser::{Deserialize, State};
/// use deser::de::SinkHandle;
///
/// /// Deserializes with `A`, missing values are the default.
/// pub struct DefaultIfMissing<A>(std::marker::PhantomData<A>);
///
/// impl<'de, T: Default + Send, A: Deserialize<'de, T>> Deserialize<'de, T>
///     for DefaultIfMissing<A>
/// {
///     fn deserialize_into<'out>(
///         out: &'out mut Option<T>,
///         state: &mut State,
///     ) -> SinkHandle<'out, 'de> {
///         A::deserialize_into(out, state)
///     }
///
///     fn initial_value() -> Option<T> {
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
/// // the limits are missing
/// let config: Config = deser_json::from_str("{}").unwrap();
/// assert_eq!(config.limits.max_connections, 0);
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
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de>;

    fn initial_value() -> Option<Self> {
        None
    }

    fn expecting() -> Cow<'static, str> {
        crate::de::slot::short_type_name(core::any::type_name::<Self>())
    }

    fn describe_type(d: &mut dyn Describe) {
        let _ = d;
    }

    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        update::replace_handle_with(
            value,
            <Self as DerivedDeserialize<'de>>::deserialize_into,
            state,
        )
    }

    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        atom_into_handle(
            <Self as DerivedDeserialize<'de>>::deserialize_into(out, state),
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
            <Self as DerivedDeserialize<'de>>::deserialize_into(out, state),
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
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error>;

    fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
        let _ = (value, state);
        Ok(())
    }

    fn is_optional(value: &Self) -> bool {
        let _ = value;
        false
    }

    fn describe(value: &Self, d: &mut dyn Describe) {
        let _ = (value, d);
    }

    fn container_shape(value: &Self) -> ContainerShape {
        let _ = value;
        ContainerShape::new()
    }

    fn __private_begin<'a>(value: &'a Self, state: &mut State) -> Result<Begin<'a>, Error> {
        let shape = <Self as DerivedSerialize>::container_shape(value);
        Ok(Begin::emit(
            <Self as DerivedSerialize>::serialize(value, state)?,
            shape,
            true,
        ))
    }

    fn __private_is_plain() -> bool {
        false
    }

    fn __private_is_plain_value(value: &Self) -> bool {
        let _ = value;
        <Self as DerivedSerialize>::__private_is_plain()
    }

    fn __private_emit_plain(value: &Self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        let _ = (value, sink);
        unreachable!("not a plain value")
    }

    fn __private_plain_cost(value: &Self, budget: usize) -> Option<usize> {
        let _ = value;
        budget.checked_sub(1)
    }

    fn __private_slice_as_bytes(_val: &[Self]) -> Option<Cow<'_, [u8]>>
    where
        Self: Sized,
    {
        None
    }
}

impl<'de, T: DerivedDeserialize<'de>> Deserialize<'de, T> for Derived {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        T::deserialize_into(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        T::expecting()
    }

    fn describe_type(d: &mut dyn Describe) {
        T::describe_type(d)
    }

    #[inline]
    fn initial_value() -> Option<T> {
        T::initial_value()
    }

    #[inline]
    fn deserialize_update<'out>(value: &'out mut T, state: &mut State) -> SinkHandle<'out, 'de> {
        T::deserialize_update(value, state)
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        T::__private_atom_into(out, atom, state)
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        T::__private_borrowed_atom_into(out, atom, state)
    }

    #[inline]
    fn __private_is_bytes() -> bool {
        T::__private_is_bytes()
    }

    #[inline]
    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<T>> {
        T::__private_vec_from_bytes(bytes)
    }

    #[inline]
    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
        T::__private_array_from_bytes(bytes)
    }
}

impl<T: DerivedSerialize + ?Sized> Serialize<T> for Derived {
    #[inline]
    fn serialize<'a>(value: &'a T, state: &mut State) -> Result<Emit<'a>, Error> {
        T::serialize(value, state)
    }

    #[inline]
    fn finish(value: &T, state: &mut State) -> Result<(), Error> {
        T::finish(value, state)
    }

    #[inline]
    fn is_optional(value: &T) -> bool {
        T::is_optional(value)
    }

    #[inline]
    fn container_shape(value: &T) -> ContainerShape {
        T::container_shape(value)
    }

    fn describe(value: &T, d: &mut dyn Describe) {
        T::describe(value, d)
    }

    #[inline]
    fn __private_begin<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        T::__private_begin(value, state)
    }

    #[inline]
    fn __private_is_plain() -> bool {
        T::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(value: &T) -> bool {
        T::__private_is_plain_value(value)
    }

    #[inline]
    fn __private_emit_plain(value: &T, sink: &mut dyn PlainSink) -> Result<(), Error> {
        T::__private_emit_plain(value, sink)
    }

    #[inline]
    fn __private_plain_cost(value: &T, budget: usize) -> Option<usize> {
        T::__private_plain_cost(value, budget)
    }

    #[inline]
    fn __private_slice_as_bytes(val: &[T]) -> Option<Cow<'_, [u8]>>
    where
        T: Sized,
    {
        T::__private_slice_as_bytes(val)
    }
}
