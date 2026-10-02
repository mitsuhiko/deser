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
//! to it where the deserialized value will be placed.  The events of the
//! value are received by a [`Sink`] which places the value in the slot.
//! [`Deserialize::deserialize_into`] returns the sink of a slot in a
//! [`SinkHandle`].  There are two ways to implement [`Deserialize`]:
//!
//! * Values that are deserialized from a single [`Atom`] (like numbers or
//!   strings) implement [`Deserialize::deserialize_atom`].  They need no
//!   state: the slot itself (a [`Slot`], which dereferences to the
//!   `Option<T>`) is their sink and nothing needs to be allocated (see
//!   [Deserializing Primitives](#deserializing-primitives)).
//! * All other values (like structs, maps and sequences) implement
//!   [`Deserialize::deserialize_into`] and return a sink of their own which
//!   holds the state of the deserialization (see
//!   [Struct Deserialization](#struct-deserialization)).
//!
//! # Streaming Deserialization
//!
//! Driving sinks by hand is tricky due to their lifetimes, so a safe
//! abstraction is provided with the [`DeserializeDriver`].  It drives the
//! deserialization without using the call stack for nesting: you emit
//! events into it and the driver passes them on to the right sinks.
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
//! which feeds the events of a value into a driver.  Functions like
//! `from_str` are implemented with [`deserialize_value`] so that only the
//! code that depends on the type of the value exists once per type.
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
//! Primitives are deserialized from [`Atom`]s.  As no state is needed for
//! this, you only implement [`Deserialize::deserialize_atom`] which receives
//! the atom and the [`Slot`] the resulting value must be placed in.  In this
//! example we want to accept a `bool`:
//!
//! ```rust
//! use std::borrow::Cow;
//! use deser::de::{Deserialize, Slot, default_atom};
//! use deser::{Atom, Error, State};
//!
//! struct MyBool(bool);
//!
//! impl<'de> Deserialize<'de> for MyBool {
//!     fn deserialize_atom(
//!         slot: &mut Slot<Self>,
//!         atom: Atom,
//!         state: &mut State,
//!     ) -> Result<(), Error> {
//!         match atom {
//!             Atom::Bool(value) => {
//!                 slot.set(MyBool(value));
//!                 Ok(())
//!             }
//!             // any other atom goes to the default handling, which passes
//!             // some atoms on in another form (like extension values as
//!             // their fallback) and rejects the rest
//!             other => default_atom(slot, other, state),
//!         }
//!     }
//!
//!     // what is expected in error messages, this defaults to the name
//!     // of the type
//!     fn expecting() -> Cow<'static, str> {
//!         Cow::Borrowed("bool")
//!     }
//! }
//! ```
//!
//! # Struct Deserialization
//!
//! If you want to deserialize a struct you need a sink that implements the
//! map methods.  As the sink keeps track of state, you implement
//! [`deserialize_into`](Deserialize::deserialize_into) which returns a sink
//! that is owned by the handle (allocated in the arena of the
//! deserialization with [`SinkHandle::arena`]).
//!
//! ```rust
//! use std::borrow::Cow;
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
//!     fn deserialize_into<'out>(
//!         out: &'out mut Option<Self>,
//!         state: &mut State,
//!     ) -> SinkHandle<'out, 'de> {
//!         let sink = FlagSink {
//!             out,
//!             key: None,
//!             enabled: None,
//!             name: None,
//!         };
//!         SinkHandle::arena(sink, state)
//!     }
//!
//!     // what is expected in error messages (like for a string that is
//!     // passed instead of the map)
//!     fn expecting() -> Cow<'static, str> {
//!         Cow::Borrowed("flag")
//!     }
//! }
//!
//! struct FlagSink<'a> {
//!     out: &'a mut Option<Flag>,
//!     key: Option<String>,
//!     enabled: Option<bool>,
//!     name: Option<String>,
//! }
//!
//! impl<'a, 'de> Sink<'de> for FlagSink<'a> {
//!     // the sink of a value reports what the value expects
//!     fn expecting(&self) -> Cow<'_, str> {
//!         Flag::expecting()
//!     }
//!
//!     fn map(&mut self, _state: &mut State) -> Result<(), Error> {
//!         // the default implementation returns an error, so we need to
//!         // override it to remove this error.
//!         Ok(())
//!     }
//!
//!     fn next_key(
//!         &mut self,
//!         state: &mut State,
//!     ) -> Result<SinkHandle<'_, 'de>, Error> {
//!         // directly attach to the key field which can hold any
//!         // string value.  This means that any string is accepted
//!         // as key.
//!         Ok(String::deserialize_into(&mut self.key, state))
//!     }
//!
//!     fn next_value(
//!         &mut self,
//!         state: &mut State,
//!     ) -> Result<SinkHandle<'_, 'de>, Error> {
//!         let key = self.key.take().unwrap();
//!         // since we implement a sink for a struct, move the actual logic
//!         // for matching into `value_for_key` so that our deserializer can
//!         // support struct flattening.  If we don't know the key, just
//!         // return a null handle to ignore it.
//!         let handle = self.value_for_key(&key, state)?;
//!         Ok(handle.unwrap_or_else(SinkHandle::null))
//!     }
//!
//!     fn value_for_key(
//!         &mut self,
//!         key: &str,
//!         state: &mut State,
//!     ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
//!         Ok(Some(match key {
//!             "enabled" => bool::deserialize_into(&mut self.enabled, state),
//!             "name" => String::deserialize_into(&mut self.name, state),
//!             _ => return Ok(None),
//!         }))
//!     }
//!
//!     fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
//!         // when we're done, write the final value into the output slot.
//!         let enabled = self.enabled.take().ok_or_else(|| {
//!             Error::new(ErrorKind::MissingField, "field 'enabled' missing")
//!         })?;
//!         let name = self.name.take().ok_or_else(|| {
//!             Error::new(ErrorKind::MissingField, "field 'name' missing")
//!         })?;
//!         *self.out = Some(Flag { enabled, name });
//!         Ok(())
//!     }
//! }
//! ```
//!
//! # Owned Sinks and Slots
//!
//! From the above model you can see that deserialization requires a
//! mutable reference to an `Option`.  In certain situations it can become
//! necessary to "make up a slot on the spot" to temporarily deserialize
//! into.  [`OwnedSink`] bundles a sink with its slot and [`OwnedDriver`]
//! a driver with the slot of the value it deserializes.
use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::error::Error;
use crate::event::Atom;

