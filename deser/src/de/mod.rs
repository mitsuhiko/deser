//! Generic data structure deserialization framework.
//!
//! Deserialization is based on the [`Sink`] and [`Deserializable`] traits.
//! When deserialization is started the target deserializable object
//! is attached to a destination slot.  As deserialization is happening
//! the value is placed there.
//!
//! # Slots and Sinks
//!
//! The system of slots and sinks can be relatively hard to understand.
//! The basic idea is that when a type should be deserialized a "slot"
//! is passed to it.  The slot is just an `Option` where the final value
//! is placed.  The system that places these values there is called a
//! [`Sink`].
//!
//! While sinks can be implemented on arbitrary types it's more typical to
//! implement them on a [`SlotWrapper`].  Due to Rust's orphan rules you need
//! to create your own [`SlotWrapper`] type in your crate by using the
//! [`make_slot_wrapper`](crate::make_slot_wrapper`) macro ([more
//! information](https://doc.rust-lang.org/error-index.html#E0117)).
//!
//! A [`SlotWrapper`] acts as a newtype around an `Option<T>` and derefs into an
//! `Option<T>`.  To use it implement your desired [`Sink`] for it.  This has the
//! advantage that it does not need to be allocated.  By calling
//! [`SlotWrapper::make_handle`] on a slot, one can directly retrieve a
//! [`SinkHandle`] without the need to box up the slot.
//!
//! # Driver
//!
//! Because the serialization interface of `deser` is tricky to use with lifetimes
//! without using a lot of stack space, a safe abstraction is provided with the
//! [`Driver`] type which allow you to drive the deserialization process without
//! using stack space.  You feed it events and internally the driver ensures
//! that the deserlization system is driven in the right way.
//!
//! ```rust
//! use std::collections::BTreeMap;
//! use deser::de::Driver;
//! use deser::Event;
//!
//! let mut out = None::<BTreeMap<u32, String>>;
//! {
//!     let mut driver = Driver::new(&mut out);
//!     driver.emit(&Event::MapStart).unwrap();
//!     driver.emit(&Event::I64(1)).unwrap();
//!     driver.emit(&Event::Str("Hello".into())).unwrap();
//!     driver.emit(&Event::I64(2)).unwrap();
//!     driver.emit(&Event::Str("World".into())).unwrap();
//!     driver.emit(&Event::MapEnd).unwrap();
//! }
//!
//! let map = out.unwrap();
//! assert_eq!(map[&1], "Hello");
//! assert_eq!(map[&2], "World");
//! ```
//!
//! # Deserializing primitives
//!
//! To deserialize a primitive you implement a sink for your slot wrapper and
//! implement the necessary callback.  For instance to accept a `bool` implement
//! the corresponding [`bool`](Sink::bool) method.  The resulting value then
//! must be placed in the slot:
//!
//! ```rust
//! use deser::de::{Sink, Deserializable, DeserializerState, SinkHandle};
//! use deser::{make_slot_wrapper, Error};
//!
//! make_slot_wrapper!(SlotWrapper);
//!
//! struct MyBool(bool);
//!
//! impl Sink for SlotWrapper<MyBool> {
//!     fn bool(
//!         &mut self,
//!         value: bool,
//!         _state: &DeserializerState,
//!     ) -> Result<(), Error> {
//!         // note the extra star here to reach through the deref
//!         // of the slot wrapper.
//!         **self = Some(MyBool(value));
//!         Ok(())
//!     }
//! }
//!
//! impl Deserializable for MyBool {
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
//! If you want to deserialize a struct you need to implement a
//! [`MapSink`] and return it from the main [`Sink`]:
//!
//! ```rust
//! use deser::de::{DeserializerState, Deserializable, Sink, SinkHandle, MapSink, ignore};
//! use deser::{make_slot_wrapper, Error, ErrorKind};
//!
//! make_slot_wrapper!(SlotWrapper);
//!
//! struct Flag {
//!     enabled: bool,
//!     name: String,
//! }
//!
//! impl Deserializable for Flag {
//!     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
//!         SlotWrapper::make_handle(out)
//!     }
//! }
//!
//! impl Sink for SlotWrapper<Flag> {
//!     fn map(
//!         &mut self,
//!         _state: &DeserializerState,
//!     ) -> Result<Box<dyn MapSink + '_>, Error> {
//!         // return a new map sink for our struct
//!         Ok(Box::new(FlagMapSink {
//!             // note that we can directly connect our slot wrapper
//!             // to the output slot on the sink as it deref's into an Option
//!             out: self,
//!             key: None,
//!             enabled_field: None,
//!             name_field: None,
//!         }))
//!     }
//! }
//!     
//! struct FlagMapSink<'a> {
//!     out: &'a mut Option<Flag>,
//!     key: Option<String>,
//!     enabled_field: Option<bool>,
//!     name_field: Option<String>,
//! }
//!     
//! impl<'a> MapSink for FlagMapSink<'a> {
//!     fn key(&mut self) -> Result<SinkHandle, Error> {
//!         // directly attach to the key field which can hold any
//!         // string value.  This means that any string is accepted
//!         // as key.
//!         Ok(Deserializable::deserialize_into(&mut self.key))
//!     }
//!     
//!     fn value(&mut self) -> Result<SinkHandle, Error> {
//!         // whenever we are looking for a value slot, look at the last key
//!         // to decide which value slot to connect.
//!         match self.key.take().as_deref() {
//!             Some("enabled") => Ok(Deserializable::deserialize_into(&mut self.enabled_field)),
//!             Some("name") => Ok(Deserializable::deserialize_into(&mut self.name_field)),
//!             // if we don't know the key, return a ignore sink to drop the value.
//!             _ => Ok(SinkHandle::to(ignore())),
//!         }
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
use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::fmt;
use std::mem::{replace, ManuallyDrop};
use std::ops::{Deref, DerefMut};

