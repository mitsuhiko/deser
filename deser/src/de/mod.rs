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
//! # Streaming Deserialization
//!
//! Because the serialization interface of Deser is tricky due to use of
//! lifetimes, a safe abstraction is provided with the [`DeserializeDriver`].
//! This type which allow you to drive the deserialization process without using
//! stack space.  You feed it events and internally the driver ensures that the
//! deserlization system is driven in the right way.
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
//!     driver.emit(Event::map_start()).unwrap();
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
//!
//! The deserializers of data formats implement the [`Deserializer`] trait
//! which feeds the events of a value into a driver.
//!
//! # Layers and Wrapped Sinks
//!
//! There are two ways to change how a deserialization is processed without
//! support by the format or the types:
//!
//! * [`Layer`]s sit between the format and the driver and see the events.
//!   They are useful for everything that can be derived from the events,
//!   for instance to track the current path, to enforce limits (see
//!   [`Limits`]) or to rewrite values.
//! * Wrapped sinks (see [`DeserializeDriver::wrap_sink`]) sit between the
//!   driver and the sinks of the values.  They are useful for changes that
//!   depend on the target types.
//!
//! Both are set up with [`Deserializer::deserialize_with`].
//!
//! # Deserializing Primitives
//!
//! To deserialize a primitive you implement a sink for your slot wrapper and
//! implement the necessary callback.  You can do this as you do not need any
//! state on the sink so we can use a [`SlotWrapper`].  In this example we
//! want to accept a `bool` so we just need to implement the
//! [`atom`](Sink::atom) method as bools are represented as [`Atom`]s.  The
//! resulting value then must be placed in the slot:
//!
//! ```rust
//! use deser::de::{Sink, Deserialize, SinkHandle};
//! use deser::State;
//! use deser::{make_slot_wrapper, Error, Atom};
//!
//! make_slot_wrapper!(SlotWrapper);
//!
//! struct MyBool(bool);
//!
//! impl<'de> Sink<'de> for SlotWrapper<MyBool> {
//!     fn atom(
//!         &mut self,
//!         atom: Atom,
//!         state: &mut State,
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
//! impl<'de> Deserialize<'de> for MyBool {
//!     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
//!         // Since we're using the SlotWrapper abstraction we can directly
//!         // make a handle here by using the `make_handle` utility.
//!         SlotWrapper::make_handle(out)
//!     }
//! }
//! ```
//!
//! # Struct Deserialization
//!
//! If you want to deserialize a struct you need to implement the map methods.
//! As you need to keep track of state you will need to return a boxed sink
//! and you can't use the slot wrapper.
//!
//! ```rust
//! use deser::de::{Deserialize, Sink, SinkHandle};
//! use deser::State;
//! use deser::{Error, ErrorKind};
//!
//! struct Flag {
//!     enabled: bool,
//!     name: String,
//! }
//!
//! impl<'de> Deserialize<'de> for Flag {
//!     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
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
//! impl<'a, 'de> Sink<'de> for FlagSink<'a> {
//!     fn map(&mut self, _state: &mut State) -> Result<(), Error> {
//!         // the default implementation returns an error, so we need to
//!         // override it to remove this error.
//!         Ok(())
//!     }
//!
//!     fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
//!         // directly attach to the key field which can hold any
//!         // string value.  This means that any string is accepted
//!         // as key.
//!         Ok(Deserialize::deserialize_into(&mut self.key))
//!     }
//!     
//!     fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
//!         let key = self.key.take().unwrap();
//!         // since we implement a sink for a struct, move the actual logic for
//!         // matching into `value_for_key` so that our deserializer can support
//!         // struct flattening.  If we don't know the key, just return a null
//!         // handle to ignore it.
//!         Ok(self.value_for_key(&key, state)?.unwrap_or_else(SinkHandle::null))
//!     }
//!
//!     fn value_for_key(&mut self, key: &str, _state: &mut State)
//!         -> Result<Option<SinkHandle<'_, 'de>>, Error>
//!     {
//!         Ok(Some(match key {
//!             "enabled" => Deserialize::deserialize_into(&mut self.enabled_field),
//!             "name" => Deserialize::deserialize_into(&mut self.name_field),
//!             _ => return Ok(None)
//!         }))
//!     }
//!     
//!     fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
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
//! # Owned Sinks and Slots
//!
//! From the above model you can see that deserialization requires a mutable reference
//! to an `Option`.  In certain situations it can become necessary to "make up a slot
//! on the spot" to temporarily deserialize into.  For more information see
//! [`OwnedSink`].
use std::borrow::Cow;

