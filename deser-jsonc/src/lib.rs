//! Parse JSONC (JSON with comments) compatible with deser.
//!
//! JSONC is JSON with `//` and `/* */` comments and commas after the last
//! element of sequences and maps, as used by configuration files such as
//! `tsconfig.json` or the settings of VS Code.  Otherwise this works like
//! [`deser-json`](https://docs.rs/deser-json): strings without escape
//! sequences are borrowed from the input and the positions of errors and
//! values refer to the input.  In [JSON Lines](Trailing::Newline) only
//! line breaks outside of comments end a value.
//!
//! ```rust
//! #[derive(deser::Deserialize)]
//! struct Config<'a> {
//!     name: &'a str,
//!     ports: Vec<u16>,
//! }
//!
//! let config: Config = deser_jsonc::from_str(r#"{
//!     // the name of the service
//!     "name": "api",
//!     /* the ports it listens on */
//!     "ports": [80, 443,],
//! }"#).unwrap();
//! assert_eq!(config.name, "api");
//! assert_eq!(config.ports, [80, 443]);
//! ```
//!
//! JSON is valid JSONC, so values are serialized as JSON (with the same
//! serializer as `deser-json`).
//!
//! # Raw Values
//!
//! [`RawJsonc`] holds the JSONC text of a value.  Like `deser_json::RawJson` it
//! keeps the text of values that are deserialized from JSONC as it is
//! (including comments), other values are encoded as JSON.  The serializer
//! of this crate writes the text of raw JSONC values as it is.  See
//! [`Raw`](deser_core::ext::Raw) for more information.
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
mod raw;
mod scan;
mod ser;
mod stream;
mod trailing;

pub use self::de::{
    Deserializer, DeserializerConfig, DeserializerConfigBuilder, Iter, from_slice, from_str,
};
pub use self::raw::{Jsonc, RawJsonc};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{
    Indent, InlinePolicy, Serializer, SerializerConfig, SerializerConfigBuilder, to_string,
};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
pub use self::trailing::Trailing;