use crate::descriptors::{Descriptor, NullDescriptor};
use crate::error::{Error, ErrorKind};
use crate::event::Event;
use crate::extensions::Extensions;

mod ignore;
mod impls;

pub use self::ignore::ignore;

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
/// [`SerializableHandle`](crate::ser::SerializableHandle).
pub enum SinkHandle<'a> {
    /// A borrowed reference to a [`Sink`].
    Borrowed(&'a mut dyn Sink),
    /// A boxed up [`Sink`] within the handle.
    Owned(Box<dyn Sink + 'a>),
}

impl<'a> Deref for SinkHandle<'a> {
    type Target = dyn Sink + 'a;

    fn deref(&self) -> &Self::Target {
        match self {
            SinkHandle::Borrowed(val) => &**val,
            SinkHandle::Owned(val) => &**val,
        }
    }
}

impl<'a> DerefMut for SinkHandle<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            SinkHandle::Borrowed(val) => &mut **val,
            SinkHandle::Owned(val) => &mut **val,
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
}

/// A trait for deserializable types.
///
/// A type is deserializable if it can deserialize into a [`Sink`].  The
/// actual deserialization logic itself is implemented by the returned
/// [`Sink`].
pub trait Deserializable: Sized {
    /// Creates a sink that deserializes the value into the given slot.
    ///
    /// There are two typical implementations for this method: the common one is
    /// to return a [`SlotWrapper`].  Custom types will most likely just return
    /// that.  An alternative method is to "wrap" the deserializable in a custom
    /// sink.
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle;

    /// Internal method to specialize byte arrays.
    #[doc(hidden)]
    fn __private_byte_slice(_bytes: &[u8]) -> Option<&[Self]>
    where
        Self: Sized,
    {
        None
    }
}

