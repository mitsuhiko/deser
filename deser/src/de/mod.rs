//! Generic data structure deserialization framework.
//!
//! Deserialization is based on the [`Sink`] and [`Deserialize`] traits.
//! When deserialization is started the target deserializable object
//! is attached to a destination slot.  As deserialization is happening
//! the value is placed there.
//!
//! # Slots and Sinks
//!
//! Deserialization is based on "slots" and "sinks".  The basic idea is that when a
//! type should be deserialized a slot in the form of an `Option<T>` is passed
//! to it where the deserialized value will be placed.  The abstraction that
//! places these values there is called a [`Sink`] which is returned within a
//! [`SinkHandle`] from the deserializer.
//!
//! If you can get away with stateless deserialization you can avoid an
//! allocation by using a newtype wrapper around `Option<T>`.  You can
//! get such a wrapper by using the
//! [`make_slot_wrapper`](crate::make_slot_wrapper`) macro ([more
//! information](https://doc.rust-lang.org/error-index.html#E0117))
//! which will create a type [`SlotWrapper`].  Due to Rust's orphan rules
//! you need to create your own type in your crate and you can't use the
//! one from this module directly.  ([more
//! information](https://doc.rust-lang.org/error-index.html#E0117)).
//!
//! This [`SlotWrapper`] derefs into an `Option<T>` which makes it quite
//! convenient to use.  By calling [`SlotWrapper::make_handle`] with a slot, one
//! can directly retrieve a [`SinkHandle`].
//!
//! # Deserializing primitives
//!
//! To deserialize a primitive you implement a sink for your slot wrapper and
//! implement the necessary callback.  You can do this as you do not need any
//! state on the sink so we can use a [`SlotWrapper`].  In this example we
//! want to accept a `bool` so we just need to implement the
//! [`atom`](Sink::atom) method as bools are represented as [`Atom`]s.  The
//! resulting value then must be placed in the slot:
//!
//! ```rust
//! use deser::de::{Sink, Deserialize, DeserializerState, SinkHandle};
//! use deser::{make_slot_wrapper, Error, Atom};
//!
//! make_slot_wrapper!(SlotWrapper);
//!
//! struct MyBool(bool);
//!
//! impl Sink for SlotWrapper<MyBool> {
//!     fn atom(
//!         &mut self,
//!         atom: Atom,
//!         state: &DeserializerState,
//!     ) -> Result<(), Error> {
//!         match atom {
//!             Atom::Bool(value) => {
//!                 // note the extra star here to reach through the deref
//!                 // of the slot wrapper.
//!                 **self = Some(MyBool(value));
//!                 Ok(())
//!             }
//!             // for any other value we dispatch to the default handling
//!             // which creates an unexpected type error but might have
//!             // more elaborate default behavior in the future.
//!             other => self.unexpected_atom(other, state)
//!         }
//!     }
//! }
//!
//! impl Deserialize for MyBool {
//!     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
//!         // Since we're using the SlotWrapper abstraction we can directly
//!         // make a handle here by using the `make_handle` utility.
//!         SlotWrapper::make_handle(out)
//!     }
//! }
//! ```
//!
//! # Struct deserialization
//!
//! If you want to deserialize a struct you need to implement the map methods.
//! As you need to keep track of state you will need to return a boxed sink
//! and you can't use the slot wrapper.
//!
//! ```rust
//! use deser::de::{DeserializerState, Deserialize, Sink, SinkHandle};
//! use deser::{Error, ErrorKind};
//!
//! struct Flag {
//!     enabled: bool,
//!     name: String,
//! }
//!
//! impl Deserialize for Flag {
//!     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
//!         SinkHandle::boxed(FlagSink {
//!             out,
//!             key: None,
//!             enabled_field: None,
//!             name_field: None,
//!         })
//!     }
//! }
//!
//! struct FlagSink<'a> {
//!     out: &'a mut Option<Flag>,
//!     key: Option<String>,
//!     enabled_field: Option<bool>,
//!     name_field: Option<String>,
//! }
//!     
//! impl<'a> Sink for FlagSink<'a> {
//!     fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
//!         // the default implementation returns an error, so we need to
//!         // override it to remove this error.
//!         Ok(())
//!     }
//!
//!     fn next_key(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
//!         // directly attach to the key field which can hold any
//!         // string value.  This means that any string is accepted
//!         // as key.
//!         Ok(Deserialize::deserialize_into(&mut self.key))
//!     }
//!     
//!     fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
//!         let key = self.key.take().unwrap();
//!         // since we implement a sink for a struct, move the actual logic for
//!         // matching into `value_for_key` so that our deserializer can support
//!         // struct flattening.  If we don't know the key, just return a null
//!         // handle to ignore it.
//!         Ok(self.value_for_key(&key, state)?.unwrap_or_else(SinkHandle::null))
//!     }
//!
//!     fn value_for_key(&mut self, key: &str, _state: &DeserializerState)
//!         -> Result<Option<SinkHandle>, Error>
//!     {
//!         Ok(Some(match key {
//!             "enabled" => Deserialize::deserialize_into(&mut self.enabled_field),
//!             "name" => Deserialize::deserialize_into(&mut self.name_field),
//!             _ => return Ok(None)
//!         }))
//!     }
//!     
//!     fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
//!         // when we're done, write the final value into the output slot.
//!         *self.out = Some(Flag {
//!             enabled: self.enabled_field.take().ok_or_else(|| {
//!                 Error::new(ErrorKind::MissingField, "field 'enabled' missing")
//!             })?,
//!             name: self.name_field.take().ok_or_else(|| {
//!                 Error::new(ErrorKind::MissingField, "field 'name' missing")
//!             })?,
//!         });
//!         Ok(())
//!     }
//! }
//! ```
//!
//! # Driver
//!
//! Because the serialization interface of `deser` is tricky to use because of
//! lifetimes, a safe abstraction is provided with the [`Driver`] type which
//! allow you to drive the deserialization process without using stack space.
//! You feed it events and internally the driver ensures that the deserlization
//! system is driven in the right way.
//!
//! ```rust
//! use std::collections::BTreeMap;
//! use deser::de::DeserializeDriver;
//! use deser::Event;
//!
//! let mut out = None::<BTreeMap<u32, String>>;
//! {
//!     let mut driver = DeserializeDriver::new(&mut out);
//!     // emit takes values that implement Into<Event>
//!     driver.emit(Event::MapStart).unwrap();
//!     driver.emit(1i64).unwrap();
//!     driver.emit("Hello").unwrap();
//!     driver.emit(2i64).unwrap();
//!     driver.emit("World").unwrap();
//!     driver.emit(Event::MapEnd).unwrap();
//! }
//!
//! let map = out.unwrap();
//! assert_eq!(map[&1], "Hello");
//! assert_eq!(map[&2], "World");
//! ```
use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::fmt;
use std::ops::{Deref, DerefMut};

