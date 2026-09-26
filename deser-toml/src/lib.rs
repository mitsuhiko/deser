//! Parse and serialize [TOML](https://toml.io/en/v1.1.0) compatible with
//! deser.
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Deserialize, Serialize, Debug)]
//! struct Config {
//!     name: String,
//!     ports: Vec<u16>,
//! }
//!
//! let config: Config = deser_toml::from_str(r#"
//! name = "web"
//! ports = [80, 443]
//! "#).unwrap();
//! assert_eq!(config.name, "web");
//! assert_eq!(config.ports, [80, 443]);
//!
//! let toml = deser_toml::to_string(&config).unwrap();
//! assert_eq!(toml, "name = \"web\"\nports = [80, 443]\n");
//! ```
//!
//! # Data Model
//!
//! The parser implements TOML 1.1 and passes the
//! [toml-test](https://github.com/toml-lang/toml-test) suite.  TOML maps
//! onto the deser data model as follows:
//!
//! | TOML                                  | deser                                   |
//! |---------------------------------------|-----------------------------------------|
//! | tables (including the document)       | maps                                    |
//! | arrays (including arrays of tables)   | sequences                               |
//! | strings                               | `Str`                                   |
//! | integers                              | `U64`, `I64`                            |
//! | floats                                | `F64`                                   |
//! | booleans                              | `Bool`                                  |
//! | date-times, dates and times           | [`Datetime`]                            |
//!
//! Date-times are passed through deser as the well-known
//! [`Datetime`] extension type which falls back to a
//! string for types that do not know it.  With the respective features of
//! deser, the date and time types of `jiff`, `chrono` and `time` can be
//! used directly (this example requires the `jiff` feature of deser):
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Deserialize, Serialize)]
//! struct Event {
//!     start: jiff::Timestamp,
//!     day: jiff::civil::Date,
//! }
//!
//! let event: Event = deser_toml::from_str("
//! start = 2024-06-19 15:22:45-04:00
//! day = 2024-06-19
//! ").unwrap();
//! assert_eq!(event.start.to_string(), "2024-06-19T19:22:45Z");
//!
//! let toml = deser_toml::to_string(&event).unwrap();
//! assert_eq!(toml, "start = 2024-06-19T19:22:45Z\nday = 2024-06-19\n");
//! ```
//!
//! The document is always a table.  Keys are emitted in the order in which
//! they are defined in the document.
//!
//! Integers are supported in the range of `i64` and `u64`, integers that
//! do not fit are an error.  Floats that overflow to infinity are an error
//! as well.  Newlines in multi-line strings are normalized to `\n`.  A
//! UTF-8 byte order mark at the start of the document is ignored.
//!
//! When serializing, maps are written as tables and sequences of maps as
//! arrays of tables.  The well-known [`Timestamp`](deser::ext::Timestamp)
//! type is written as offset date-time in UTC, other well-known types
//! (such as UUIDs and decimals) are written as strings.  TOML has no null value: map entries with null values
//! are skipped and null values in sequences are an error.  TOML has no
//! bytes either, they are written as base64 strings by default (see
//! [`deser::adapters::bytes`]).  See [`SerializerConfig`] for more
//! information.
//!
//! # Features
//!
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
mod datetime;
mod de;
mod document;
mod parser;
mod ser;

pub use self::de::{Deserializer, DeserializerConfig, from_slice, from_str};
pub use self::ser::{SerializerConfig, to_string};
/// Re-exported from [`deser::ext`] for convenience.
pub use deser::ext::{Date, Datetime, Offset, Time};