pub(crate) mod atoms;
mod collect;
mod deserializer;
mod driver;
pub(crate) mod duplicates;
#[cfg(feature = "derive")]
pub(crate) mod enums;
#[cfg(feature = "derive")]
pub(crate) mod fields;
mod ignore;
pub(crate) mod impls;
mod layer;
pub(crate) mod lexical;
pub(crate) mod mapped;
mod owned;
pub(crate) mod recording;
mod sinkbox;
pub(crate) mod slot;
mod stream;
pub(crate) mod unknown;
pub(crate) mod update;

pub use self::atoms::default_atom;
pub(crate) use self::atoms::{atom_into_handle, borrowed_atom_into_handle};
use self::atoms::{
    default_borrowed_key_atom, default_borrowed_value_atom, default_container, default_key_atom,
    default_value_atom,
};
pub use self::collect::CollectedErrors;
pub use self::deserializer::{Deserializer, deserialize_value};
pub use self::driver::DeserializeDriver;
pub use self::duplicates::DuplicateKeys;
pub use self::layer::{Layer, LayerEvent, Limits, Next};
pub use self::lexical::{ContentKey, LexicalRules};
pub use self::owned::{OwnedDriver, OwnedSink};
pub use self::recording::{RecordBuf, Recording};
#[cfg(feature = "derive")]
use self::sinkbox::ArenaStruct;
use self::sinkbox::{ArenaSink, HeapSink, arena_sink};
pub use self::slot::Slot;
pub use self::stream::{Frame, Progress, StreamDeserializer};
pub use self::unknown::{IgnoredFields, UnknownFields};
pub use self::update::checked_update;
use crate::State;

/// Builds a sequence of atoms in the sink of the sequence it's an element
/// of.
///
/// Sequences of a fixed number of atoms (like `[f32; 2]` or `(u8, u8)`)
/// are small containers that appear in large numbers.  Instead of creating
/// a sink for every one of them, the sink of the sequence they are
/// elements of (see [`Sink::__private_seq`]) builds them in its slot for
/// the element, the driver passes their events to it.  This has to behave
/// exactly like the sink of the element.
///
/// Internal fast path, not public API (see `lib.rs`).
#[doc(hidden)]
pub struct InlineSeq<T> {
    /// Starts the sequence in the slot.
    pub start: fn(&mut Option<T>),
    /// Deserializes the atom at the index.
    pub atom: fn(&mut Option<T>, usize, Atom, &mut State) -> Result<(), Error>,
    /// Ends the sequence with the given length.
    pub end: fn(&mut Option<T>, usize) -> Result<(), Error>,
    /// Returns the error for a map (`true`) or sequence at the index.
    pub container: fn(usize, bool, &mut State) -> Error,
}

/// An event of a sequence that is built inline (see [`InlineSeq`]).
///
/// Internal fast path, not public API (see `lib.rs`).
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum InlineEvent {
    /// The sequence starts.
    Start,
    /// The sequence ends with the given length.
    End(usize),
    /// A map (`true`) or sequence starts at the index.
    Container(usize, bool),
}

/// Panics as the sink does not build sequences inline.
#[cold]
#[inline(never)]
fn no_inline_seq() -> ! {
    panic!("the sink does not build sequences inline")
}

/// A handle to a [`Sink`].
///
/// During deserialization the sinks often need to return other sinks
/// to recurse into structures.  This poses a challenge if the target
/// sink cannot be directly borrowed.  This is where [`SinkHandle`]
/// comes in.  In cases where the [`Sink`] cannot be borrowed it's owned
/// by the handle, either in the arena of the state
/// ([`arena`](Self::arena), which is what sinks typically use) or on the
/// heap ([`heap`](Self::heap)).
///
/// The handle itself implements [`Sink`] and forwards all calls to the
/// sink it holds.
///
/// Unlike the [`SerializeHandle`](crate::ser::SerializeHandle) of
/// serialization, which holds a value that is not serialized yet, this
/// holds a sink that is already deserializing a value.  The serialization
/// equivalent of a sink is an emitter in a [`Emit`](crate::ser::Emit).
/// The constructors line up: [`to`](Self::to) borrows,
/// [`arena`](Self::arena) and [`heap`](Self::heap) own in the same way for
/// both handles.
pub struct SinkHandle<'a, 'de: 'a>(HandleInner<'a, 'de>);

