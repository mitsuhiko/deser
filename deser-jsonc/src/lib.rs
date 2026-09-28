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
//! JSON is valid JSONC, so values are serialized as JSON with the
//! serializer of `deser-json` (which is re-exported).
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams.  Requires
//!   `std`.
//! * `speedups`: faster UTF-8 validation and serialization.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

// These are generated from `deser-template-json`.
mod de;
#[cfg(feature = "io")]
mod io;
mod parser;
mod scan;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice, from_str};
#[cfg(feature = "io")]
pub use self::io::{StreamState, from_reader};
#[cfg(feature = "io")]
pub use deser_json::to_writer;
pub use deser_json::{Indent, InlinePolicy, Serializer, SerializerConfig, Trailing, to_string};
