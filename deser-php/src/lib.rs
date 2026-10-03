//! Parse and serialize PHP's serialization format (the format of PHP's
//! `serialize` and `unserialize`) compatible with deser.
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
//! let bytes = deser_php::to_vec(&session).unwrap();
//! assert_eq!(
//!     bytes,
//!     br#"a:2:{s:7:"user_id";i:42;s:5:"roles";a:1:{i:0;s:5:"admin";}}"#
//! );
//! assert_eq!(deser_php::from_slice::<Session>(&bytes).unwrap(), session);
//! ```
//!
//! # Data Model
//!
//! PHP's values map onto the deser data model as follows:
//!
//! | PHP                                  | deser                                   |
//! |--------------------------------------|-----------------------------------------|
//! | `null` (`N;`)                        | `Null`                                  |
//! | booleans (`b:`)                      | `Bool`                                  |
//! | integers (`i:`)                      | `U64`, `I64`                            |
//! | floats (`d:`)                        | `F64`                                   |
//! | strings (`s:` and `S:`)              | `Str` if valid UTF-8, `Bytes` otherwise |
//! | arrays with the keys `0`, `1`, ...   | sequences                               |
//! | other arrays                         | maps                                    |
//! | objects (`O:`)                       | maps with a [class](#classes)           |
//! | enum cases (`E:`)                    | `Str` (the case) with a [class](#classes) |
//! | custom serialized objects (`C:`)     | `Bytes` (the payload) with a [class](#classes) |
//! | references (`r:` and `R:`)           | [`Reference`] (see [References](#references)) |
//!
//! Arrays are both lists and maps in PHP.  Arrays whose keys are `0`, `1`,
//! `2`, ... in this order are sequences, all others are maps.  The empty
//! array is both: it's an empty sequence that types which expect a map
//! (like structs) take as an empty map (see
//! [`ContainerShape::set_ambiguous_empty`](deser_core::ContainerShape::set_ambiguous_empty)).
//! The keys of maps are integers and strings, integers
//! are passed on as [`Implicit`](deser_core::Atom::Implicit) atoms: maps
//! with string keys take their text, maps with integer keys their value.
//! Like PHP, strings that are the text of an integer (`"5"` but not `"05"`)
//! are integer keys.
//!
//! PHP strings are bytes.  Strings that are valid UTF-8 are passed on as
//! text, all others as bytes.  Types that expect bytes (like `Vec<u8>`)
//! take the bytes of the text rather than decoding it as base64 (unless the
//! context configures a [`BytesFormat`](deser_core::BytesFormat)).
//!
//! Deserialization is strict where PHP is lenient: data after the value is
//! an error (PHP ignores it with a warning) and so are integers that do not
//! fit into 64 bits (PHP clamps them).  The functions that deserialize a
//! value never instantiate classes or run code, objects are just maps.
//!
//! # Classes
//!
//! Objects, enum cases and custom serialized objects have a class.  It's
//! passed on out of band as event data of the value: [`take_class`]
//! returns it, [`set_class`] sets it for serialization and [`Object`]
//! captures it.  Types that do not care about classes never see them, an
//! object deserializes into a struct or map like an array:
//!
//! ```rust
//! use deser_php::Object;
//!
//! #[derive(Debug, deser::Deserialize, deser::Serialize)]
//! struct User {
//!     name: String,
//! }
//!
//! let input = br#"O:4:"User":1:{s:4:"name";s:4:"Jane";}"#;
//! let user: User = deser_php::from_slice(input).unwrap();
//! assert_eq!(user.name, "Jane");
//!
//! let user: Object<User> = deser_php::from_slice(input).unwrap();
//! assert_eq!(user.class.as_deref(), Some("User"));
//! assert_eq!(deser_php::to_vec(&user).unwrap(), input);
//! ```
//!
//! The names of protected and private properties have a prefix in PHP's
//! format (`\0*\0name` and `\0Class\0name`).  The deserializer removes it,
//! so properties deserialize into fields of the same name whatever their
//! visibility.  The visibility is event data of the key (see
//! [`take_visibility`] and [`set_visibility`]).
//!
//! Enum cases are strings, so they deserialize into enums with unit
//! variants of the same name.  Custom serialized objects (classes that
//! implement `Serializable`) are written in a format that only the class
//! knows, their payload is passed on as bytes.
//!
//! # References
//!
//! PHP writes values that appear more than once (the same object, or
//! values that were assigned by reference) once and refers back to them
//! with a number.  **References are not resolved:** they are passed on as
//! [`Reference`] markers which hold the number and the serializer writes
//! them back as they are.  The marker is close to useless for anything but
//! detecting references and writing the input back unchanged: the number
//! refers to the position of a value in the whole input, and there is no
//! way to get from it to the value (see [`Reference`]).  Other types than
//! [`Reference`] receive the number as integer.
//!
//! # Serialization
//!
//! The serializer writes what PHP's `serialize` writes for the same values:
//!
//! * sequences are arrays with the keys `0`, `1`, ..., maps and structs are
//!   arrays with their keys.  Maps with a [class](#classes) are objects.
//! * keys are integers or strings.  Strings that are the text of an
//!   integer are written as integer like PHP does, booleans are `0` and
//!   `1`.  Other keys are an error.
//! * floats are written with the shortest text that reads back as the same
//!   value, like PHP does (`0.1`, `1.0E+25`, `INF`, `NAN`).
//! * bytes are strings, PHP strings are bytes.  Integers out of the range
//!   of `i64` are an error.
//! * other extension types (such as UUIDs and decimals) are written as
//!   their fallback, usually a string.
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library with [`from_reader`] and [`to_writer`] and the
//!   readers and writers of [`deser::io`](deser_core::io)
//!   ([`DeserializerConfig::reader`] and [`SerializerConfig::writer`]).
//!   Values are validated before they are deserialized, the whole stream
//!   is read before its values are parsed.  Requires `std`.  The stream
//!   serializer ([`Serializer`]) and deserializer ([`StreamDeserializer`])
//!   do not need it.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod de;
mod float;
mod object;
mod parser;
mod reference;
mod ser;
mod stream;

pub use self::de::{
    Deserializer, DeserializerConfig, DeserializerConfigBuilder, Iter, from_slice, from_str,
};
pub use self::object::{
    Object, Visibility, set_class, set_visibility, take_class, take_visibility,
};
pub use self::reference::{Reference, ReferenceKind};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Serializer, SerializerConfig, SerializerConfigBuilder, to_vec};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;

// the examples of the readme are tested
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