enum HandleInner<'a, 'de> {
    Borrowed(&'a mut dyn Sink<'de>),
    Arena(ArenaSink<'a, 'de>),
    Heap(HeapSink<'a, 'de>),
    #[cfg(feature = "derive")]
    Struct(ArenaStruct<'a, 'de>),
    Null(ignore::Ignore),
    // The optional variants are used to implement `Option<T>` without an
    // extra allocation: a null atom is not forwarded but turns the handle
    // into a null handle so that `finish` is not forwarded either.
    OptionalBorrowed(&'a mut dyn Sink<'de>),
    OptionalArena(ArenaSink<'a, 'de>),
    OptionalHeap(HeapSink<'a, 'de>),
    #[cfg(feature = "derive")]
    OptionalStruct(ArenaStruct<'a, 'de>),
}

impl<'a, 'de> SinkHandle<'a, 'de> {
    /// Create a borrowed handle to a [`Sink`].
    pub fn to(sink: &'a mut dyn Sink<'de>) -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Borrowed(sink))
    }

    /// Creates an owned handle to a sink in the arena of the state.
    ///
    /// This is how sinks are typically created: the arena belongs to the
    /// state of the deserialization and the sinks of the containers that
    /// are open are on top of each other in it, allocating one is little
    /// more than bumping a pointer.  Its space is reused once the handle is
    /// dropped (and the sinks allocated after it are dropped too).
    ///
    /// A sink can outlive the deserialization it was created for (the
    /// state).  The arena then frees its memory except for the chunk the
    /// sink is in, which is freed when the sink is dropped (and the other
    /// sinks in it).  A sink that is meant to be kept for longer should
    /// rather be created with [`heap`](Self::heap).
    ///
    /// ```
    /// use deser::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
    /// use deser::{Error, Event, State};
    ///
    /// /// The number of items of a sequence.
    /// struct Count(usize);
    ///
    /// struct CountSink<'a> {
    ///     out: &'a mut Option<Count>,
    ///     count: usize,
    /// }
    ///
    /// impl<'de> Sink<'de> for CountSink<'_> {
    ///     fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
    ///         Ok(())
    ///     }
    ///
    ///     fn next_value(
    ///         &mut self,
    ///         _state: &mut State,
    ///     ) -> Result<SinkHandle<'_, 'de>, Error> {
    ///         self.count += 1;
    ///         // the items themselves are ignored
    ///         Ok(SinkHandle::null())
    ///     }
    ///
    ///     fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
    ///         *self.out = Some(Count(self.count));
    ///         Ok(())
    ///     }
    /// }
    ///
    /// impl<'de> Deserialize<'de> for Count {
    ///     fn deserialize_into<'a>(
    ///         out: &'a mut Option<Self>,
    ///         state: &mut State,
    ///     ) -> SinkHandle<'a, 'de> {
    ///         SinkHandle::arena(CountSink { out, count: 0 }, state)
    ///     }
    /// }
    ///
    /// let mut out = None::<Count>;
    /// let mut driver = DeserializeDriver::new(&mut out);
    /// for event in [Event::seq_start(), "a".into(), "b".into(), Event::SeqEnd] {
    ///     driver.emit(event).unwrap();
    /// }
    /// drop(driver);
    /// assert_eq!(out.unwrap().0, 2);
    /// ```
    #[inline(always)]
    pub fn arena<S: Sink<'de> + 'a>(sink: S, state: &mut State) -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Arena(arena_sink(sink, &mut state.arena)))
    }

    /// Like [`arena`](Self::arena) but the sink does not need to outlive the
    /// handle.
    ///
    /// This is how the implementations that are generic over adapters (like
    /// `Vec<A>` for `Vec<T>`) create their sinks.  The compiler requires
    /// the sink to outlive the handle, which includes the adapter.  The
    /// adapter always does: it's either the type of the value, which
    /// outlives the slot, or a marker type.  But this cannot be expressed.
    ///
    /// # Safety
    ///
    /// Everything the sink holds has to outlive `'a`.  The type parameters
    /// that do not need to outlive it are the ones of adapters, which are
    /// only used for their functions and never instantiated (the sink holds
    /// no values of them, only markers like `PhantomData<fn() -> A>`).
    /// Functions cannot hold borrowed data.
    #[inline(always)]
    pub(crate) unsafe fn arena_unbounded<S: Sink<'de>>(
        sink: S,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        // SAFETY: guaranteed by the caller
        SinkHandle(HandleInner::Arena(unsafe {
            sinkbox::arena_sink_unbounded(sink, &mut state.arena)
        }))
    }

    /// Drops the handle, the block of an owned sink is returned to the arena
    /// of the state right away if it's the top block.
    ///
    /// Dropping the handle has the same effect, but the block is only
    /// reused when the next sink is allocated.  The driver does this with
    /// the sinks of the containers it closes.
    #[inline(always)]
    pub(crate) fn release(self, state: &mut State) {
        match self.0 {
            HandleInner::Arena(sink) | HandleInner::OptionalArena(sink) => {
                crate::arena::ArenaBox::release_in(sink, &mut state.arena)
            }
            #[cfg(feature = "derive")]
            HandleInner::Struct(sink) | HandleInner::OptionalStruct(sink) => {
                sink.release_in(&mut state.arena)
            }
            _ => {}
        }
    }

    /// Creates an owned handle to a sink on the heap.
    ///
    /// Unlike [`arena`](Self::arena) the sink does not need a state and is
    /// independent of any deserialization, but every sink is a separate
    /// allocation.
    pub fn heap<S: Sink<'de> + 'a>(sink: S) -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Heap(HeapSink::new(sink)))
    }

    /// Creates an owned handle to the sink of a derived struct.
    #[cfg(feature = "derive")]
    #[inline]
    pub(crate) fn from_arena_struct(sink: ArenaStruct<'a, 'de>) -> SinkHandle<'a, 'de> {
        SinkHandle(HandleInner::Struct(sink))
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
            HandleInner::Arena(sink) => HandleInner::Arena(sink),
            HandleInner::Heap(sink) => HandleInner::Heap(sink),
            HandleInner::Null(sink) => HandleInner::Null(sink),
            HandleInner::OptionalBorrowed(sink) => HandleInner::OptionalBorrowed(sink),
            HandleInner::OptionalArena(sink) => HandleInner::OptionalArena(sink),
            HandleInner::OptionalHeap(sink) => HandleInner::OptionalHeap(sink),
            #[cfg(feature = "derive")]
            HandleInner::Struct(sink) => HandleInner::Struct(sink),
            #[cfg(feature = "derive")]
            HandleInner::OptionalStruct(sink) => HandleInner::OptionalStruct(sink),
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
    /// falls back to null.  An empty [`Atom::Lexical`] (like the value of
    /// `?limit=` in a query string) is passed to the wrapped sink, if the
    /// sink rejects it the handle turns into a null handle too.
    ///
    /// This is used to implement `Option<T>`: the slot is set to `Some(None)`
    /// before the handle of the inner value is created and made to ignore
    /// nulls.
    ///
    /// ```
    /// use deser::State;
    /// use deser::de::{Deserialize, SinkHandle};
    ///
    /// /// Deserializes like an `Option<T>`.
    /// fn deserialize_optional<'a, 'de, T: Deserialize<'de>>(
    ///     out: &'a mut Option<Option<T>>,
    ///     state: &mut State,
    /// ) -> SinkHandle<'a, 'de> {
    ///     T::deserialize_into(out.insert(None), state).ignore_null()
    /// }
    /// ```
    pub fn ignore_null(self) -> SinkHandle<'a, 'de> {
        SinkHandle(match self.0 {
            HandleInner::Borrowed(sink) => HandleInner::OptionalBorrowed(sink),
            HandleInner::Arena(sink) => HandleInner::OptionalArena(sink),
            HandleInner::Heap(sink) => HandleInner::OptionalHeap(sink),
            #[cfg(feature = "derive")]
            HandleInner::Struct(sink) => HandleInner::OptionalStruct(sink),
            other => other,
        })
    }

    /// Returns `true` if the handle ignores the atom because it's null.
    ///
    /// In that case the handle turned into a null handle.
    #[inline(always)]
    fn skip_null(&mut self, atom: &Atom) -> bool {
        if self.is_optional() && is_null_atom(atom) {
            *self = SinkHandle::null();
            return true;
        }
        false
    }

    #[inline(always)]
    fn sink(&self) -> &(dyn Sink<'de> + 'a) {
        match self.0 {
            HandleInner::Borrowed(ref sink) | HandleInner::OptionalBorrowed(ref sink) => &**sink,
            HandleInner::Arena(ref sink) | HandleInner::OptionalArena(ref sink) => sink.get(),
            HandleInner::Heap(ref sink) | HandleInner::OptionalHeap(ref sink) => sink.get(),
            #[cfg(feature = "derive")]
            HandleInner::Struct(ref sink) | HandleInner::OptionalStruct(ref sink) => sink.get(),
            HandleInner::Null(ref sink) => sink,
        }
    }

    #[inline(always)]
    fn sink_mut(&mut self) -> &mut (dyn Sink<'de> + 'a) {
        match self.0 {
            HandleInner::Borrowed(ref mut sink) | HandleInner::OptionalBorrowed(ref mut sink) => {
                &mut **sink
            }
            HandleInner::Arena(ref mut sink) | HandleInner::OptionalArena(ref mut sink) => {
                sink.get_mut()
            }
            HandleInner::Heap(ref mut sink) | HandleInner::OptionalHeap(ref mut sink) => {
                sink.get_mut()
            }
            #[cfg(feature = "derive")]
            HandleInner::Struct(ref mut sink) | HandleInner::OptionalStruct(ref mut sink) => {
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
        Atom::Implicit(value) => value.value() == crate::ImplicitValue::Null,
        _ => false,
    }
}

/// Checks if an atom is an empty lexical atom that is a missing value.
///
/// Optionals are `None` for these if the value rejects them (like the
/// empty value of a number in a query string), see [`LexicalRules`].
#[inline]
pub(crate) fn is_empty_lexical(atom: &Atom, state: &State) -> bool {
    matches!(atom, Atom::Lexical(value) if lexical::is_empty_null(value, state))
}

/// Delivers an empty lexical atom to an optional value.
///
/// Returns `false` if the value rejects it (see `ErrorKind::is_rejection`),
/// the optional is `None` then.  As that error is thrown away, it's created
/// without a message (optional numbers are empty in every other row of
/// some CSV files).  Other errors are passed on with their message (the
/// atom is delivered again for this).
#[inline]
pub(crate) fn empty_lexical_or_none<'a>(
    atom: Atom<'a>,
    state: &mut State,
    mut deliver: impl FnMut(Atom<'a>, &mut State) -> Result<(), Error>,
) -> Result<bool, Error> {
    let retry = atom.clone();
    match state.discard_errors(|state| deliver(atom, state)) {
        Ok(()) => Ok(true),
        Err(err) if err.kind().is_rejection() => Ok(false),
        Err(err) if state.discards_errors => Err(err),
        Err(_) => deliver(retry, state).map(|()| true),
    }
}

impl<'a, 'de> SinkHandle<'a, 'de> {
    /// Returns `true` if the handle ignores null atoms (see
    /// [`ignore_null`](Self::ignore_null)).
    #[inline(always)]
    fn is_optional(&self) -> bool {
        match self.0 {
            HandleInner::OptionalBorrowed(_)
            | HandleInner::OptionalArena(_)
            | HandleInner::OptionalHeap(_) => true,
            #[cfg(feature = "derive")]
            HandleInner::OptionalStruct(_) => true,
            _ => false,
        }
    }
}

// The handle forwards to the sink it holds.
impl<'a, 'de> Sink<'de> for SinkHandle<'a, 'de> {
    #[inline]
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if self.skip_null(&atom) {
            return Ok(());
        }
        if self.is_optional() && is_empty_lexical(&atom, state) {
            if !empty_lexical_or_none(atom, state, |atom, state| self.sink_mut().atom(atom, state))?
            {
                *self = SinkHandle::null();
            }
            return Ok(());
        }
        self.sink_mut().atom(atom, state)
    }

    #[inline]
    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        if self.skip_null(&atom) {
            return Ok(());
        }
        if self.is_optional() && is_empty_lexical(&atom, state) {
            let delivered = empty_lexical_or_none(atom, state, |atom, state| {
                self.sink_mut().borrowed_atom(atom, state)
            })?;
            if !delivered {
                *self = SinkHandle::null();
            }
            return Ok(());
        }
        self.sink_mut().borrowed_atom(atom, state)
    }

    #[inline]
    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink_mut().map(state)
    }

    #[inline]
    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink_mut().seq(state)
    }

    #[inline]
    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink_mut().next_key(state)
    }

    #[inline]
    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink_mut().next_value(state)
    }

    #[inline]
    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink_mut().__private_key_atom(atom, state)
    }

    #[inline]
    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink_mut().__private_value_atom(atom, state)
    }

    #[inline]
    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink_mut().__private_borrowed_key_atom(atom, state)
    }

    #[inline]
    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink_mut().__private_borrowed_value_atom(atom, state)
    }

    #[inline]
    fn __private_seq(&mut self, state: &mut State) -> Result<bool, Error> {
        self.sink_mut().__private_seq(state)
    }

    #[inline]
    fn __private_inline_atom(
        &mut self,
        index: usize,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink_mut().__private_inline_atom(index, atom, state)
    }

    #[inline]
    fn __private_inline_event(
        &mut self,
        event: InlineEvent,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink_mut().__private_inline_event(event, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.sink_mut().value_for_key(key, state)
    }

    #[inline]
    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink_mut().finish(state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.sink_mut().recover(err, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink().expecting()
    }
}

/// A trait for deserializable types.
///
/// A type is deserializable if it can create a [`Sink`] for its slot with
/// [`deserialize_into`](Self::deserialize_into).  This is how values are
/// deserialized, but there are two ways to implement it:
///
/// * Values that are deserialized from a single atom (like numbers or
///   strings) implement [`deserialize_atom`](Self::deserialize_atom).  The
///   default implementation of `deserialize_into` returns the slot itself
///   as sink (see [`Slot`]), which passes the atom on.
/// * All other values implement `deserialize_into` and return a sink of
///   their own (see [`SinkHandle::arena`]), which implements the actual
///   deserialization logic.
///
/// Either way, [`expecting`](Self::expecting) says what the value expects
/// in error messages (the sink reports it).  If neither `deserialize_atom`
/// nor `deserialize_into` is implemented, every value is rejected.  See the
/// [module documentation](crate::de) for examples of both.
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
/// # Adapters
///
/// The type parameter `T` is the type of the value that is deserialized.
/// It defaults to `Self`: `impl Deserialize<'de> for Foo` deserializes
/// `Foo` values.  A type that implements `Deserialize` for another type is
/// an adapter, it deserializes values of that type on their behalf (see
/// [`adapters`](crate::adapters)):
///
/// ```
/// use std::borrow::Cow;
/// use deser::de::Slot;
/// use deser::{Atom, Deserialize, Error, State};
///
/// /// Deserializes a `u32` as `u16`.
/// pub struct Small;
///
/// impl<'de> Deserialize<'de, u32> for Small {
///     fn deserialize_atom(
///         slot: &mut Slot<u32, Self>,
///         atom: Atom,
///         state: &mut State,
///     ) -> Result<(), Error> {
///         let mut value = None::<u16>;
///         u16::deserialize_atom(Slot::wrap(&mut value), atom, state)?;
///         **slot = value.map(u32::from);
///         Ok(())
///     }
///
///     fn expecting() -> Cow<'static, str> {
///         Cow::Borrowed("u16")
///     }
/// }
/// ```
///
/// Adapters are never instantiated, only their functions are used.
///
/// # Thread Safety
///
/// Deserializable values are `Send` and so are the sinks they create.  This
/// allows an ongoing deserialization (a [`DeserializeDriver`]) to move
/// between threads, for instance when it is suspended while waiting for more
/// input.  Types that are not `Send` (such as `Rc`) cannot be deserialized,
/// which is why `T` has to be `Send`.
pub trait Deserialize<'de, T: Send = Self>: Sized + Send {
    /// Creates a sink that deserializes the value into the given slot.
    ///
    /// This is how every value is deserialized, whichever way the type
    /// implements `Deserialize`.  The default implementation returns the
    /// slot itself as sink (see [`Slot`]), which passes atoms to
    /// [`deserialize_atom`](Self::deserialize_atom).  This is what values
    /// that are deserialized from atoms use.  Values that need state to be
    /// deserialized (like maps and sequences) implement this and return
    /// their own sink (see [`SinkHandle::arena`]).
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        // the slot is the sink, nothing is allocated in the state (the atoms
        // come with the state)
        let _ = state;
        Slot::<T, Self>::handle(out)
    }

    /// Deserializes an atom into the slot.
    ///
    /// This is invoked by the sink of the default implementation of
    /// [`deserialize_into`](Self::deserialize_into) (the slot itself, see
    /// [`Slot`]) for every atom it receives.  The value is placed in the
    /// slot, atoms which are not accepted are passed to [`default_atom`]
    /// (with the slot as sink).  The default implementation does this for
    /// every atom.
    ///
    /// Types that implement `deserialize_into` do not use this.  This also
    /// means that calling it only deserializes values that implement it
    /// (like the primitives), other values reject every atom.  To
    /// deserialize an atom into any value, pass it to the sink returned by
    /// `deserialize_into`.
    fn deserialize_atom(
        slot: &mut Slot<T, Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        default_atom(slot, atom, state)
    }

    /// Deserializes an atom that borrows from the data being deserialized
    /// into the slot.
    ///
    /// This is like [`deserialize_atom`](Self::deserialize_atom) for
    /// [`Sink::borrowed_atom`], which it forwards to by default.  Only values
    /// which borrow (like `&'de str`) need to implement it.
    fn deserialize_borrowed_atom(
        slot: &mut Slot<T, Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        <Self as Deserialize<'de, T>>::deserialize_atom(slot, atom, state)
    }

    /// Returns what the value expects, for error messages.
    ///
    /// This is what the errors of values that do not match say they expect
    /// (`unexpected map, expected u32`).  The sink of the value reports it
    /// with [`Sink::expecting`]: the [`Slot`] returns this, sinks returned
    /// by [`deserialize_into`](Self::deserialize_into) should return it as
    /// well.  The derive returns the name of the type (or what
    /// `#[deser(expecting = "...")]` says), wrappers like `Option<T>` and
    /// `Box<T>` what their value expects.  The default implementation
    /// returns the name of the type without the module paths (like
    /// `Vec<Point>`), which is the name of the adapter for adapters.
    fn expecting() -> Cow<'static, str> {
        slot::short_type_name(core::any::type_name::<Self>())
    }

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
    fn initial_value() -> Option<T> {
        None
    }

    /// Creates a sink that updates an existing value.
    ///
    /// This is used to apply data on top of a value, for instance to layer a
    /// configuration file over the defaults (see
    /// [`DeserializeDriver::update`] and [`Deserializer::update`]).  The
    /// default implementation replaces the value with the deserialized one.
    /// Derived structs update the fields that are given and keep the others
    /// (fields are updated the same way, so nested structs are merged).
    /// `Option` updates the value in it if it's set, null clears it.  `Box`
    /// updates the value in it.  `HashMap` and `BTreeMap` insert the given
    /// entries, the values of keys that exist are replaced.
    ///
    /// If the update fails, the value might be partially updated.
    fn deserialize_update<'out>(value: &'out mut T, state: &mut State) -> SinkHandle<'out, 'de> {
        update::replace_handle_with(
            value,
            <Self as Deserialize<'de, T>>::deserialize_into,
            state,
        )
    }

    /// Deserializes an atom into the slot.
    ///
    /// This must behave exactly like invoking [`atom`](Sink::atom) and
    /// [`finish`](Sink::finish) on the sink returned by
    /// [`deserialize_into`](Self::deserialize_into), which is what the default
    /// implementation does.  Types that are deserialized with a [`Slot`]
    /// override this so that atoms can be deserialized without dynamic
    /// dispatch.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_atom_into(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        atom_into_handle(
            <Self as Deserialize<'de, T>>::deserialize_into(out, state),
            atom,
            state,
        )
    }

    /// Deserializes a borrowed atom into the slot.
    ///
    /// This is like [`__private_atom_into`](Self::__private_atom_into) but
    /// for [`borrowed_atom`](Sink::borrowed_atom).
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_borrowed_atom_into(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        borrowed_atom_into_handle(
            <Self as Deserialize<'de, T>>::deserialize_into(out, state),
            atom,
            state,
        )
    }

    /// Returns `true` if the values are `u8`.
    ///
    /// This is used to specialize the handling of bytes for vectors and
    /// arrays of `u8`.
    ///
    /// Internal specialization of bytes, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_is_bytes() -> bool {
        false
    }

    /// Converts bytes into a vector of values.
    ///
    /// This is only implemented for `u8` and used to specialize the
    /// deserialization of `Vec<u8>` from bytes.
    ///
    /// Internal specialization of bytes, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<T>> {
        let _ = bytes;
        None
    }

    /// Converts bytes into an array of values.
    ///
    /// This is only implemented for `u8` and used to specialize the
    /// deserialization of `[u8; N]` from bytes.  Returns `None` if the
    /// type is not `u8` or the length does not match.
    ///
    /// Internal specialization of bytes, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
        let _ = bytes;
        None
    }

    /// Returns the value of a type that is only deserialized from atoms.
    ///
    /// This is implemented for numbers and booleans, sequences of them are
    /// built inline (see [`InlineSeq`]).  The value is a placeholder, it's
    /// overwritten.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_atom_default() -> Option<T> {
        None
    }

    /// Returns how the value is built inline if it's a sequence of atoms.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_inline_seq() -> Option<InlineSeq<T>> {
        None
    }

    /// Returns the format if the value is deserialized as raw value.
    ///
    /// This is the format of [`Raw`](crate::ext::Raw) values (and wrappers
    /// of them like `Option` and `Box`).  The sinks of containers request
    /// the values of such types as raw values from the format before they
    /// start (see [`State::__private_request_raw`]).
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    #[inline(always)]
    fn __private_raw() -> Option<&'static crate::ext::RawFormatInfo> {
        None
    }

    /// Returns `true` if the value collects the values of a repeated key.
    ///
    /// This is `true` for collections like `Vec<T>` and sets (and
    /// `Option`s of them).  In a multimap (see
    /// [`ContainerShape::with_multimap`](crate::ContainerShape::with_multimap))
    /// fields and map values of these types receive every value of their
    /// key through [`__private_collect_into`](Self::__private_collect_into)
    /// and [`__private_collect_update`](Self::__private_collect_update).
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    fn __private_collects() -> bool {
        false
    }

    /// Returns `true` if the value rejects empty lexical atoms.
    ///
    /// Optionals of such values are `None` for empty text (if it's a
    /// missing value, see [`LexicalRules`]) without delivering it, which
    /// saves creating the error that would be thrown away (optional numbers
    /// are empty in every other row of some CSV files).  This is `true` for
    /// numbers and booleans.
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    #[inline(always)]
    fn __private_rejects_empty_lexical() -> bool {
        false
    }

    /// Returns a sink for a value that is added to the collection in the
    /// slot.
    ///
    /// The collection is created if the slot is empty.  This is only used
    /// if [`__private_collects`](Self::__private_collects) returns `true`.
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    fn __private_collect_into<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <Self as Deserialize<'de, T>>::deserialize_into(out, state)
    }

    /// Returns a sink for a value that is added to a collection that is
    /// updated.
    ///
    /// The value that is added `first` replaces the collection.  This is
    /// only used if [`__private_collects`](Self::__private_collects) returns
    /// `true`.
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    fn __private_collect_update<'out>(
        value: &'out mut T,
        first: bool,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        let _ = first;
        <Self as Deserialize<'de, T>>::deserialize_update(value, state)
    }

    /// Returns the value of a collection whose key is missing in a
    /// multimap.
    ///
    /// Collections are empty then.  `None` means that the field is missing
    /// (or has its [`initial_value`](Self::initial_value)).
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    fn __private_collect_empty() -> Option<T> {
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

/// Returns the value of a key that is missing in a multimap.
///
/// In a multimap (see
/// [`ContainerShape::with_multimap`](crate::ContainerShape::with_multimap))
/// collections like `Vec<T>` and sets are empty if their key is missing.
/// Other types have their [`initial_value`](Deserialize::initial_value)
/// (`None` for `Option<T>`).  `None` means that the value is required.
/// This is the counterpart of
/// [`DeserializeDriver::multimap_value`] for a key that is not there.
///
/// ```
/// use deser::de::missing_multimap_value;
///
/// assert_eq!(missing_multimap_value::<Vec<u16>>(), Some(vec![]));
/// assert_eq!(missing_multimap_value::<Option<u16>>(), Some(None));
/// assert_eq!(missing_multimap_value::<u16>(), None);
/// ```
pub fn missing_multimap_value<'de, T: Deserialize<'de>>() -> Option<T> {
    T::__private_collect_empty().or_else(T::initial_value)
}

/// Converts a sink into a trait object.
///
/// This is implemented for all sinks.  The default methods of [`Sink`] exist
/// for every sink type, they use this to forward to code that exists once.
///
/// Internal fast path, not public API (see `lib.rs`).
#[doc(hidden)]
pub trait AsDynSink<'de> {
    fn __private_as_dyn(&mut self) -> &mut dyn Sink<'de>;
}