use crate::error::{Error, ErrorKind};
use crate::event::Atom;

pub(crate) mod atoms;
mod deserializer;
mod driver;
#[cfg(feature = "derive")]
pub(crate) mod enums;
mod ignore;
pub(crate) mod impls;
mod layer;
pub(crate) mod lexical;
pub(crate) mod mapped;
mod owned;
mod recording;
mod sinkbox;

pub(crate) use self::atoms::{atom_into_handle, borrowed_atom_into_handle};
pub use self::deserializer::Deserializer;
pub use self::driver::DeserializeDriver;
pub use self::layer::{Layer, LayerEvent, Limits, Next};
pub use self::owned::{OwnedDriver, OwnedSink};
pub use self::recording::Recording;
use self::sinkbox::SinkBox;
use crate::State;

__make_slot_wrapper!((pub), SlotWrapper);

/// A handle to a [`Sink`].
///
/// During deserialization the sinks often need to return other sinks
/// to recurse into structures.  This poses a challenge if the target
/// sink cannot be directly borrowed.  This is where [`SinkHandle`]
/// comes in.  In cases where the [`Sink`] cannot be borrowed it can
/// be boxed up inside the handle.
///
/// The handle itself implements [`Sink`] and forwards all calls to the
/// sink it holds.
///
/// The equivalent for serialization is the
/// [`SerializeHandle`](crate::ser::SerializeHandle).
pub struct SinkHandle<'a, 'de: 'a>(HandleInner<'a, 'de>);

enum HandleInner<'a, 'de> {
    Borrowed(&'a mut dyn Sink<'de>),
    Owned(SinkBox<'a, 'de>),
    Null(ignore::Ignore),
    // The optional variants are used to implement `Option<T>` without an
    // extra allocation: a null atom is not forwarded but turns the handle
    // into a null handle so that `finish` is not forwarded either.
    OptionalBorrowed(&'a mut dyn Sink<'de>),
    OptionalOwned(SinkBox<'a, 'de>),
}

