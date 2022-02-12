//! Generic data structure serialization framework.
//!
//! Serialization in deser is based on the [`Serialize`] trait which produces
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
//! use deser::ser::{Serialize, SerializerState, Chunk};
//! use deser::{Atom, Descriptor, Error};
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
//! impl Serialize for MyInt {
//!     fn descriptor(&self) -> &dyn Descriptor {
//!         &MyIntDescriptor
//!     }
//!
//!     fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
//!         // one can also just do `self.0.serialize(state)`
//!         Ok(Chunk::Atom(Atom::U64(self.0 as u64)))
//!     }
//! }
//! ```
//!
//! # Serializing structs
//!
//! To serialize compounds like structs you return a chunk containing an emitter.
//! Note that the emitter returns a [`SerializeHandle`].  If want you want to
//! serialize is not already available the handle can hold a boxed [`Serialize`].
//!
//! ```rust
//! use std::borrow::Cow;
//! use deser::ser::{Serialize, SerializerState, Chunk, StructEmitter, SerializeHandle};
//! use deser::Error;
//!
//! struct User {
//!     id: u32,
//!     username: String,
//! }
//!
//! impl Serialize for User {
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
//!     fn next(&mut self, _state: &SerializerState)
//!         -> Result<Option<(Cow<'_, str>, SerializeHandle)>, Error>
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
use std::cell::{Ref, RefMut};
use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::Deref;

use crate::descriptors::{Descriptor, NullDescriptor};
use crate::error::Error;
use crate::event::{Atom, Event};
use crate::extensions::Extensions;

mod chunk;
mod driver;
mod impls;

pub use self::chunk::Chunk;

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
            SerializeHandle::Borrowed(val) => &**val,
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
pub struct SerializerState<'a> {
    extensions: Extensions,
    descriptor_stack: Vec<&'a dyn Descriptor>,
}

impl<'a> fmt::Debug for SerializerState<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Stack<'a>(&'a [&'a dyn Descriptor]);
        struct Entry<'a>(&'a dyn Descriptor);

        impl<'a> fmt::Debug for Entry<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Layer")
                    .field("type_name", &self.0.name())
                    .field("precision", &self.0.precision())
                    .field("unordered", &self.0.unordered())
                    .finish()
            }
        }

        impl<'a> fmt::Debug for Stack<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut l = f.debug_list();
                for item in self.0.iter() {
                    l.entry(&Entry(*item));
                }
                l.finish()
            }
        }

        f.debug_struct("SerializerState")
            .field("extensions", &self.extensions)
            .field("stack", &Stack(&self.descriptor_stack))
            .finish()
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
        self.descriptor_stack.len()
    }

    /// Returns the topmost descriptor.
    ///
    /// This descriptor always points to a container as the descriptor of a value itself
    /// will always be passed to the callback explicitly.
    pub fn top_descriptor(&self) -> Option<&dyn Descriptor> {
        self.descriptor_stack.last().copied()
    }
}

#[derive(Default)]
struct EmitterStack<'a> {
    layers: ManuallyDrop<Vec<Layer<'a>>>,
}