use crate::descriptors::{Descriptor, NullDescriptor};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;

mod driver;
mod ignore;
mod impls;
mod owned;

pub use self::driver::DeserializeDriver;
pub use self::owned::OwnedSink;
use crate::extensions::Extensions;

__make_slot_wrapper!((pub), SlotWrapper);

/// A handle to a [`Sink`].
///
/// During deserialization the sinks often need to return other sinks
/// to recurse into structures.  This poses a challenge if the target
/// sink cannot be directly borrowed.  This is where [`SinkHandle`]
/// comes in.  In cases where the [`Sink`] cannot be borrowed it can
/// be boxed up inside the handle.
///
/// The equivalent for serialization is the
/// [`SerializeHandle`](crate::ser::SerializeHandle).
pub enum SinkHandle<'a> {
    /// A borrowed reference to a [`Sink`].
    Borrowed(&'a mut dyn Sink),
    /// A boxed up [`Sink`] within the handle.
    Owned(Box<dyn Sink + 'a>),
    /// A special handle that drops all values.
    ///
    /// To create this handle call [`SinkHandle::null`].
    Null(ignore::Ignore),
}

impl<'a> Deref for SinkHandle<'a> {
    type Target = dyn Sink + 'a;

    fn deref(&self) -> &Self::Target {
        match self {
            SinkHandle::Borrowed(val) => &**val,
            SinkHandle::Owned(val) => &**val,
            SinkHandle::Null(ref val) => val,
        }
    }
}

impl<'a> DerefMut for SinkHandle<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            SinkHandle::Borrowed(val) => &mut **val,
            SinkHandle::Owned(val) => &mut **val,
            SinkHandle::Null(ref mut val) => val,
        }
    }
}

impl<'a> SinkHandle<'a> {
    /// Create a borrowed handle to a [`Sink`].
    pub fn to(val: &'a mut dyn Sink) -> SinkHandle<'a> {
        SinkHandle::Borrowed(val)
    }

    /// Create an owned handle to a heap allocated [`Sink`].
    pub fn boxed<S: Sink + 'a>(val: S) -> SinkHandle<'a> {
        SinkHandle::Owned(Box::new(val))
    }

    /// Creates a sink handle that drops all values.
    ///
    /// This can be used in places where a sink is required but no value
    /// wants to be collected.  For instance it can be tricky to provide a
    /// mutable reference to a sink from a function that doesn't have a way
    /// to put a slot somewhere.
    pub fn null() -> SinkHandle<'a> {
        SinkHandle::Null(ignore::Ignore)
    }
}