impl<'a, 'de> SinkHandle<'a, 'de> {
    /// Create a borrowed handle to a [`Sink`].
    pub fn to(val: &'a mut dyn Sink<'de>) -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Borrowed(val))
    }

    /// Create an owned handle to a heap allocated [`Sink`].
    pub fn boxed<S: Sink<'de> + 'a>(val: S) -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Owned(SinkBox::new(val)))
    }

    /// Creates a sink handle that drops all values.
    ///
    /// This can be used in places where a sink is required but no value
    /// wants to be collected.  For instance it can be tricky to provide a
    /// mutable reference to a sink from a function that doesn't have a way
    /// to put a slot somewhere.
    pub fn null() -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Null(ignore::Ignore))
    }

    /// Shortens the lifetime of the handle.
    ///
    /// Handles are invariant over their lifetime, this performs the
    /// conversion explicitly.
    pub fn shorten<'b>(self) -> SinkHandle<'b, 'de>
    where
        'a: 'b,
    {
        SinkHandle(match self.0 {
            HandleInner::Borrowed(sink) => HandleInner::Borrowed(sink),
            HandleInner::Owned(sink) => HandleInner::Owned(sink),
            HandleInner::Null(sink) => HandleInner::Null(sink),
            HandleInner::OptionalBorrowed(sink) => HandleInner::OptionalBorrowed(sink),
            HandleInner::OptionalOwned(sink) => HandleInner::OptionalOwned(sink),
        })
    }

    /// Returns `true` if this is a null handle.
    pub fn is_null(&self) -> bool {
        matches!(self.0, HandleInner::Null(_))
    }

    /// Converts the handle into one that ignores null atoms.
    ///
    /// When a null atom is received the wrapped sink is not invoked (not even
    /// [`finish`](Sink::finish)) and the handle turns into a null handle.  An
    /// atom counts as null if it is [`Atom::Null`] or an extension value which
    /// falls back to null.
    ///
    /// This is used to implement `Option<T>`: the slot is set to `Some(None)`
    /// before the handle of the inner value is created and made to ignore
    /// nulls.
    ///
    /// ```
    /// use deser::de::{Deserialize, SinkHandle};
    ///
    /// /// Deserializes like an `Option<T>`.
    /// fn deserialize_optional<'de, T: Deserialize<'de>>(
    ///     out: &mut Option<Option<T>>,
    /// ) -> SinkHandle<'_, 'de> {
    ///     T::deserialize_into(out.insert(None)).ignore_null()
    /// }
    /// ```
    pub fn ignore_null(self) -> SinkHandle<'a, 'de> {
        SinkHandle(match self.0 {
            HandleInner::Borrowed(sink) => HandleInner::OptionalBorrowed(sink),
            HandleInner::Owned(sink) => HandleInner::OptionalOwned(sink),
            other => other,
        })
    }

    /// Returns `true` if the handle ignores the atom because it's null.
    ///
    /// In that case the handle turned into a null handle.
    #[inline(always)]
    fn skip_null(&mut self, atom: &Atom) -> bool {
        if let HandleInner::OptionalBorrowed(_) | HandleInner::OptionalOwned(_) = self.0
            && is_null_atom(atom)
        {
            *self = SinkHandle::null();
            return true;
        }
        false
    }

    #[inline(always)]
    fn sink(&self) -> &(dyn Sink<'de> + 'a) {
        match self.0 {
            HandleInner::Borrowed(ref sink) | HandleInner::OptionalBorrowed(ref sink) => &**sink,
            HandleInner::Owned(ref sink) | HandleInner::OptionalOwned(ref sink) => sink.get(),
            HandleInner::Null(ref sink) => sink,
        }
    }

    #[inline(always)]
    fn sink_mut(&mut self) -> &mut (dyn Sink<'de> + 'a) {
        match self.0 {
            HandleInner::Borrowed(ref mut sink) | HandleInner::OptionalBorrowed(ref mut sink) => {
                &mut **sink
            }
            HandleInner::Owned(ref mut sink) | HandleInner::OptionalOwned(ref mut sink) => {
                sink.get_mut()
            }
            HandleInner::Null(ref mut sink) => sink,
        }
    }
}

#[cold]
fn is_null_ext(ext: &crate::ext::ExtValue) -> bool {
    matches!(ext.fallback(), Atom::Null)
}

/// Checks if an atom is a null for the purpose of optionals.
#[inline]
pub(crate) fn is_null_atom(atom: &Atom) -> bool {
    match atom {
        Atom::Null => true,
        // an extension value that falls back to null (for instance a
        // null with additional information attached) is a null too.
        Atom::Ext(ext) => is_null_ext(ext),
        _ => false,
    }
}

