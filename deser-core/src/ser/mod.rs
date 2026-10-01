//! Generic data structure serialization framework.
//!
//! Serialization in deser is based on the [`Serialize`] trait which produces
//! [`Emit`] values.  A serializable value either emits an atom or an emitter
//! which yields further values.
//!
//! # Streaming Serialization
//!
//! For convenient serialization, Deser provides a [`SerializeDriver`] that allows
//! streaming serialization of values.  A driver can be created by passing a reference
//! to a [`Serialize`] value to the constructor.  Then [`next`](SerializeDriver::next)
//! is called repeatedly until no more events are produced.
//!
//! ```
//! # use deser::ser::SerializeDriver;
//! # fn do_it() -> Result<(), deser::Error> {
//! let serializable = vec!["foo", "bar", "baz"];
//! let mut driver = SerializeDriver::new(&serializable);
//! while let Some((_event, _value, _state)) = driver.next()? {
//!     // serialize each event for the target format such as JSON
//! }
//! # Ok(()) } do_it().unwrap();
//! ```
//!
//! This type of interface also permits the serialization of almost unlimited depth.
//!
//! The serializers of data formats implement the [`Serializer`] trait which
//! receives the events of a value from a driver.  [`Layer`]s sit between
//! the values and the format and see every event, for instance to rename
//! keys or to redact values.  They are added with
//! [`SerializeDriver::push_layer`] (for instance in
//! [`Serializer::serialize_with`]).
//!
//! # Serializing Primitives
//!
//! Primitive values such as integers are trivial to serialize as you just
//! directly return the right type of [`Emit`] from the serialization method.
//!
//! ```rust
//! use deser::ser::{Serialize, Emit};
//! use deser::State;
//! use deser::{Atom, Error};
//!
//! struct MyInt(u32);
//!
//! impl Serialize for MyInt {
//!     fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
//!         // one can also just do `u32::serialize(&value.0, state)`
//!         Ok(Emit::Atom(Atom::U64(value.0 as u64)))
//!     }
//! }
//! ```
//!
//! # Serializing Structs
//!
//! To serialize compounds like structs you return an [`Emit`] holding an
//! emitter.  The emitter hands out the values of the fields as
//! [`SerializeHandle`]s: a handle borrows the value if it exists already,
//! otherwise it can own it (see [`SerializeHandle::arena`]).
//!
//! ```rust
//! use std::borrow::Cow;
//! use deser::ser::{Serialize, Emit, StructEmitter, SerializeHandle};
//! use deser::State;
//! use deser::Error;
//!
//! struct User {
//!     id: u32,
//!     username: String,
//! }
//!
//! impl Serialize for User {
//!     fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
//!         // the emitter is allocated in the arena of the state
//!         Ok(Emit::structure(UserEmitter { user: value, index: 0 }, state))
//!     }
//! }
//!
//! struct UserEmitter<'a> {
//!     user: &'a User,
//!     index: usize,
//! }
//!
//! impl<'a> StructEmitter for UserEmitter<'a> {
//!     fn next(
//!         &mut self,
//!         _state: &mut State,
//!     ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error>
//!     {
//!         let index = self.index;
//!         self.index += 1;
//!         Ok(match index {
//!             0 => Some(("id".into(), SerializeHandle::to(&self.user.id))),
//!             1 => Some((
//!                 "username".into(),
//!                 SerializeHandle::to(&self.user.username),
//!             )),
//!             _ => None,
//!         })
//!     }
//! }
//! ```
use alloc::borrow::Cow;

use crate::State;
use crate::error::Error;
use crate::event::ContainerShape;

pub(crate) mod begin;
mod boxed;
mod describe;
mod driver;
mod emit;
#[cfg(feature = "derive")]
pub(crate) mod enums;
#[cfg(feature = "derive")]
pub(crate) mod flatten;
mod handle;
pub(crate) mod impls;
mod layer;
mod serializer;
mod stream;

pub use self::boxed::Boxed;
pub use self::describe::{Describe, Variant, VariantKind, VariantRepr};
pub use self::emit::Emit;
pub(crate) use self::handle::{Adapted, Erased, HandleInner};
pub use self::handle::{SerializeHandle, SerializeRef};
pub use self::layer::{Layer, Next};
pub use self::serializer::Serializer;
pub use self::stream::StreamSerializer;

