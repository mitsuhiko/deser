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
//! | Hint       | Honored by                                                  |
//! |------------|-------------------------------------------------------------|
//! | [`Layout`] | TOML (inline tables and arrays of tables), YAML (flow style) |
//!
//! Formats can define their own hints and adapters for them with [`Hint`]
//! and [`Hinted`].
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

/// A hint that can be set by the [`Hinted`] adapter.
///
/// Hints are types which set some [event data](crate::State::event) for
/// the value that is serialized.  Formats that define their own hints can
/// use this to provide adapters for them:
///
/// ```
/// use deser::State;
/// use deser::hints::{Hint, Hinted};
///
/// #[derive(Debug, Default, Clone)]
/// pub struct Emphasis(pub bool);
///
/// /// Sets the emphasis hint.
/// pub struct Emphasized;
///
/// impl Hint for Emphasized {
///     fn set(state: &mut State) {
///         state.event_mut::<Emphasis>().0 = true;
///     }
/// }
///
/// /// The adapter: `#[deser(as = Emphasize)]`.
/// pub type Emphasize<A = deser::adapters::Same> = Hinted<Emphasized, A>;
/// ```
pub trait Hint: 'static {
    /// Sets the hint for the value that is serialized.
    fn set(state: &mut State);
}

/// An adapter which sets a [`Hint`] and serializes the value with another
/// adapter.
///
/// This is an adapter (see [`adapters`](crate::adapters)) for all types
/// that the adapter `A` supports, by default ([`Same`]) the value is
/// serialized with its own [`Serialize`](crate::Serialize) implementation.
/// It's transparent when deserializing.  [`Compact`] and [`Expanded`] are
/// such adapters: `Compact<Vec<Hex>>` serializes a `Vec<Vec<u8>>` as hex
/// strings with [`Layout::Compact`].
pub struct Hinted<H, A = Same>(PhantomData<fn() -> (H, A)>);

impl<T: ?Sized, H: Hint, A: SerializeAs<T>> SerializeAs<T> for Hinted<H, A> {
    #[inline]
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
        H::set(state);
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
        H::set(state);
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

impl<'de, T, H: Hint, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for Hinted<H, A> {
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

/// The [`Hint`] for [`Layout::Compact`].
pub struct CompactLayout;

impl Hint for CompactLayout {
    #[inline]
    fn set(state: &mut State) {
        Layout::Compact.set(state);
    }
}

/// The [`Hint`] for [`Layout::Expanded`].
pub struct ExpandedLayout;

impl Hint for ExpandedLayout {
    #[inline]
    fn set(state: &mut State) {
        Layout::Expanded.set(state);
    }
}

/// Serializes a value with [`Layout::Compact`].
///
/// See [`Hinted`], `Compact<A>` uses the adapter `A` for the value.
pub type Compact<A = Same> = Hinted<CompactLayout, A>;

/// Serializes a value with [`Layout::Expanded`].
///
/// See [`Hinted`], `Expanded<A>` uses the adapter `A` for the value.
pub type Expanded<A = Same> = Hinted<ExpandedLayout, A>;