// The methods on the handle are inherent so that they can be used without
// having the `Sink` trait in scope.  The `Sink` implementation delegates to
// them.
impl<'a, 'de> SinkHandle<'a, 'de> {
    /// Forwards to [`Sink::atom`].
    #[inline]
    pub fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if self.skip_null(&atom) {
            return Ok(());
        }
        self.sink_mut().atom(atom, state)
    }

    /// Forwards to [`Sink::borrowed_atom`].
    #[inline]
    pub fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        if self.skip_null(&atom) {
            return Ok(());
        }
        self.sink_mut().borrowed_atom(atom, state)
    }

    /// Forwards to [`Sink::unexpected_atom`].
    pub fn unexpected_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink_mut().unexpected_atom(atom, state)
    }

    /// Forwards to [`Sink::map`].
    #[inline]
    pub fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink_mut().map(state)
    }

    /// Forwards to [`Sink::seq`].
    #[inline]
    pub fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink_mut().seq(state)
    }

    /// Forwards to [`Sink::next_key`].
    #[inline]
    pub fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink_mut().next_key(state)
    }

    /// Forwards to [`Sink::next_value`].
    #[inline]
    pub fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink_mut().next_value(state)
    }

    /// Forwards to [`Sink::key_atom`].
    #[inline]
    pub fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink_mut().key_atom(atom, state)
    }

    /// Forwards to [`Sink::value_atom`].
    #[inline]
    pub fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink_mut().value_atom(atom, state)
    }

    /// Forwards to [`Sink::borrowed_key_atom`].
    #[inline]
    pub fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink_mut().borrowed_key_atom(atom, state)
    }

    /// Forwards to [`Sink::borrowed_value_atom`].
    #[inline]
    pub fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink_mut().borrowed_value_atom(atom, state)
    }

    /// Forwards to [`Sink::value_for_key`].
    pub fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.sink_mut().value_for_key(key, state)
    }

    /// Forwards to [`Sink::finish`].
    #[inline]
    pub fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink_mut().finish(state)
    }

    /// Forwards to [`Sink::expecting`].
    pub fn expecting(&self) -> Cow<'_, str> {
        self.sink().expecting()
    }
}

impl<'a, 'de> Sink<'de> for SinkHandle<'a, 'de> {
    #[inline]
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        SinkHandle::atom(self, atom, state)
    }

    #[inline]
    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        SinkHandle::borrowed_atom(self, atom, state)
    }

    fn unexpected_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        SinkHandle::unexpected_atom(self, atom, state)
    }

    #[inline]
    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        SinkHandle::map(self, state)
    }

    #[inline]
    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        SinkHandle::seq(self, state)
    }

    #[inline]
    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        SinkHandle::next_key(self, state)
    }

    #[inline]
    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        SinkHandle::next_value(self, state)
    }

    #[inline]
    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        SinkHandle::key_atom(self, atom, state)
    }

    #[inline]
    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        SinkHandle::value_atom(self, atom, state)
    }

    #[inline]
    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        SinkHandle::borrowed_key_atom(self, atom, state)
    }

    #[inline]
    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        SinkHandle::borrowed_value_atom(self, atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        SinkHandle::value_for_key(self, key, state)
    }

    #[inline]
    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        SinkHandle::finish(self, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        SinkHandle::expecting(self)
    }
}

