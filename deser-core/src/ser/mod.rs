//! Generic data structure serialization framework.
//!
//! Serialization in deser is based on the [`Serialize`] trait which produces
//! [`Chunk`] objects.  A serializable object walks an object and produces either
//! an atomic chunk or a chunk containing an emitter which yields further values.
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
//! # Serializing Primitives
//!
//! Primitive values such as integers are trivial to serialize as you just
//! directly return the right type of [`Chunk`] from the serialization method.
//!
//! ```rust
//! use deser::ser::{Serialize, Chunk};
//! use deser::State;
//! use deser::{Atom, Error};
//!
//! struct MyInt(u32);
//!
//! impl Serialize for MyInt {
//!     fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
//!         // one can also just do `self.0.serialize(state)`
//!         Ok(Chunk::Atom(Atom::U64(self.0 as u64)))
//!     }
//! }
//! ```
//!
//! # Serializing Structs
//!
//! To serialize compounds like structs you return a chunk containing an emitter.
//! Note that the emitter returns a [`SerializeHandle`].  If want you want to
//! serialize is not already available the handle can hold a boxed [`Serialize`].
//!
//! ```rust
//! use std::borrow::Cow;
//! use deser::ser::{Serialize, Chunk, StructEmitter, SerializeHandle};
//! use deser::State;
//! use deser::Error;
//!
//! struct User {
//!     id: u32,
//!     username: String,
//! }
//!
//! impl Serialize for User {
//!     fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
//!         // the emitter is allocated in the arena of the serialization
//!         Ok(Chunk::structure(UserEmitter { user: self, index: 0 }, state))
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
use alloc::boxed::Box;
use core::ops::Deref;

use crate::State;
use crate::error::Error;
use crate::event::ContainerShape;

pub(crate) mod begin;
mod boxed;
mod chunk;
mod describe;
mod driver;
#[cfg(feature = "derive")]
pub(crate) mod enums;
#[cfg(feature = "derive")]
pub(crate) mod flatten;
pub(crate) mod impls;
mod layer;
mod serializer;
mod stream;

pub use self::boxed::Boxed;
pub use self::chunk::Chunk;
pub use self::describe::{Describe, Variant, VariantKind, VariantRepr};
pub use self::layer::{Layer, Next};
pub use self::serializer::Serializer;
pub use self::stream::{StreamSerializer, Written};

pub use driver::{EventSink, PausableSink, SerializeDriver};

pub(crate) use self::begin::{
    Begin, BeginKind, FIELDS_END, IndexedSeq, IndexedSeqEmitter, IndexedStruct, PLAIN_BUDGET,
    PlainSink, StructField, atom_cost, plain_atom,
};