/// Gives access to the deserializer state.
pub struct DeserializerState<'a> {
    extensions: Extensions,
    descriptor_stack: Vec<&'a dyn Descriptor>,
}

impl<'a> DeserializerState<'a> {
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
    /// This descriptor always points to a container as the descriptor.
    pub fn top_descriptor(&self) -> Option<&dyn Descriptor> {
        self.descriptor_stack.last().copied()
    }
}

/// A trait for deserializable types.
///
/// A type is deserializable if it can deserialize into a [`Sink`].  The
/// actual deserialization logic itself is implemented by the returned
/// [`Sink`].
pub trait Deserialize: Sized {
    /// Creates a sink that deserializes the value into the given slot.
    ///
    /// There are two typical implementations for this method: the common one is
    /// to return a [`SlotWrapper`].  Custom types will most likely just return
    /// that.  An alternative method is to "wrap" the deserializable in a custom
    /// sink.
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle;

    /// Returns `true` if this deserialize is `u8`.
    ///
    /// # Safety
    ///
    /// This method is unsafe as it must only ever return `Some` here if `Self` is `u8`.
    /// Returning this true for any other type will cause undefined behavior due to how
    /// the arrays are implemented.
    #[doc(hidden)]
    unsafe fn __private_is_bytes() -> bool {
        false
    }
}

/// Trait to place values in a slot.
///
/// A sink acts as an abstraction to receive a value during deserialization from
/// the deserializer.  Sinks in deser are one-shot receivers.  A deserializer must
/// invoke one receiver method for a total of zero or one times.
///
/// The sink then places the received value in the slot connected to the sink.
pub trait Sink {
    /// Receives an [`Atom`].
    ///
    /// Any unknown atom variant should be dispatched to [`unexpected_atom`](Self::unexpected_atom).
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        self.unexpected_atom(atom, state)
    }

    /// Implements a default fallback handling for atoms.
    fn unexpected_atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        let _ = state;
        Err(atom.unexpected_error(&self.expecting()))
    }

    /// Begins the deserialization of a map.
    ///
    /// While the deserialization of a map is ongoing the methods
    /// [`next_key`](Self::next_key) and [`next_value`](Self::next_value) are
    /// called alternatingly.  The map is ended by [`finish`](Self::finish).
    ///
    /// The default implementation returns an error.
    fn map(&mut self, state: &DeserializerState) -> Result<(), Error> {
        let _ = state;
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected map, expected {}", self.expecting()),
        ))
    }

    /// Begins the receiving process for sequences.
    ///
    /// While the deserialization of a sequence is ongoing the method
    /// [`next_value`](Self::next_value) is called for every new item.
    /// The sequence is ended by [`finish`](Self::finish).
    ///
    /// The default implementation returns an error.
    fn seq(&mut self, state: &DeserializerState) -> Result<(), Error> {
        let _ = state;
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected sequence, expected {}", self.expecting()),
        ))
    }

    /// Returns a sink for the next key in a map.
    fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
        let _ = state;
        Ok(SinkHandle::null())
    }

    /// Returns a sink for the next value in a map or sequence.
    fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
        let _ = state;
        Ok(SinkHandle::null())
    }

    /// Returns a value sink for a specific struct field.
    ///
    /// This is a special method that is supposed to be implemented by structs
    /// if they want to support flattening.  A struct that gets flattened into
    /// another struct will have this method called to figure out if a key is
    /// used by it.  The default implementation always returns `None`.
    fn value_for_key(
        &mut self,
        key: &str,
        state: &DeserializerState,
    ) -> Result<Option<SinkHandle>, Error> {
        let _ = key;
        let _ = state;
        Ok(None)
    }

    /// Called after [`atom`](Self::atom), [`map`](Self::map) or [`seq](Self::seq).
    ///
    /// The default implementation does nothing.
    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        let _ = state;
        Ok(())
    }

    /// Returns a descriptor for this type.
    fn descriptor(&self) -> &dyn Descriptor {
        &NullDescriptor
    }

    /// Utility method to return an expectation message that is used in error messages.
    ///
    /// The default implementation returns the type name of the descriptor if available.
    fn expecting(&self) -> Cow<'_, str> {
        self.descriptor().name().unwrap_or("compatible type").into()
    }
}
