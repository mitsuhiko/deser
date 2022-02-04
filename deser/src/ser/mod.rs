//! Generic data structure serialization framework.
//!
//! Serialization in deser is based on the [`Serializable`] trait which produces
//! [`Chunk`] objects.  A serializable object walks an object and produces either
//! an atomic chunk or a chunk containing an emitter which yields further values.
//!
//! This allows the system to support unlimited recursion.  This is tricky to with
//! the borrow checker due to lifetimes.  The [`for_each_event`] function is provided
//! which calls a callback for each event in the produced chunks as a safe convenience
//! API.
//!
//! # Serializing primitives
//!
//! Primitives are trivial to serialize as you just directly return the right type
//! of [`Chunk`] from the serialization method.  In this example we also provide
//! an optional [`Descriptor`] which can help serializers make better decisions.
//!
//! ```rust
//! use deser::ser::{Serializable, SerializerState, Chunk};
//! use deser::{Descriptor, Error};
//!
//! struct MyInt(u32);
//!
//! #[derive(Debug)]
//! struct MyIntDescriptor;
//!
//! impl Descriptor for MyIntDescriptor {
//!     fn name(&self) -> Option<&str> {
//!         Some("MyInt")
//!     }
//!
//!     fn precision(&self) -> Option<usize> {
//!         Some(32)
//!     }
//! }
//!
//! impl Serializable for MyInt {
//!     fn descriptor(&self) -> &dyn Descriptor {
//!         &MyIntDescriptor
//!     }
//!
//!     fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
//!         Ok(Chunk::U64(self.0 as u64))
//!     }
//! }
//! ```
//!
//! # Serializing structs
//!
//! To serialize compounds like structs you return a chunk containing an emitter.
//! Note that the emitter returns [`&dyn Serializable`](crate::ser::Serializable) references.
//! If want you want to serialize is not already available so you can borrow from,
//! you an stash away a temporary value on the emitter itself and return a loan to it.
//!
//! ```rust
//! use std::borrow::Cow;
//! use deser::ser::{Serializable, SerializerState, Chunk, StructEmitter, SerializableHandle};
//! use deser::Error;
//!
//! struct User {
//!     id: u32,
//!     username: String,
//! }
//!
//! impl Serializable for User {
//!     fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
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
//!     fn next(&mut self) -> Option<(Cow<'_, str>, SerializableHandle)> {
//!         let index = self.index;
//!         self.index += 1;
//!         match index {
//!             0 => Some((Cow::Borrowed("id"), SerializableHandle::Borrowed(&self.user.id))),
//!             1 => Some((Cow::Borrowed("username"), SerializableHandle::Borrowed(&self.user.username))),
//!             _ => None
//!         }
//!     }
//! }
//! ```
use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::Deref;

use crate::descriptors::{Descriptor, NullDescriptor};
use crate::error::Error;
use crate::event::Event;
use crate::extensions::Extensions;

mod impls;