/// A trait for deserializable types.
///
/// A type is deserializable if it can deserialize into a [`Sink`].  The
/// actual deserialization logic itself is implemented by the returned
/// [`Sink`].
///
/// The lifetime `'de` is the lifetime of the data that is deserialized.
/// Types that borrow from it (like `&'de str`) only implement
/// `Deserialize<'de>` for that lifetime, types that do not borrow implement
/// it for all lifetimes (see [`DeserializeOwned`]):
///
/// ```
/// use deser::Deserialize;
///
/// #[derive(Deserialize)]
/// struct User<'a> {
///     name: &'a str,
///     id: u64,
/// }
/// ```
///
/// Data can only be borrowed if the data format passes it on borrowed (see
/// [`Sink::borrowed_atom`]).
///
/// # Thread Safety
///
/// Deserializable values are `Send` and so are the sinks they create.  This
/// allows an ongoing deserialization (a [`DeserializeDriver`]) to move
/// between threads, for instance when it is suspended while waiting for more
/// input.  Types that are not `Send` (such as `Rc`) cannot be deserialized.
pub trait Deserialize<'de>: Sized + Send {
    /// Creates a sink that deserializes the value into the given slot.
    ///
    /// There are two typical implementations for this method: the common one is
    /// to return a [`SlotWrapper`].  Custom types will most likely just return
    /// that.  An alternative method is to "wrap" the deserializable in a custom
    /// sink.
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de>;

    /// Provides the value of a missing struct field.
    ///
    /// When a struct is deserialized the slots of its fields start out with
    /// this value.  If a field does not appear in the data, the initial value
    /// is used.  If it is `None` (the default) the field is required.
    /// `Option<T>` returns `Some(None)` here which makes optional fields
    /// default to `None` when they are missing.
    ///
    /// This only controls missing values.  How null values are handled is up
    /// to the sink (see [`SinkHandle::ignore_null`]).  The initial value is not
    /// used for fields with `#[deser(default)]`.
    fn initial_value() -> Option<Self> {
        None
    }

    /// Deserializes an atom into the slot.
    ///
    /// This must behave exactly like invoking [`atom`](Sink::atom) and
    /// [`finish`](Sink::finish) on the sink returned by
    /// [`deserialize_into`](Self::deserialize_into), which is what the default
    /// implementation does.  Types with stateless sinks override this so that
    /// atoms can be deserialized without dynamic dispatch.
    #[doc(hidden)]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        atom_into_handle(Self::deserialize_into(out), atom, state)
    }

    /// Deserializes a borrowed atom into the slot.
    ///
    /// This is like [`__private_atom_into`](Self::__private_atom_into) but
    /// for [`borrowed_atom`](Sink::borrowed_atom).
    #[doc(hidden)]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        borrowed_atom_into_handle(Self::deserialize_into(out), atom, state)
    }

    /// Returns `true` if this deserialize is `u8`.
    ///
    /// This is used to specialize the handling of bytes for vectors and
    /// arrays of `u8`.
    #[doc(hidden)]
    fn __private_is_bytes() -> bool {
        false
    }

    /// Converts bytes into a vector of `Self`.
    ///
    /// This is only implemented for `u8` and used to specialize the
    /// deserialization of `Vec<u8>` from bytes.
    #[doc(hidden)]
    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<Self>> {
        let _ = bytes;
        None
    }

    /// Converts bytes into an array of `Self`.
    ///
    /// This is only implemented for `u8` and used to specialize the
    /// deserialization of `[u8; N]` from bytes.  Returns `None` if the
    /// type is not `u8` or the length does not match.
    #[doc(hidden)]
    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[Self; N]> {
        let _ = bytes;
        None
    }
}

/// A type that can be deserialized without borrowing.
///
/// This is implemented for all types that implement [`Deserialize`] for all
/// lifetimes, which means that they do not borrow from the data they are
/// deserialized from.  It's useful as a bound where the data does not
/// outlive the deserialization (for instance when reading from a stream).
pub trait DeserializeOwned: for<'de> Deserialize<'de> {}

impl<T> DeserializeOwned for T where T: for<'de> Deserialize<'de> {}

/// Generates the default error for unexpected maps and sequences.
fn fail_unexpected(got: &str, expecting: &str) -> Result<(), Error> {
    Err(Error::new(
        ErrorKind::Unexpected,
        format!("unexpected {}, expected {}", got, expecting),
    ))
}

