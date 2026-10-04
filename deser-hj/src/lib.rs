//! Parse [Hjson](https://hjson.github.io/) compatible with deser.
//!
//! Hjson is JSON for humans, a format for configuration files:
//!
//! * `#`, `//` and `/* */` comments,
//! * commas between values are optional, commas after the last element of
//!   sequences and maps are allowed,
//! * map keys without quotes (`name: api`), they end at whitespace and
//!   the punctuators `{}[],:`,
//! * strings without quotes, which end at the end of the line (`note:
//!   runs until the end, #, and // too`),
//! * multiline strings in triple single quotes (`'''`), the indentation up
//!   to the column of the opening quotes is removed,
//! * strings in single quotes,
//! * the braces of a map at the root can be omitted.
//!
//! Numbers, `true`, `false` and `null` without quotes are values only if
//! nothing but whitespace, a comma, the end of a container or a comment
//! follows them on the line: `5 # minutes` is the number `5`, `5 minutes`
//! is a string.  Their type is inferred from their text, so they are
//! passed on as [`Implicit`](deser_core::Implicit) values: an `u16`
//! receives `8080` as number, a `String` as `"8080"`.
//!
//! Otherwise this works like [`deser-json`](https://docs.rs/deser-json):
//! strings and keys without escape sequences are borrowed from the input and
//! the positions of errors and values refer to the input.  In [JSON
//! Lines](Trailing::Newline) every line break ends a value.  The parser passes the [Hjson test
//! suite](https://github.com/hjson/hjson/tree/master/testCases).
//!
//! ```rust
//! #[derive(deser::Deserialize)]
//! struct Config<'a> {
//!     name: &'a str,
//!     version: String,
//!     ports: Vec<u16>,
//!     motd: String,
//! }
//!
//! let config: Config = deser_hj::from_str(r#"
//!     ## the name of the service
//!     name: api
//!     version: 2
//!     ports: [80, 443]
//!     motd:
//!         '''
//!         Welcome!
//!         Have a nice day.
//!         '''
//! "#).unwrap();
//! assert_eq!(config.name, "api");
//! assert_eq!(config.version, "2");
//! assert_eq!(config.ports, [80, 443]);
//! assert_eq!(config.motd, "Welcome!\nHave a nice day.");
//! ```
//!
//! JSON is valid Hjson, so values are serialized as JSON (with the same
//! serializer as `deser-json`).
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library (with `DeserializerConfig::reader`).  Requires
//!   `std`.
//! * `speedups` (enabled by default): faster UTF-8 validation.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

// These are generated from `deser-template-json`.
mod buf;
mod copy;
mod de;
mod escape;
mod parser;
mod pretty;
mod scan;
mod ser;
mod stream;
mod trailing;

pub use self::de::{
    Deserializer, DeserializerConfig, DeserializerConfigBuilder, Iter, from_slice, from_str,
};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{
    Indent, InlinePolicy, Serializer, SerializerConfig, SerializerConfigBuilder, to_string,
};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
pub use self::trailing::Trailing;
