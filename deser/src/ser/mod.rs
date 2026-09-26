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
//!     fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
//!         Ok(Chunk::Struct(Box::new(UserEmitter {
//!             user: self,
//!             index: 0,
//!         })))
//!     }
//! }
//!
//! struct UserEmitter<'a> {
//!     user: &'a User,
//!     index: usize,
//! }
//!
//! impl<'a> StructEmitter for UserEmitter<'a> {
//!     fn next(&mut self, _state: &mut State)
//!         -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error>
//!     {
//!         let index = self.index;
//!         self.index += 1;
//!         Ok(match index {
//!             0 => Some(("id".into(), SerializeHandle::to(&self.user.id))),
//!             1 => Some(("username".into(), SerializeHandle::to(&self.user.username))),
//!             _ => None
//!         })
//!     }
//! }
//! ```
use std::borrow::Cow;
use std::ops::Deref;

use crate::State;
use crate::error::Error;
use crate::event::ContainerShape;

mod chunk;
mod describe;
mod driver;
#[cfg(feature = "derive")]
pub(crate) mod enums;
mod impls;
mod layer;

pub use self::chunk::Chunk;
pub use self::describe::{Describe, Variant, VariantKind, VariantRepr};
pub use self::layer::{Layer, Next};

pub use driver::SerializeDriver;

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
    /// A boxed up [`Serialize`].
    Owned(Box<dyn Serialize + 'a>),
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

    /// Create an owned handle to a heap allocated [`Serialize`].
    pub fn boxed<S: Serialize + 'a>(val: S) -> SerializeHandle<'a> {
        SerializeHandle::Owned(Box::new(val))
    }
}

/// The result of [`Serialize::__private_begin`].
#[doc(hidden)]
pub struct Begin<'a> {
    pub(crate) kind: BeginKind<'a>,
    pub(crate) shape: ContainerShape,
    pub(crate) needs_finish: bool,
}

pub(crate) enum BeginKind<'a> {
    Chunk(Chunk<'a>),
    Struct(&'a dyn IndexedStruct),
    Seq(&'a dyn IndexedSeq),
}

impl<'a> Begin<'a> {
    /// Begins a value with a chunk.
    #[inline]
    pub fn chunk(chunk: Chunk<'a>, shape: ContainerShape, needs_finish: bool) -> Begin<'a> {
        Begin {
            kind: BeginKind::Chunk(chunk),
            shape,
            needs_finish,
        }
    }

    /// Begins a struct which provides its fields by index.
    ///
    /// This is equivalent to a [`Chunk::Struct`] but does not require an
    /// emitter to be allocated.  `finish` is not invoked.
    #[inline]
    pub fn indexed_struct(value: &'a dyn IndexedStruct, shape: ContainerShape) -> Begin<'a> {
        Begin {
            kind: BeginKind::Struct(value),
            shape,
            needs_finish: false,
        }
    }

    /// Begins a sequence which provides its elements by index.
    ///
    /// This is equivalent to a [`Chunk::Seq`] but does not require an
    /// emitter to be allocated.  `finish` is not invoked.
    #[inline]
    pub fn indexed_seq(value: &'a dyn IndexedSeq, shape: ContainerShape) -> Begin<'a> {
        Begin {
            kind: BeginKind::Seq(value),
            shape,
            needs_finish: false,
        }
    }
}

/// A field of an [`IndexedStruct`].
#[doc(hidden)]
pub enum StructField<'a> {
    /// A field with key and value.
    Field(&'a str, SerializeHandle<'a>),
    /// The field is skipped.
    Skip,
    /// There are no more fields.
    End,
}

/// A struct which provides its fields by index.
///
/// The fields are requested with increasing indexes starting at zero until
/// [`StructField::End`] is returned.
#[doc(hidden)]
pub trait IndexedStruct {
    fn field(&self, index: usize, state: &mut State) -> Result<StructField<'_>, Error>;
}

/// A struct emitter for an [`IndexedStruct`].
#[doc(hidden)]
pub struct IndexedStructEmitter<'a> {
    fields: &'a dyn IndexedStruct,
    index: usize,
}

impl<'a> IndexedStructEmitter<'a> {
    pub fn new(fields: &'a dyn IndexedStruct) -> IndexedStructEmitter<'a> {
        IndexedStructEmitter { fields, index: 0 }
    }
}

impl<'a> StructEmitter for IndexedStructEmitter<'a> {
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        loop {
            let field = self.fields.field(self.index, state)?;
            self.index += 1;
            match field {
                StructField::Field(key, value) => return Ok(Some((Cow::Borrowed(key), value))),
                StructField::Skip => continue,
                StructField::End => return Ok(None),
            }
        }
    }
}

/// A sequence which provides its elements by index.
///
/// The elements are requested with increasing indexes starting at zero
/// until `None` is returned.
#[doc(hidden)]
pub trait IndexedSeq {
    fn element(
        &self,
        index: usize,
        state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error>;
}

/// A struct emitter.
///
/// A struct emitter is a simplified version of a [`MapEmitter`] which produces struct
/// field and value in one go.  The object model itself however does not know structs,
/// it only knows about maps.
pub trait StructEmitter {
    /// Produces the next field and value in the struct.
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error>;
}

/// A map emitter.
pub trait MapEmitter {
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
pub trait SeqEmitter {
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
pub trait Serialize {
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
    #[doc(hidden)]
    #[inline]
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        let shape = self.container_shape();
        Ok(Begin::chunk(self.serialize(state)?, shape, true))
    }

    /// Hidden internal trait method to allow specializations of bytes.
    ///
    /// This method is used by `u8` and `Vec<T>` / `&[T]` to achieve special
    /// casing of bytes for the serialization system.  It allows a vector of
    /// bytes to be emitted as `Chunk::Bytes` rather than a `Seq`.
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
    let mut m = std::collections::BTreeMap::new();
    m.insert(true, vec![vec![&b"x"[..], b"yyy"], vec![b"zzzz"]]);
    m.insert(false, vec![]);

    let mut driver = SerializeDriver::new(&m);
    while let Some((event, _, _)) = driver.next().unwrap() {
        v.push(format!("{:?}", event));
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