pub use driver::{EventSink, SerializeDriver};

pub(crate) use self::begin::{
    Begin, BeginKind, FIELDS_END, IndexedSeq, IndexedSeqEmitter, IndexedStruct, PLAIN_BUDGET,
    PlainSink, StructField, atom_cost, plain_atom,
};

/// A struct emitter.
///
/// A struct emitter is a simplified version of a [`MapEmitter`] which produces struct
/// field and value in one go.  The object model itself however does not know structs,
/// it only knows about maps.
pub trait StructEmitter: Send {
    /// Produces the next field and value in the struct.
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error>;
}

/// A map emitter.
pub trait MapEmitter: Send {
    /// Produces the next key in the map.
    ///
    /// If this reached the end of the map `None` shall be returned.  The expectation
    /// is that this method changes an internal state in the emitter and the next
    /// call to [`next_value`](Self::next_value) returns the corresponding value.
    fn next_key(&mut self, state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error>;

    /// Produces the next value in the map.
    ///
    /// # Panics
    ///
    /// This method shall panic if the emitter is not able to produce a value because
    /// the emitter is in the wrong state.
    fn next_value(&mut self, state: &mut State) -> Result<SerializeHandle<'_>, Error>;
}

/// A sequence emitter.
pub trait SeqEmitter: Send {
    /// Produces the next item in the sequence.
    fn next(&mut self, state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error>;
}

/// A data structure that can be serialized into any data format supported by Deser.
///
/// [`serialize`](Self::serialize) serializes the value into an [`Emit`].  For
/// compound values like lists or structs, it holds an emitter which
/// hands out the values the compound value contains.  The
/// [`container_shape`](Self::container_shape) of such values is passed on
/// with the start event of the container.
///
/// # Adapters
///
/// The type parameter `T` is the type of the value that is serialized.  It
/// defaults to `Self`: `impl Serialize for Foo` serializes `Foo` values.  A
/// type that implements `Serialize` for another type is an adapter, it
/// serializes values of that type on their behalf (see
/// [`adapters`](crate::adapters)):
///
/// ```
/// use deser::ser::{Emit, Serialize};
/// use deser::{Atom, Error, State};
///
/// /// Serializes a `u32` as string.
/// pub struct AsString;
///
/// impl Serialize<u32> for AsString {
///     fn serialize<'a>(value: &'a u32, _state: &mut State) -> Result<Emit<'a>, Error> {
///         Ok(Emit::Atom(Atom::Str(value.to_string().into())))
///     }
/// }
/// ```
///
/// Adapters are never instantiated, only their functions are used.  The
/// serializers of the data formats receive the values as [`SerializeRef`],
/// a reference to the value with its type (and adapter) erased.
///
/// # Thread Safety
///
/// Serializables are `Sync` and the emitters they create are `Send`.  This
/// allows an ongoing serialization (a [`SerializeDriver`]) to move between
/// threads, for instance when it is suspended while the output is written
/// asynchronously.  Types with shared ownership or interior mutability that
/// is not thread safe (such as `Rc` or `RefCell`) cannot be serialized.
/// `Mutex` and `RwLock` cannot be serialized either as the lock guard would
/// have to be held while the serialization moves between threads.
pub trait Serialize<T: ?Sized = Self>: Sync {
    /// Serializes the value.
    fn serialize<'a>(value: &'a T, state: &mut State) -> Result<Emit<'a>, Error>;

    /// Invoked after the serialization finished.
    ///
    /// This is primarily useful to undo some state change in the serializer
    /// state at the end of the processing.
    fn finish(value: &T, state: &mut State) -> Result<(), Error> {
        let _ = (value, state);
        Ok(())
    }

    /// Checks if the value represents an optional value.
    ///
    /// This can be used by an emitter to skip over values that are currently
    /// in the optional state.  For instance `Option<T>` returns `true` here if
    /// the value is `None` and the struct emitter created by the `derive` feature
    /// will skip over these if `#[deser(skip_serializing_optionals)]` is set on
    /// the struct.
    fn is_optional(value: &T) -> bool {
        let _ = value;
        false
    }

    /// Describes the Rust shape of the value.
    ///
    /// This is only invoked by formats which want to reflect the Rust shape
    /// of values, see [`Describe`].  The default implementation describes
    /// nothing.  Wrappers which serialize as the value they wrap should
    /// describe themselves and then delegate to the wrapped value.
    fn describe(value: &T, d: &mut dyn Describe) {
        let _ = (value, d);
    }