impl<'de, T: Sink<'de>> AsDynSink<'de> for T {
    #[inline(always)]
    fn __private_as_dyn(&mut self) -> &mut dyn Sink<'de> {
        self
    }
}

/// Trait to place values in a slot.
///
/// A sink acts as an abstraction to receive a value during deserialization from
/// the deserializer.  Sinks in deser are one-shot receivers.  A deserializer must
/// invoke one receiver method for a total of zero or one times.
///
/// The sink then places the received value in the slot connected to the sink.
///
/// Values that are deserialized from a single atom do not need to implement
/// a sink, the [`Slot`] is their sink (see
/// [`Deserialize::deserialize_atom`]).  Sinks are implemented for values
/// that need state, like structs, maps and sequences.
///
/// # Borrowed Data
///
/// Atoms are passed to [`atom`](Self::atom) with a lifetime that only lasts
/// for the call.  Formats pass atoms which borrow from the data that is
/// deserialized (which lives for `'de`) to [`borrowed_atom`](Self::borrowed_atom)
/// instead.  By default this forwards to [`atom`](Self::atom), only sinks of
/// types which want to borrow (like `&'de str`) need to implement it.
pub trait Sink<'de>: Send + AsDynSink<'de> {
    /// Receives an [`Atom`].
    ///
    /// Atoms which are not accepted are passed to [`default_atom`], the
    /// default handling of atoms (which is what the default implementation
    /// does for every atom).  This is
    /// particularly important for [`Atom::Ext`] as extension values are
    /// passed on as their fallback.  Values that are deserialized from atoms
    /// implement [`Deserialize::deserialize_atom`] instead of a sink.
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        default_atom(self.__private_as_dyn(), atom, state)
    }

    /// Receives an [`Atom`] that borrows from the data being deserialized.
    ///
    /// The default implementation forwards to [`atom`](Self::atom).
    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.atom(atom, state)
    }

    /// Begins the deserialization of a map.
    ///
    /// While the deserialization of a map is ongoing the methods
    /// [`next_key`](Self::next_key) and [`next_value`](Self::next_value) are
    /// called alternatingly.  The map is ended by [`finish`](Self::finish).
    ///
    /// The default implementation returns an error.
    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        default_container(self.__private_as_dyn(), "map", state)
    }

    /// Begins the receiving process for sequences.
    ///
    /// While the deserialization of a sequence is ongoing the method
    /// [`next_value`](Self::next_value) is called for every new item.
    /// The sequence is ended by [`finish`](Self::finish).
    ///
    /// The default implementation returns an error.
    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        default_container(self.__private_as_dyn(), "sequence", state)
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
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        default_key_atom(self.__private_as_dyn(), atom, state)
    }

    /// Receives an atom as the next value in a map or sequence.
    ///
    /// This is a shortcut for invoking [`next_value`](Self::next_value) and
    /// then [`atom`](Self::atom) and [`finish`](Self::finish) on the returned
    /// sink, which is exactly what the default implementation does.  See
    /// [`__private_key_atom`](Self::__private_key_atom) for more information.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        default_value_atom(self.__private_as_dyn(), atom, state)
    }

    /// Receives a borrowed atom as the next key in a map.
    ///
    /// Like [`__private_key_atom`](Self::__private_key_atom) but the atom is
    /// passed to [`borrowed_atom`](Self::borrowed_atom).
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        default_borrowed_key_atom(self.__private_as_dyn(), atom, state)
    }

    /// Receives a borrowed atom as the next value in a map or sequence.
    ///
    /// Like [`__private_value_atom`](Self::__private_value_atom) but the atom
    /// is passed to [`borrowed_atom`](Self::borrowed_atom).
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        default_borrowed_value_atom(self.__private_as_dyn(), atom, state)
    }

    /// Begins a sequence like [`seq`](Self::seq).
    ///
    /// Returns `true` if the sink builds sequences that are its elements
    /// inline (see [`InlineSeq`]): the driver then passes their events to
    /// [`__private_inline_atom`](Self::__private_inline_atom) and
    /// [`__private_inline_event`](Self::__private_inline_event) instead of
    /// asking for a sink for them.  Wrappers that forward this have to
    /// forward those as well.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_seq(&mut self, state: &mut State) -> Result<bool, Error> {
        self.seq(state)?;
        Ok(false)
    }

    /// Receives the atom at the index of an element that is built inline.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_inline_atom(
        &mut self,
        index: usize,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let _ = (index, atom, state);
        no_inline_seq()
    }

    /// Receives the other events of an element that is built inline.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    fn __private_inline_event(
        &mut self,
        event: InlineEvent,
        state: &mut State,
    ) -> Result<(), Error> {
        let _ = (event, state);
        no_inline_seq()
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

    /// Called when an item of this map or sequence failed.
    ///
    /// This is invoked by the [`DeserializeDriver`] when the key or value
    /// that was started last in this container failed with an error, either
    /// because its sink (or a sink nested in it) returned the error or
    /// because this sink returned it while handling the item (for instance
    /// from [`next_value`](Self::next_value) or
    /// [`__private_value_atom`](Self::__private_value_atom)).  Errors of
    /// this sink's own [`map`](Self::map), [`seq`](Self::seq) and
    /// [`finish`](Self::finish) are errors of this sink's value and go to
    /// the container this sink is an item of.
    ///
    /// A sink that returns `Ok` recovers from the error: the driver skips
    /// the remaining events of the failed item (and the value of a failed
    /// key) and deserialization continues with the next item.  Returning
    /// the error (which is what the default implementation does) passes it
    /// on to the enclosing container.  All sinks of the failed item are
    /// dropped before this is invoked.  The error already has the context of
    /// the event that failed attached (see [`Error`]).
    ///
    /// Only errors of sinks are recoverable: errors of the format and of
    /// [`Layer`]s end the deserialization.
    ///
    /// Sinks that forward [`next_key`](Self::next_key) and
    /// [`next_value`](Self::next_value) to another sink should forward this
    /// as well.
    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        let _ = state;
        Err(err)
    }

    /// Returns what the sink expects, for error messages.
    ///
    /// The sink of a value returns what the value expects (see
    /// [`Deserialize::expecting`]), sinks which pass values on to another
    /// sink what that sink expects.  The default implementation returns
    /// `"compatible type"`.
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("compatible type")
    }
}