impl<'a> Drop for EmitterStack<'a> {
    fn drop(&mut self) {
        while let Some(_item) = self.layers.pop() {
            // drop in inverse order
        }
        unsafe {
            ManuallyDrop::drop(&mut self.layers);
        }
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
pub fn for_each_event<F>(serializable: &dyn Serialize, mut callback: F) -> Result<(), Error>
where
    F: FnMut(Event, &dyn Descriptor, &SerializerState) -> Result<(), Error>,
{
    let mut serializable = SerializeHandle::Borrowed(serializable);
    let mut state = SerializerState {
        extensions: Extensions::default(),
        descriptor_stack: Vec::new(),
    };
    let mut emitter_stack = EmitterStack::default();

    macro_rules! extended_serializable {
        () => {
            extend_lifetime!(&serializable, &SerializeHandle)
        };
    }

    let mut chunk = unsafe { extended_serializable!() }.serialize(&state)?;
    let mut descriptor = unsafe { extended_serializable!() }.descriptor();

    loop {
        let (event, emitter_opt) = match chunk {
            Chunk::Atom(atom) => (Event::Atom(atom), None),
            Chunk::Struct(emitter) => (Event::MapStart, Some(Layer::Struct(emitter))),
            Chunk::Map(emitter) => (Event::MapStart, Some(Layer::Map(emitter, false))),
            Chunk::Seq(emitter) => (Event::SeqStart, Some(Layer::Seq(emitter))),
        };
        let done = emitter_opt.is_none();
        if let Some(emitter) = emitter_opt {
            state.descriptor_stack.push(descriptor);
            emitter_stack.layers.push(emitter);
        }
        callback(event, descriptor, &state)?;
        if done {
            serializable.finish(&state)?;
        }
        loop {
            // special case: close down the key before going to value
            if let Some(Layer::Map(_, true)) = emitter_stack.layers.last() {
                serializable.finish(&state)?;
            }

            if let Some(layer) = emitter_stack.layers.last_mut() {
                match layer {
                    Layer::Struct(ref mut s) => {
                        // this is safe as we maintain our own stack.
                        match unsafe {
                            extend_lifetime!(s.next(&state)?, Option<(Cow<str>, SerializeHandle)>)
                        } {
                            Some((key, value)) => {
                                let key_descriptor = key.descriptor();
                                callback(
                                    Event::Atom(Atom::Str(Cow::Borrowed(&key))),
                                    key_descriptor,
                                    &state,
                                )?;
                                serializable = value;
                                chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                                descriptor = unsafe { extended_serializable!() }.descriptor();
                                break;
                            }
                            None => {
                                callback(Event::MapEnd, state.top_descriptor().unwrap(), &state)?
                            }
                        }
                    }
                    Layer::Map(ref mut m, ref mut feed_value) => {
                        let old_feed_value = *feed_value;
                        *feed_value = !old_feed_value;
                        if old_feed_value {
                            let value =
                                unsafe { extend_lifetime!(m.next_value(&state)?, SerializeHandle) };
                            serializable = value;
                            chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                            descriptor = unsafe { extended_serializable!() }.descriptor();
                            break;
                        }
                        // this is safe as we maintain our own stack.
                        match unsafe {
                            extend_lifetime!(m.next_key(&state)?, Option<SerializeHandle>)
                        } {
                            Some(key) => {
                                serializable = key;
                                chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                                descriptor = unsafe { extended_serializable!() }.descriptor();
                                break;
                            }
                            None => {
                                callback(Event::MapEnd, state.top_descriptor().unwrap(), &state)?
                            }
                        }
                    }
                    Layer::Seq(ref mut seq) => {
                        // this is safe as we maintain our own stack.
                        match unsafe {
                            extend_lifetime!(seq.next(&state)?, Option<SerializeHandle>)
                        } {
                            Some(next) => {
                                serializable = next;
                                chunk = unsafe { extended_serializable!() }.serialize(&state)?;
                                descriptor = unsafe { extended_serializable!() }.descriptor();
                                break;
                            }
                            None => {
                                callback(Event::SeqEnd, state.top_descriptor().unwrap(), &state)?
                            }
                        }
                    }
                }
            } else {
                return Ok(());
            }

            state.descriptor_stack.pop();
            emitter_stack.layers.pop();
            serializable.finish(&state)?;
        }
    }
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
        state: &SerializerState,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle)>, Error>;
}

/// A map emitter.
pub trait MapEmitter {
    /// Produces the next key in the map.
    ///
    /// If this reached the end of the map `None` shall be returned.  The expectation
    /// is that this method changes an internal state in the emitter and the next
    /// call to [`next_value`](Self::next_value) returns the corresponding value.
    fn next_key(&mut self, state: &SerializerState) -> Result<Option<SerializeHandle>, Error>;

    /// Produces the next value in the map.
    ///
    /// # Panics
    ///
    /// This method shall panic if the emitter is not able to produce a value because
    /// the emitter is in the wrong state.
    fn next_value(&mut self, state: &SerializerState) -> Result<SerializeHandle, Error>;
}

/// A sequence emitter.
pub trait SeqEmitter {
    /// Produces the next item in the sequence.
    fn next(&mut self, state: &SerializerState) -> Result<Option<SerializeHandle>, Error>;
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
pub trait Serialize {
    /// Serializes this serializable.
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error>;

    /// Invoked after the serialization finished.
    ///
    /// This is primarily useful to undo some state change in the serializer
    /// state at the end of the processing.
    fn finish(&self, _state: &SerializerState) -> Result<(), Error> {
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

    /// Returns the descriptor of this serializable if it exists.
    fn descriptor(&self) -> &dyn Descriptor {
        &NullDescriptor
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

    for_each_event(&m, |event, _, _| {
        v.push(format!("{:?}", event));
        Ok(())
    })
    .unwrap();

    assert_eq!(
        &v[..],
        [
            "MapStart",
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
