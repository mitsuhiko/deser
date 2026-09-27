//! Parse [JSON5](https://json5.org/) compatible with deser.
//!
//! JSON5 extends JSON with syntax from ECMAScript 5.1:
//!
//! * `//` and `/* */` comments,
//! * commas after the last element of sequences and maps,
//! * map keys which are identifiers (`{name: "api"}`),
//! * strings in single quotes, escaped line breaks within strings and the
//!   escapes `\'`, `\v`, `\0` and `\xFF`,
//! * hexadecimal numbers, numbers with a leading `+` or a leading or
//!   trailing decimal point, `Infinity` and `NaN`,
//! * more whitespace characters.
//!
//! Otherwise this works like [`deser-json`](https://docs.rs/deser-json):
//! strings without escape sequences (and identifiers) are borrowed from the
//! input and the positions of errors and values refer to the input.
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
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams.
//! * `speedups`: faster UTF-8 validation and serialization.

// These are generated from `deser-private-jsontemplate`.
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