/// A handle to a [`Serialize`] type.
///
/// During serialization it common to be in a situation where one needs to
/// return locally constructed [`Serialize`].  This is where
/// [`SerializeHandle`] comes in.  In cases where the [`Serialize`] cannot
/// be borrowed it can be boxed up inside the handle.
///
/// The equivalent for deserialization is the
/// [`SinkHandle`](crate::de::SinkHandle).
pub enum SerializeHandle<'a> {
    /// A borrowed reference to a [`Serialize`].
    Borrowed(&'a dyn Serialize),
    /// A [`Serialize`] owned by the handle (see [`Boxed`]).
    ///
    /// Owned values must be `Send` so that the serialization can move
    /// between threads.
    Owned(Boxed<dyn Serialize + Send + 'a>),
}

impl<'a> Deref for SerializeHandle<'a> {
    type Target = dyn Serialize + 'a;

    fn deref(&self) -> &Self::Target {
        match self {
            SerializeHandle::Borrowed(val) => *val,
            SerializeHandle::Owned(val) => &**val,
        }
    }
}

impl<'a> SerializeHandle<'a> {
    /// Create a borrowed handle to a [`Serialize`].
    pub fn to<S: Serialize + 'a>(val: &'a S) -> SerializeHandle<'a> {
        SerializeHandle::Borrowed(val as &dyn Serialize)
    }

    /// Creates an owned handle to a value in the arena of the serialization.
    ///
    /// This is how owned values are typically created (for instance for
    /// [`Chunk::Forward`]), see [`Boxed`].
    #[inline(always)]
    pub fn arena<S: Serialize + Send + 'a>(val: S, state: &mut State) -> SerializeHandle<'a> {
        SerializeHandle::Owned(boxed::unsize(Boxed::arena(val, state), |x| {
            x as *mut (dyn Serialize + Send + 'a)
        }))
    }

    /// Creates an owned handle to a value on the heap.
    ///
    /// Unlike [`arena`](Self::arena) the value does not need a state and is
    /// independent of any serialization.
    pub fn heap<S: Serialize + Send + 'a>(val: S) -> SerializeHandle<'a> {
        SerializeHandle::Owned(Boxed::from(Box::new(val) as Box<dyn Serialize + Send + 'a>))
    }
}

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
/// [`serialize`](Self::serialize) serializes the value into a [`Chunk`].  For
/// compound values like lists or similar, the piece contains a boxed emitter
/// which can be further processed to walk the embedded compound value.  The
/// [`container_shape`](Self::container_shape) of such values is passed on
/// with the start event of the container.
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
pub trait Serialize: Sync {
    /// Serializes this serializable.
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error>;

    /// Invoked after the serialization finished.
    ///
    /// This is primarily useful to undo some state change in the serializer
    /// state at the end of the processing.
    fn finish(&self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    /// Checks if the current value that would be serialized represents an
    /// optional value.
    ///
    /// This can be used by an emitter to skip over values that are currently
    /// in the optional state.  For instance `Option<T>` returns `true` here if
    /// the value is `None` and the struct emitter created by the `derive` feature
    /// will skip over these if `#[deser(skip_serializing_optionals)]` is set on
    /// the struct.
    fn is_optional(&self) -> bool {
        false
    }

    /// Describes the Rust shape of this value.
    ///
    /// This is only invoked by formats which want to reflect the Rust shape
    /// of values, see [`Describe`].  The default implementation describes
    /// nothing.  Wrappers which serialize as the value they wrap should
    /// describe themselves and then delegate to the wrapped value.
    fn describe(&self, d: &mut dyn Describe) {
        let _ = d;
    }

    /// Returns the shape of this value if it's a map or sequence.
    ///
    /// The shape is passed on with the [`MapStart`](crate::Event::MapStart)
    /// or [`SeqStart`](crate::Event::SeqStart) event, it's ignored for other
    /// values.  The default is [`ContainerShape::new`].
    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
    }

    /// Begins the serialization of this value.
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
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        let shape = self.container_shape();
        Ok(Begin::chunk(self.serialize(state)?, shape, true))
    }

    /// Returns `true` if the values of this type are plain.
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
    fn __private_is_plain() -> bool
    where
        Self: Sized,
    {
        false
    }

    /// Returns `true` if this value is plain.
    ///
    /// This is `true` for all values of plain types and for some values
    /// of other types, like empty sequences and maps or `None`.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    fn __private_is_plain_value(&self) -> bool
    where
        Self: Sized,
    {
        Self::__private_is_plain()
    }

    /// Emits the events of a plain value.
    ///
    /// This is only invoked if
    /// [`__private_is_plain_value`](Self::__private_is_plain_value) returns
    /// `true`.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        let _ = sink;
        unreachable!("not a plain value")
    }

    /// Returns the budget that is left after emitting this plain value at
    /// once, `None` if it does not fit.
    ///
    /// Atoms cost one (long text and bytes more, see `atom_cost`),
    /// containers one plus the costs of their values.  Values that are not
    /// plain are not emitted at once, they cost one.  See `PLAIN_BUDGET`.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    fn __private_plain_cost(&self, budget: usize) -> Option<usize> {
        budget.checked_sub(1)
    }

    /// Hidden internal trait method to allow specializations of bytes.
    ///
    /// This method is used by `u8` and `Vec<T>` / `&[T]` to achieve special
    /// casing of bytes for the serialization system.  It allows a vector of
    /// bytes to be emitted as `Chunk::Bytes` rather than a `Seq`.
    ///
    /// Internal specialization of bytes, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_slice_as_bytes(_val: &[Self]) -> Option<Cow<'_, [u8]>>
    where
        Self: Sized,
    {
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
