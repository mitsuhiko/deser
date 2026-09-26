//! Well-known formatting hints.
//!
//! Hints tell formats how a value would like to be presented.  They are not
//! part of the data model and never change the value: formats which do not
//! have a choice (or do not support a hint) ignore them.  Hints are
//! [event data](crate::State::event) which is attached to the first event of
//! a value.  They are set by values (for instance through the adapters of
//! this module) or by [layers](crate::ser::Layer), which allows to set them
//! by path.  The last hint set wins, so hints set by layers take precedence
//! over the ones of the values.
//!
//! | Hint       | Honored by                                          |
//! |------------|-----------------------------------------------------|
//! | [`Layout`] | TOML (inline tables and arrays of tables)            |
//!
//! ```
//! use std::collections::BTreeMap;
//! use deser::Serialize;
//! use deser::hints::Compact;
//!
//! #[derive(Serialize)]
//! struct Config {
//!     #[deser(as = Compact)]
//!     point: BTreeMap<String, u32>,
//! }
//! ```
use std::borrow::Cow;
use std::marker::PhantomData;

use crate::State;
use crate::adapters::{DeserializeAs, Same, SerializeAs};
use crate::de::SinkHandle;
use crate::error::Error;
use crate::event::{Atom, ContainerShape};
use crate::ser::{Begin, Chunk, Describe};

/// How a map or sequence is laid out by formats that have a choice.
///
/// ```
/// use deser::hints::Layout;
/// # let mut driver = deser::ser::SerializeDriver::new(&());
/// # let state = driver.state_mut();
///
/// Layout::Compact.set(state);
/// assert_eq!(Layout::of(state), Layout::Compact);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Layout {
    /// No preference, the format decides.
    #[default]
    Auto,
    /// Keep the container compact, for instance on a single line (a flow
    /// collection in YAML, an inline table in TOML).
    Compact,
    /// Spread the container out, for instance one entry per line (a block
    /// collection in YAML, a section in TOML).
    Expanded,
}

impl Layout {
    /// Returns the layout of the current event.
    #[inline]
    pub fn of(state: &State) -> Layout {
        if !state.has_event_data() {
            return Layout::Auto;
        }
        state.event::<Layout>().copied().unwrap_or_default()
    }

    /// Sets the layout of the current event.
    ///
    /// This is intended to be called from
    /// [`Serialize::serialize`](crate::ser::Serialize::serialize) or a
    /// [`Layer`](crate::ser::Layer), the layout applies to the value that is
    /// serialized.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.event_mut::<Layout>() = self;
    }
}

macro_rules! layout_adapter {
    ($(#[$meta:meta])* $name:ident, $layout:expr) => {
        $(#[$meta])*
        pub struct $name<A = Same>(PhantomData<fn() -> A>);

        impl<T: ?Sized, A: SerializeAs<T>> SerializeAs<T> for $name<A> {
            #[inline]
            fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
                $layout.set(state);
                A::serialize_as(value, state)
            }

            #[inline]
            fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
                A::finish_as(value, state)
            }

            #[inline]
            fn is_optional_as(value: &T) -> bool {
                A::is_optional_as(value)
            }

            #[inline]
            fn container_shape_as(value: &T) -> ContainerShape {
                A::container_shape_as(value)
            }

            fn describe_as(value: &T, d: &mut dyn Describe) {
                A::describe_as(value, d)
            }

            #[inline]
            fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
                $layout.set(state);
                A::__private_begin_as(value, state)
            }

            #[inline]
            fn __private_slice_as_bytes_as(val: &[T]) -> Option<Cow<'_, [u8]>>
            where
                T: Sized,
            {
                A::__private_slice_as_bytes_as(val)
            }
        }

        impl<'de, T, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for $name<A> {
            #[inline]
            fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
                A::deserialize_into_as(out)
            }

            #[inline]
            fn initial_value_as() -> Option<T> {
                A::initial_value_as()
            }

            #[inline]
            fn __private_atom_into_as(
                out: &mut Option<T>,
                atom: Atom,
                state: &mut State,
            ) -> Result<(), Error> {
                A::__private_atom_into_as(out, atom, state)
            }

            #[inline]
            fn __private_borrowed_atom_into_as(
                out: &mut Option<T>,
                atom: Atom<'de>,
                state: &mut State,
            ) -> Result<(), Error> {
                A::__private_borrowed_atom_into_as(out, atom, state)
            }

            #[inline]
            fn __private_is_bytes_as() -> bool {
                A::__private_is_bytes_as()
            }

            #[inline]
            fn __private_vec_from_bytes_as(bytes: Vec<u8>) -> Option<Vec<T>> {
                A::__private_vec_from_bytes_as(bytes)
            }

            #[inline]
            fn __private_array_from_bytes_as<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
                A::__private_array_from_bytes_as(bytes)
            }
        }
    };
}

layout_adapter!(
    /// Serializes a value with [`Layout::Compact`].
    ///
    /// This is an adapter (see [`adapters`](crate::adapters)) for all types,
    /// by default it serializes the value with its own
    /// [`Serialize`](crate::Serialize) implementation.  Another adapter can
    /// be given as parameter (`Compact<Vec<Hex>>`).  It's transparent when
    /// deserializing.
    Compact,
    Layout::Compact
);

layout_adapter!(
    /// Serializes a value with [`Layout::Expanded`].
    ///
    /// This works like [`Compact`].
    Expanded,
    Layout::Expanded
);