    /// Returns the shape of the value if it's a map or sequence.
    ///
    /// The shape is passed on with the [`MapStart`](crate::Event::MapStart)
    /// or [`SeqStart`](crate::Event::SeqStart) event, it's ignored for other
    /// values.  The default is [`ContainerShape::new`].
    fn container_shape(value: &T) -> ContainerShape {
        let _ = value;
        ContainerShape::new()
    }

    /// Begins the serialization of the value.
    ///
    /// Returns the [`container_shape`](Self::container_shape), the result of
    /// [`serialize`](Self::serialize) and a flag that indicates if
    /// [`finish`](Self::finish) needs to be invoked.  The default
    /// implementation calls both methods (in this order) and always requests
    /// `finish` to be invoked.  Types which do not override `finish` can
    /// implement this so that the driver needs a single call per value.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    fn __private_begin<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        let shape = <Self as Serialize<T>>::container_shape(value);
        Ok(Begin::emit(
            <Self as Serialize<T>>::serialize(value, state)?,
            shape,
            true,
        ))
    }

    /// Returns `true` if the values are plain.
    ///
    /// Plain values serialize as an atom or a sequence of plain values,
    /// independent of the state and without `finish`.  The driver emits
    /// sequences of plain values without driving every value on its own
    /// (see [`PlainSink`]).  Types which are plain implement
    /// [`__private_emit_plain`](Self::__private_emit_plain) which has to
    /// produce the same events as serializing the value.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    fn __private_is_plain() -> bool {
        false
    }

    /// Returns `true` if the value is plain.
    ///
    /// This is `true` for all values of plain types and for some values
    /// of other types, like empty sequences and maps or `None`.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    fn __private_is_plain_value(value: &T) -> bool {
        let _ = value;
        <Self as Serialize<T>>::__private_is_plain()
    }

    /// Emits the events of a plain value.
    ///
    /// This is only invoked if
    /// [`__private_is_plain_value`](Self::__private_is_plain_value) returns
    /// `true`.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_emit_plain(value: &T, sink: &mut dyn PlainSink) -> Result<(), Error> {
        let _ = (value, sink);
        unreachable!("not a plain value")
    }

    /// Returns the budget that is left after emitting the plain value at
    /// once, `None` if it does not fit.
    ///
    /// Atoms cost one (long text and bytes more, see `atom_cost`),
    /// containers one plus the costs of their values.  Values that are not
    /// plain are not emitted at once, they cost one.  See `PLAIN_BUDGET`.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    fn __private_plain_cost(value: &T, budget: usize) -> Option<usize> {
        let _ = value;
        budget.checked_sub(1)
    }

    /// Hidden internal trait method to allow specializations of bytes.
    ///
    /// This method is used by `u8` and `Vec<T>` / `&[T]` to achieve special
    /// casing of bytes for the serialization system.  It allows a vector of
    /// bytes to be emitted as `Emit::Bytes` rather than a `Seq`.
    ///
    /// Internal specialization of bytes, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_slice_as_bytes(val: &[T]) -> Option<Cow<'_, [u8]>>
    where
        T: Sized,
    {
        let _ = val;
        None
    }
}

#[test]
fn test_serialize() {
    let mut v = Vec::new();
    let mut m = alloc::collections::BTreeMap::new();
    m.insert(true, vec![vec![&b"x"[..], b"yyy"], vec![b"zzzz"]]);
    m.insert(false, vec![]);

    let mut driver = SerializeDriver::new(&m);
    while let Some((event, _, _)) = driver.next().unwrap() {
        v.push(format!(
            "{:?}",
            crate::event::without_len(event.to_static())
        ));
    }

    assert_eq!(
        &v[..],
        [
            "MapStart(ContainerShape { len: None, order: Sorted })",
            "Atom(Bool(false))",
            "SeqStart",
            "SeqEnd",
            "Atom(Bool(true))",
            "SeqStart",
            "SeqStart",
            "Atom(Bytes([120]))",
            "Atom(Bytes([121, 121, 121]))",
            "SeqEnd",
            "SeqStart",
            "Atom(Bytes([122, 122, 122, 122]))",
            "SeqEnd",
            "SeqEnd",
            "MapEnd",
        ]
    );
}