/// Trait to place values in a slot.
///
/// A sink acts as an abstraction to receive a value during deserialization from
/// the deserializer.  Sinks in deser are one-shot receivers.  A deserializer must
/// invoke one receiver method for a total of zero or one times.
///
/// The sink then places the received value in the slot connected to the sink.
///
/// # Borrowed Data
///
/// Atoms are passed to [`atom`](Self::atom) with a lifetime that only lasts
/// for the call.  Formats pass atoms which borrow from the data that is
/// deserialized (which lives for `'de`) to [`borrowed_atom`](Self::borrowed_atom)
/// instead.  By default this forwards to [`atom`](Self::atom), only sinks of
/// types which want to borrow (like `&'de str`) need to implement it.
pub trait Sink<'de>: Send {
    /// Receives an [`Atom`].
    ///
    /// Any unknown atom variant should be dispatched to [`unexpected_atom`](Self::unexpected_atom).
    /// This is particularly important for [`Atom::Ext`] as the default
    /// implementation of `unexpected_atom` will retry with the fallback atom
    /// of the extension value.
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.unexpected_atom(atom, state)
    }

    /// Receives an [`Atom`] that borrows from the data being deserialized.
    ///
    /// The default implementation forwards to [`atom`](Self::atom).
    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.atom(atom, state)
    }

    /// Implements a default fallback handling for atoms.
    ///
    /// For [`Atom::Ext`] values the atom is lowered into the core data model
    /// with [`fallback`](crate::ext::ExtValue::fallback) and passed to
    /// [`atom`](Self::atom) again.  [`Atom::F32`] is widened into an
    /// [`Atom::F64`] and passed on the same way, so sinks that accept floats
    /// only need to handle `F64`.  [`Atom::Lexical`] is passed on as
    /// [`Atom::Str`], so sinks that accept strings accept lexical atoms
    /// too.  For all other atoms an error is returned.
    fn unexpected_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let atom = match atom {
            Atom::F32(value) => return self.atom(Atom::F64(f64::from(value)), state),
            Atom::Lexical(value) => return self.atom(Atom::Str(value), state),
            atom => atom,
        };
        if let Atom::Ext(ref ext) = atom {
            let fallback = ext.fallback();
            debug_assert!(
                !matches!(fallback, Atom::Ext(_)),
                "the fallback of an extension value must not be an extension value"
            );
            if !matches!(fallback, Atom::Ext(_)) {
                return self.atom(fallback, state);
            }
        }
        Err(atom.unexpected_error(&self.expecting()))
    }

    /// Begins the deserialization of a map.
    ///
    /// While the deserialization of a map is ongoing the methods
    /// [`next_key`](Self::next_key) and [`next_value`](Self::next_value) are
    /// called alternatingly.  The map is ended by [`finish`](Self::finish).
    ///
    /// The default implementation returns an error.
    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let _ = state;
        fail_unexpected("map", &self.expecting())
    }

    /// Begins the receiving process for sequences.
    ///
    /// While the deserialization of a sequence is ongoing the method
    /// [`next_value`](Self::next_value) is called for every new item.
    /// The sequence is ended by [`finish`](Self::finish).
    ///
    /// The default implementation returns an error.
    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        let _ = state;
        fail_unexpected("sequence", &self.expecting())
    }

    /// Returns a sink for the next key in a map.
    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let _ = state;
        Ok(SinkHandle::null())
    }

    /// Returns a sink for the next value in a map or sequence.
    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let _ = state;
        Ok(SinkHandle::null())
    }

    /// Receives an atom as the next key in a map.
    ///
    /// This is a shortcut for invoking [`next_key`](Self::next_key) and then
    /// [`atom`](Self::atom) and [`finish`](Self::finish) on the returned sink,
    /// which is exactly what the default implementation does.  The driver
    /// uses this for keys that are atoms which is the overwhelmingly common
    /// case.  Sinks can override this to avoid creating a sink for the key,
    /// but the behavior must be the same as with the default implementation.
    /// In particular, sinks that override [`next_key`](Self::next_key) must
    /// either not override this method or apply the same logic.
    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        atom_into_handle(self.next_key(state)?, atom, state)
    }

    /// Receives an atom as the next value in a map or sequence.
    ///
    /// This is a shortcut for invoking [`next_value`](Self::next_value) and
    /// then [`atom`](Self::atom) and [`finish`](Self::finish) on the returned
    /// sink, which is exactly what the default implementation does.  See
    /// [`key_atom`](Self::key_atom) for more information.
    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        atom_into_handle(self.next_value(state)?, atom, state)
    }

    /// Receives a borrowed atom as the next key in a map.
    ///
    /// Like [`key_atom`](Self::key_atom) but the atom is passed to
    /// [`borrowed_atom`](Self::borrowed_atom).
    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        borrowed_atom_into_handle(self.next_key(state)?, atom, state)
    }

    /// Receives a borrowed atom as the next value in a map or sequence.
    ///
    /// Like [`value_atom`](Self::value_atom) but the atom is passed to
    /// [`borrowed_atom`](Self::borrowed_atom).
    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        borrowed_atom_into_handle(self.next_value(state)?, atom, state)
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
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        let _ = key;
        let _ = state;
        Ok(None)
    }

    /// Called after [`atom`](Self::atom), [`map`](Self::map) or [`seq](Self::seq).
    ///
    /// The default implementation does nothing.
    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let _ = state;
        Ok(())
    }

    /// Utility method to return an expectation message that is used in error messages.
    ///
    /// This is typically the name of the type.  The default implementation
    /// returns `"compatible type"`.
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("compatible type")
    }
}