/// Trait to place values in a slot.
///
/// During deserialization the deserializer will invoke the right method to
/// produce a value to it.  The sink then places the value in the slot behind
/// the sink.
pub trait Sink {
    fn null(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected null, expected {}", self.expecting()),
        ))
    }

    fn bool(&mut self, value: bool, _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected bool ({}), expected {}", value, self.expecting()),
        ))
    }

    fn str(&mut self, _value: &str, _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected string, expected {}", self.expecting()),
        ))
    }

    fn bytes(&mut self, _value: &[u8], _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected bytes, expected {}", self.expecting()),
        ))
    }

    fn u64(&mut self, value: u64, _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!(
                "unexpected unsigned integer ({}), expected {}",
                value,
                self.expecting()
            ),
        ))
    }

    fn i64(&mut self, value: i64, _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!(
                "unexpected signed integer ({}), expected {}",
                value,
                self.expecting()
            ),
        ))
    }

    fn f64(&mut self, value: f64, _state: &DeserializerState) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!(
                "unexpected float ({}), expected {}",
                value,
                self.expecting()
            ),
        ))
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<Box<dyn MapSink + '_>, Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected map, expected {}", self.expecting()),
        ))
    }

    fn seq(&mut self, _state: &DeserializerState) -> Result<Box<dyn SeqSink + '_>, Error> {
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("unexpected sequence, expected {}", self.expecting()),
        ))
    }

    fn expecting(&self) -> Cow<'_, str> {
        "compatible type".into()
    }
}

/// A trait to produce sinks for key and value pairs of a map or structs.
pub trait MapSink {
    /// Returns the [`Descriptor`] for this map.
    fn descriptor(&self) -> &dyn Descriptor {
        &NullDescriptor
    }

    /// Produces the [`Sink`] for the next key.
    fn key(&mut self) -> Result<SinkHandle, Error>;

    /// Produces the [`Sink`] for the next value.
    ///
    /// This can inspect the last key to make a decision about which
    /// sink to produce.
    fn value(&mut self) -> Result<SinkHandle, Error>;

    /// Called when all pairs were produced.
    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error>;
}

/// A trait to produce sinks for items in a sequence.
pub trait SeqSink {
    /// Returns the [`Descriptor`] for this seq.
    fn descriptor(&self) -> &dyn Descriptor {
        &NullDescriptor
    }

    /// Produces the [`Sink`] for the next item.
    fn item(&mut self) -> Result<SinkHandle, Error>;

    /// Called when all items were produced.
    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error>;
}

/// Gives access to the deserializer state.
pub struct DeserializerState<'a> {
    extensions: Extensions,
    stack: ManuallyDrop<Vec<(SinkHandleWrapper, Layer<'a>)>>,
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
        self.stack.len()
    }

    /// Returns the topmost descriptor.
    ///
    /// This descriptor always points to a container as the descriptor.
    pub fn top_descriptor(&self) -> Option<&dyn Descriptor> {
        self.stack.last().map(|x| match &x.1 {
            Layer::Map(map, _) => map.descriptor(),
            Layer::Seq(seq) => seq.descriptor(),
        })
    }
}

/// A driver allows emitting deserialization events into a [`Deserializable`].
///
/// This is a convenient way to safely drive a [`Sink`] of a [`Deserializable`]
/// without using the runtime stack.  As rust lifetimes make what this type does
/// internally impossible with safe code, this is a safe abstractiont that
/// hides the unsafety internally.
pub struct Driver<'a> {
    state: ManuallyDrop<DeserializerState<'a>>,
    current_sink: SinkHandleWrapper,
}

struct SinkHandleWrapper {
    sink: SinkHandle<'static>,
    used: bool,
}

impl SinkHandleWrapper {
    unsafe fn from<'a>(sink: SinkHandle<'a>) -> SinkHandleWrapper {
        SinkHandleWrapper {
            sink: extend_lifetime!(sink, SinkHandle<'_>),
            used: false,
        }
    }
}