/// Abstraction over borrowed and owned serializable
pub enum SerializableHandle<'a> {
    Borrowed(&'a dyn Serializable),
    Owned(Box<dyn Serializable + 'a>),
}

impl<'a> Deref for SerializableHandle<'a> {
    type Target = dyn Serializable + 'a;

    fn deref(&self) -> &Self::Target {
        match self {
            SerializableHandle::Borrowed(val) => &**val,
            SerializableHandle::Owned(val) => &**val,
        }
    }
}

/// A chunk represents the minimum state necessary to serialize a value.
///
/// Chunks are of two types: atomic primitives and stateful emitters.
/// For instance `Chunk::Bool(true)` is an atomic primitive.  It can be emitted
/// to a serializer directly.  On the other hand a `Chunk::Map` contains a
/// stateful emitter that keeps yielding values until it's done walking over
/// the map.
pub enum Chunk<'a> {
    Null,
    Bool(bool),
    Str(Cow<'a, str>),
    Bytes(Cow<'a, [u8]>),
    U64(u64),
    I64(i64),
    F64(f64),
    Struct(Box<dyn StructEmitter + 'a>),
    Map(Box<dyn MapEmitter + 'a>),
    Seq(Box<dyn SeqEmitter + 'a>),
}

enum Layer<'a> {
    Struct(Box<dyn StructEmitter + 'a>),
    Map(Box<dyn MapEmitter + 'a>, bool),
    Seq(Box<dyn SeqEmitter + 'a>),
}

impl<'a> fmt::Debug for Layer<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Struct(_) => f.debug_tuple("StructEmitter").finish(),
            Self::Map(..) => f.debug_tuple("MapEmitter").finish(),
            Self::Seq(_) => f.debug_tuple("SeqEmitter").finish(),
        }
    }
}

/// The current state of the serializer.
///
/// During serializer the [`SerializerState`] acts as a communciation device between
/// the serializable types as the serializer.
#[derive(Debug)]
pub struct SerializerState<'a> {
    extensions: Extensions,
    stack: ManuallyDrop<Vec<(&'a dyn Descriptor, Layer<'a>)>>,
}

impl<'a> Drop for SerializerState<'a> {
    fn drop(&mut self) {
        // it's important that we drop the values in inverse order.
        while let Some(_last) = self.stack.pop() {
            // drop in inverse order
        }
        unsafe {
            ManuallyDrop::drop(&mut self.stack);
        }
    }
}

impl<'a> SerializerState<'a> {
    /// Returns an extension value.
    pub fn get<T: Default + fmt::Debug + 'static>(&self) -> Ref<'_, T> {
        self.extensions.get()
    }

    /// Returns a mutable extension value.
    pub fn get_mut<T: Default + fmt::Debug + 'static>(&self) -> RefMut<'_, T> {
        self.extensions.get_mut()
    }

    /// Returns the current recursion depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Returns the topmost descriptor.
    ///
    /// This descriptor always points to a container as the descriptor of a value itself
    /// will always be passed to the callback explicitly.
    pub fn top_descriptor(&self) -> Option<&dyn Descriptor> {
        self.stack.last().map(|x| x.0)
    }
}

/// Invokes a callback for each event of a serializable.
///
/// Deser understands the complexities of recursive structures.  This function will
/// invoke the callback for every [`Event`] produced from the serialization system.
/// It does so without recursion so the call stack stays flat.
///
/// The callback is invoked with three arguments: the current [`Event`], the top most
/// [`Descriptor`] and the current [`SerializerState`].
pub fn for_each_event<F>(serializable: &dyn Serializable, mut f: F) -> Result<(), Error>
where
    F: FnMut(&Event, &dyn Descriptor, &SerializerState) -> Result<(), Error>,
{
    let mut serializable = SerializableHandle::Borrowed(serializable);
    let mut state = SerializerState {
        extensions: Extensions::default(),
        stack: ManuallyDrop::new(Vec::new()),
    };

    macro_rules! extended_serializable {
        () => {
            extend_lifetime!(&serializable, &SerializableHandle)
        };
    }

    let mut chunk = unsafe { extended_serializable!() }.serialize(&state)?;
    let mut descriptor = unsafe { extended_serializable!() }.descriptor();

    loop {
        let (event, emitter_opt) = match chunk {
            Chunk::Null => (Event::Null, None),
            Chunk::Bool(value) => (Event::Bool(value), None),
            Chunk::Str(value) => (Event::Str(value), None),
            Chunk::Bytes(value) => (Event::Bytes(value), None),
            Chunk::U64(value) => (Event::U64(value), None),
            Chunk::I64(value) => (Event::I64(value), None),
            Chunk::F64(value) => (Event::F64(value), None),
            Chunk::Struct(emitter) => (Event::MapStart, Some(Layer::Struct(emitter))),
            Chunk::Map(emitter) => (Event::MapStart, Some(Layer::Map(emitter, false))),
            Chunk::Seq(emitter) => (Event::SeqStart, Some(Layer::Seq(emitter))),
        };
        let done = emitter_opt.is_none();
        if let Some(emitter) = emitter_opt {
            state.stack.push((descriptor, emitter));
        }
        f(&event, descriptor, &state)?;
        if done {
            serializable.finish(&state)?;
        }
        loop {
            // special case: close down the key before going to value
            if let Some(layer) = state.stack.last() {
                if let Layer::Map(_, true) = layer.1 {
                    serializable.finish(&state)?;
                }
            }

            if let Some(layer) = state.stack.last_mut() {
                match layer.1 {
                    Layer::Struct(ref mut s) => {
                        // this is safe as we maintain our own stack.
                        match unsafe {
                            extend_lifetime!(s.next(), Option<(Cow<str>, SerializableHandle)>)
                        } {
                            Some((key, value)) => {
                                let key_descriptor = key.descriptor();
                                f(&Event::Str(Cow::Borrowed(&key)), key_descriptor, &state)?;
                                serializable = value;
                                chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                                descriptor = unsafe { extended_serializable!() }.descriptor();
                                break;
                            }
                            None => f(&Event::MapEnd, layer.0, &state)?,
                        }
                    }
                    Layer::Map(ref mut m, ref mut feed_value) => {
                        let old_feed_value = *feed_value;
                        *feed_value = !old_feed_value;
                        if old_feed_value {
                            let value =
                                unsafe { extend_lifetime!(m.next_value(), SerializableHandle) };
                            serializable = value;
                            chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                            descriptor = unsafe { extended_serializable!() }.descriptor();
                            break;
                        }
                        // this is safe as we maintain our own stack.
                        match unsafe { extend_lifetime!(m.next_key(), Option<SerializableHandle>) }
                        {
                            Some(key) => {
                                serializable = key;
                                chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                                descriptor = unsafe { extended_serializable!() }.descriptor();
                                break;
                            }
                            None => f(&Event::MapEnd, layer.0, &state)?,
                        }
                    }
                    Layer::Seq(ref mut seq) => {
                        // this is safe as we maintain our own stack.
                        match unsafe { extend_lifetime!(seq.next(), Option<SerializableHandle>) } {
                            Some(next) => {
                                serializable = next;
                                chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                                descriptor = unsafe { extended_serializable!() }.descriptor();
                                break;
                            }
                            None => f(&Event::SeqEnd, layer.0, &state)?,
                        }
                    }
                }
            } else {
                return Ok(());
            }

            state.stack.pop();
            serializable.finish(&state)?;
        }
    }
}

/// A struct emitter.
pub trait StructEmitter {
    /// Produces the next field and value in the struct.
    fn next(&mut self) -> Option<(Cow<'_, str>, SerializableHandle)>;
}

/// A map emitter.
pub trait MapEmitter {
    /// Produces the next key in the map.
    ///
    /// If this reached the end of the map `None` shall be returned.  The expectation
    /// is that this method changes an internal state in the emitter and the next
    /// call to [`next_value`](Self::next_value) returns the corresponding value.
    fn next_key(&mut self) -> Option<SerializableHandle>;

    /// Produces the next value in the map.
    ///
    /// # Panics
    ///
    /// This method shall panic if the emitter is not able to produce a value because
    /// the emitter is in the wrong state.
    fn next_value(&mut self) -> SerializableHandle;
}

/// A sequence emitter.
pub trait SeqEmitter {
    /// Produces the next item in the sequence.
    fn next(&mut self) -> Option<SerializableHandle>;
}

/// A data structure that can be serialized into any data format supported by Deser.
///
/// This trait provides two things:
///
/// * [`descriptor`](Self::descriptor) returns a reference to the closest descriptor
///   of this value.  The descriptor provides auxiliary information about the value
///   that the serialization system does not expose.
/// * [`serialize`](Self::serialize) serializes the value into a [`Chunk`].  For
///   compound values like lists or similar, the piece contains a boxed emitter
///   which can be further processed to walk the embedded compound value.
pub trait Serializable {
    /// Returns the descriptor of this serializable if it exists.
    fn descriptor(&self) -> &dyn Descriptor {
        &NullDescriptor
    }

    /// Serializes this serializable.
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error>;

    /// Invoked after the serialization finished.
    ///
    /// This is primarily useful to undo some state change in the serializer
    /// state at the end of the processing.
    fn finish(&self, _state: &SerializerState) -> Result<(), Error> {
        Ok(())
    }

    /// Hidden internal trait method to allow specializations of bytes.
    ///
    /// This method is used by `u8` and `Vec<T>` / `&[T]` to achieve special
    /// casing of bytes for the serialization system.  It allows a vector of
    /// bytes to be emitted as `Chunk::Bytes` rather than a `Seq`.
    #[doc(hidden)]
    fn __private_slice_as_bytes(_val: &[Self]) -> Option<&[u8]>
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

    for_each_event(&m, |event, _, _| {
        v.push(format!("{:?}", event));
        Ok(())
    })
    .unwrap();

    assert_eq!(
        &v[..],
        [
            "MapStart",
            "Bool(false)",
            "SeqStart",
            "SeqEnd",
            "Bool(true)",
            "SeqStart",
            "SeqStart",
            "Bytes([120])",
            "Bytes([121, 121, 121])",
            "SeqEnd",
            "SeqStart",
            "Bytes([122, 122, 122, 122])",
            "SeqEnd",
            "SeqEnd",
            "MapEnd",
        ]
    );
}
