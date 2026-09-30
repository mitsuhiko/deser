//! Parse [JSON5](https://json5.org/) compatible with deser.
//!
//! JSON5 extends JSON with syntax from ECMAScript 5.1:
//!
//! * `//` and `/* */` comments,
//! * commas after the last element of sequences and maps,
//! * map keys which are ECMAScript identifiers (`{name: "api"}`, with
//!   Unicode letters and `\u` escapes),
//! * strings in single quotes, escaped line breaks within strings and the
//!   escapes `\'`, `\v`, `\0` and `\xFF`,
//! * hexadecimal numbers (those that do not fit into 64 bits are 128 bit
//!   integers and floats beyond that), numbers with a leading `+` or a
//!   leading or trailing decimal point, `Infinity` and `NaN`,
//! * more whitespace characters.
//!
//! Otherwise this works like [`deser-json`](https://docs.rs/deser-json):
//! strings and identifiers without escape sequences are borrowed from the
//! input and the positions of errors and values refer to the input.  In
//! [JSON Lines](Trailing::Newline) only line breaks outside of comments and
//! strings end a value.  The parser passes the [JSON5 test
//! suite](https://github.com/json5/json5-tests).
//!
//! ```rust
//! #[derive(deser::Deserialize)]
//! struct Config<'a> {
//!     name: &'a str,
//!     ports: Vec<u16>,
//!     timeout: f64,
//! }
//!
//! let config: Config = deser_json5::from_str(r#"{
//!     // the name of the service
//!     name: 'api',
//!     ports: [0x50, 443,],
//!     timeout: .5,
//! }"#).unwrap();
//! assert_eq!(config.name, "api");
//! assert_eq!(config.ports, [80, 443]);
//! assert_eq!(config.timeout, 0.5);
//! ```
//!
//! JSON is valid JSON5, so values are serialized as JSON with the
//! serializer of `deser-json` (which is re-exported).
//!
//! # Raw Values
//!
//! [`RawJson5`] holds the JSON5 text of a value.  Like `deser_json::RawJson` it
//! keeps the text of values that are deserialized from JSON5 as it is
//! (including comments), other values are encoded as JSON.  See
//! [`Raw`](deser_core::ext::Raw) for more information.
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library (with `DeserializerConfig::reader`).  Requires
//!   `std`.
//! * `speedups`: faster UTF-8 validation.  Implies `zmij`.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
//! * `zmij` (enabled by default): formats floats with
//!   [`zmij`](https://docs.rs/zmij) when serializing (see `deser-json`).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

// These are generated from `deser-template-json`.
mod copy;
mod de;
mod parser;
mod raw;
mod scan;
mod stream;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice, from_str};
pub use self::raw::{Json5, RawJson5};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
#[cfg(feature = "io")]
pub use deser_json::to_writer;
pub use deser_json::{Indent, InlinePolicy, Serializer, SerializerConfig, Trailing, to_string};
