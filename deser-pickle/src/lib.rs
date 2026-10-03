//! Parse and serialize Python's pickle format compatible with deser.
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Session {
//!     user_id: u64,
//!     roles: Vec<String>,
//! }
//!
//! let session = Session { user_id: 42, roles: vec!["admin".into()] };
//! let bytes = deser_pickle::to_vec(&session).unwrap();
//! // what `pickle.dumps({"user_id": 42, "roles": ["admin"]}, 4)` reads back
//! assert_eq!(deser_pickle::from_slice::<Session>(&bytes).unwrap(), session);
//! ```
//!
//! **Pickles are programs.**  Python runs them and imports and calls
//! whatever they ask for, which is why unpickling untrusted data in Python
//! is unsafe.  This crate never runs code: it runs the pickle machine but
//! instead of importing classes and calling them it records what would
//! have been called (see [Objects](#objects)).
//!
//! # Data Model
//!
//! Python's values map onto the deser data model as follows:
//!
//! | Python                           | deser                                         |
//! |----------------------------------|-----------------------------------------------|
//! | `None`                           | `Null`                                        |
//! | `bool`                           | `Bool`                                        |
//! | `int`                            | `U64`, `I64`, `u128`, `i128` or [`BigInt`](deser_core::ext::BigInt) |
//! | `float`                          | `F64`                                         |
//! | `str`                            | `Str`                                         |
//! | `bytes`                          | `Bytes`                                       |
//! | `bytearray`                      | `Bytes` (with [`Kind::ByteArray`])            |
//! | `str` of Python 2                | `Str` if valid UTF-8, `Bytes` otherwise       |
//! | `list`                           | sequences                                     |
//! | `tuple`, `set`, `frozenset`      | sequences (with a [`Kind`])                   |
//! | `dict`                           | maps (keys can be any value)                  |
//! | classes and functions            | [`Global`] (the dotted path as fallback)      |
//! | other objects                    | their state with a [class](#objects)          |
//! | a value that contains itself     | [`Reference`] (see [References](#references)) |
//!
//! Empty sequences and maps are both: an empty list deserializes into a
//! struct (with defaults) and an empty dict into a `Vec`.
//!
//! Deserialization is strict where Python is lenient: data after the
//! `STOP` opcode is an error (Python ignores it).  Strings that contain
//! surrogates (which Python allows) cannot be held by Rust strings and are
//! an error.  Python checks that the keys of dicts and the items of sets
//! are hashable and merges equal ones, this crate passes them on as they
//! are.  Persistent ids, extension codes and out-of-band buffers (which
//! need help from the code that unpickles) are not supported.
//!
//! # Objects
//!
//! Pickles create instances of classes by calling them (or `__new__`) with
//! arguments, then set their state (`BUILD`, which calls `__setstate__`)
//! and add items (for list and dict subclasses).  Instead of calling them,
//! this crate records the calls.  An object is emitted as
//!
//! * its items, if items were added to it (a map for `__setitem__`, a
//!   sequence for `append` and `extend`),
//! * else its state: a map if it's a dict (the instance dictionary) or a
//!   tuple of two dicts (the dictionary and the slots, which are merged),
//!   the state itself otherwise,
//! * else its arguments: no arguments are an empty map (or the keyword
//!   arguments), one is the argument, more are a tuple.
//!
//! A plain class instance (including dataclasses) is a map of its
//! attributes, an `OrderedDict` a map of its items, a `Decimal` its text
//! and an enum member its value.  The class is passed on out of band as
//! event data together with the [`Form`] the object is created in from
//! the value: [`take_class`] and [`take_form`] return them, [`set_class`]
//! and [`set_form`] set them for serialization and [`Object`] captures
//! them.  Types that do not care about classes never see them:
//!
//! ```rust
//! use deser_pickle::{Form, Global, Object};
//!
//! #[derive(Debug, deser::Deserialize, deser::Serialize)]
//! struct User {
//!     name: String,
//! }
//!
//! // `pickle.dumps(User(name="Jane"), 4)` with a class `User` of `app`
//! let input = b"\x80\x04\x95%\x00\x00\x00\x00\x00\x00\x00\x8c\x03app\x94\x8c\x04User\x94\x93\x94)\x81\x94}\x94\x8c\x04name\x94\x8c\x04Jane\x94sb.";
//! let user: User = deser_pickle::from_slice(input).unwrap();
//! assert_eq!(user.name, "Jane");
//!
//! let user: Object<User> = deser_pickle::from_slice(input).unwrap();
//! assert_eq!(user.class, Some(Global::new("app", "User")));
//! assert_eq!(user.form, Some(Form::State));
//!
//! // written back as Python writes it: `User.__new__(User)` with the state
//! assert_eq!(
//!     deser_pickle::to_vec(&user).unwrap(),
//!     b"\x80\x04\x8c\x03app\x8c\x04User\x93)\x81}(\x8c\x04name\x8c\x04Janeub."
//! );
//! ```
//!
//! Objects that need both arguments and state (like classes with
//! `__getnewargs__`) are emitted as their state, their arguments are lost.
//!
//! The globals that stand for builtin types are understood: `set`,
//! `frozenset`, `bytearray`, `bytes`, `_codecs.encode` (bytes of protocols
//! 0 to 2) and `copyreg._reconstructor` (objects of protocols 0 and 1).
//! Like Python, the names of Python 2 (such as `__builtin__.unicode`) are
//! read as the ones of Python 3 (`builtins.str`) before protocol 3.
//!
//! # References
//!
//! A pickle is a graph: values can be reached more than once (the memo of
//! the pickle refers back to them) and values can contain themselves (a
//! list that contains itself, a child object that refers to its parent).
//! The data model of deser is a tree:
//!
//! * A value that is reached more than once is emitted at every place.
//!   Every time its first event carries the same id as event data (see
//!   [`take_shared_id`]), which allows types to share them again.
//!   [`DeserializerConfig::set_max_shared_events`] limits how much is
//!   emitted for repeated values.
//! * Where a value is reached again from within itself (a cycle) it cannot
//!   be emitted again.  A [`Reference`] with its id is emitted instead,
//!   whose fallback is `null`: types which do not understand references see
//!   a missing value, an `Option` is `None`.
//!
//! ```rust
//! #[derive(Debug, deser::Deserialize)]
//! struct Node {
//!     name: String,
//!     parent: Option<Box<Node>>,
//!     children: Vec<Node>,
//! }
//!
//! // a root with a child whose parent is the root (`tree.Node`)
//! let input = b"\x80\x04\x95[\x00\x00\x00\x00\x00\x00\x00\x8c\x04tree\x94\x8c\x04Node\x94\x93\x94)\x81\x94}\x94(\x8c\x04name\x94\x8c\x04root\x94\x8c\x06parent\x94N\x8c\x08children\x94]\x94h\x02)\x81\x94}\x94(h\x05\x8c\x05child\x94h\x07h\x03h\x08]\x94ubaub.";
//! let root: Node = deser_pickle::from_slice(input).unwrap();
//! assert_eq!(root.children[0].name, "child");
//! assert!(root.children[0].parent.is_none());
//! ```
//!
//! The serializer writes values with an id once and refers to them after
//! that, so values deserialized into types that keep the event data (like
//! [`deser_value::Value`](https://docs.rs/deser-value)) are written back
//! with their sharing and cycles.  Tuples that contain themselves cannot be
//! written.
//!
//! # Serialization
//!
//! The serializer writes protocol 4 by default (protocols 2 to 5 are
//! supported, see [`SerializerConfig::set_protocol`]):
//!
//! * sequences are lists, unless they have a [`Kind`]: tuples, sets and
//!   frozensets.  Sequences in keys of maps are tuples.
//! * maps are dicts.  Keys can be any value but maps.
//! * bytes are `bytes` (or `bytearray`s with [`Kind::ByteArray`]).
//! * values with a [class](#objects) are objects, created in their
//!   [`Form`].  Without a form, maps are the state of `cls.__new__(cls)`
//!   (like Python pickles a class instance), lists are appended to the new
//!   object (like a list subclass), tuples are the arguments of the class
//!   (`cls(*value)`) and other values the argument (`cls(value)`).
//! * before protocol 3 the globals are written with the names of Python 2
//!   (like Python does).
//! * [`Global`]s are globals, [`Reference`]s refer to the value of their id.
//! * other extension types (such as UUIDs and decimals) are written as
//!   their fallback, usually a string.
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library with [`from_reader`] and [`to_writer`] and the
//!   readers and writers of [`deser::io`](deser_core::io)
//!   ([`DeserializerConfig::reader`] and [`SerializerConfig::writer`]).
//!   Requires `std`.  The stream serializer ([`Serializer`]) and
//!   deserializer ([`StreamDeserializer`]) do not need it.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod compat;
mod de;
mod emit;
mod ser;
mod stream;
mod text;
mod types;
mod vm;

pub use self::de::{Deserializer, DeserializerConfig, DeserializerConfigBuilder, Iter, from_slice};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Serializer, SerializerConfig, SerializerConfigBuilder, to_vec};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
pub use self::types::{
    Form, Global, Kind, Object, Reference, set_class, set_form, set_kind, set_shared_id,
    take_class, take_form, take_kind, take_shared_id,
};

// the examples of the readme are tested
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