enum Layer<'a> {
    Map(Box<dyn MapSink + 'a>, bool),
    Seq(Box<dyn SeqSink + 'a>),
}

impl<'a> Driver<'a> {
    /// Creates a new deserializer driver.
    pub fn new<T: Deserializable>(out: &'a mut Option<T>) -> Driver<'a> {
        Driver::from_sink(T::deserialize_into(out))
    }

    /// Creates a new deserializer driver from a sink.
    pub fn from_sink(sink: SinkHandle) -> Driver<'a> {
        Driver {
            state: ManuallyDrop::new(DeserializerState {
                extensions: Extensions::default(),
                stack: ManuallyDrop::new(Vec::new()),
            }),
            current_sink: unsafe { SinkHandleWrapper::from(sink) },
        }
    }

    /// Returns a borrowed reference to the current deserializer state.
    pub fn state(&self) -> &DeserializerState {
        &self.state
    }

    /// Emits an event into the driver.
    ///
    /// # Panics
    ///
    /// The driver keeps an internal state and emitting events when they are
    /// not expected will cause the driver to panic.  For instance trying to
    /// feed two events into a sink that was already used is guarded against.
    /// Likewise sending an unexpected `MapEnd` event or similar into the
    /// driver will cause a panic.
    pub fn emit(&mut self, event: &Event) -> Result<(), Error> {
        debug_assert!(
            !self.current_sink.used,
            "cannot emit event because sink has already been used"
        );

        let current_sink = &mut self.current_sink.sink;
        match event {
            Event::Null => current_sink.null(&self.state)?,
            Event::Bool(v) => current_sink.bool(*v, &self.state)?,
            Event::Str(v) => current_sink.str(v, &self.state)?,
            Event::Bytes(v) => current_sink.bytes(v, &self.state)?,
            Event::U64(v) => current_sink.u64(*v, &self.state)?,
            Event::I64(v) => current_sink.i64(*v, &self.state)?,
            Event::F64(v) => current_sink.f64(*v, &self.state)?,
            Event::MapStart => {
                let mut map_sink = current_sink.map(&self.state)?;
                let key_sink = unsafe { SinkHandleWrapper::from(map_sink.key()?) };
                let layer = unsafe { extend_lifetime!(Layer::Map(map_sink, true), Layer<'_>) };
                let old_sink = replace(&mut self.current_sink, key_sink);
                self.state.stack.push((old_sink, layer));
                return Ok(());
            }
            Event::MapEnd => match self.state.stack.pop() {
                Some((next_sink, Layer::Map(mut map_sink, _))) => {
                    map_sink.finish(&self.state)?;
                    self.current_sink = next_sink;
                }
                _ => panic!("not inside a MapSink"),
            },
            Event::SeqStart => {
                let mut seq_sink = current_sink.seq(&self.state)?;
                let item_sink = unsafe { SinkHandleWrapper::from(seq_sink.item()?) };
                let layer = unsafe { extend_lifetime!(Layer::Seq(seq_sink), Layer<'_>) };
                let old_sink = replace(&mut self.current_sink, item_sink);
                self.state.stack.push((old_sink, layer));
                return Ok(());
            }
            Event::SeqEnd => match self.state.stack.pop() {
                Some((next_sink, Layer::Seq(mut seq_sink))) => {
                    seq_sink.finish(&self.state)?;
                    self.current_sink = next_sink;
                }
                _ => panic!("not inside a SeqSink"),
            },
        }

        self.current_sink.used = true;

        match self.state.stack.last_mut() {
            Some((_, Layer::Map(ref mut map_sink, ref mut is_key))) => {
                let next_sink = if *is_key {
                    map_sink.value()?
                } else {
                    map_sink.key()?
                };
                *is_key = !*is_key;
                self.current_sink = unsafe { SinkHandleWrapper::from(next_sink) };
            }
            Some((_, Layer::Seq(ref mut seq_sink))) => {
                self.current_sink = unsafe { SinkHandleWrapper::from(seq_sink.item()?) };
            }
            _ => {}
        }

        Ok(())
    }
}

impl<'a> Drop for Driver<'a> {
    fn drop(&mut self) {
        // it's important that we drop the values in inverse order.
        while let Some(_last) = self.state.stack.pop() {
            // drop in inverse order
        }
        unsafe {
            ManuallyDrop::drop(&mut self.state.stack);
            ManuallyDrop::drop(&mut self.state);
        }
    }
}

#[test]
fn test_driver() {
    let mut out: Option<std::collections::BTreeMap<u32, String>> = None;
    {
        let mut driver = Driver::new(&mut out);
        driver.emit(&Event::MapStart).unwrap();
        driver.emit(&Event::I64(1)).unwrap();
        driver.emit(&Event::Str("Hello".into())).unwrap();
        driver.emit(&Event::I64(2)).unwrap();
        driver.emit(&Event::Str("World".into())).unwrap();
        driver.emit(&Event::MapEnd).unwrap();
    }

    let map = out.unwrap();
    assert_eq!(map[&1], "Hello");
    assert_eq!(map[&2], "World");
}
