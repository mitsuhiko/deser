//! Parse and write YAML compatible with deser.
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
//! let config: Config = deser_yaml::from_str("
//! name: web
//! ports: [80, 443]
//! ").unwrap();
//! assert_eq!(config.name, "web");
//! assert_eq!(config.ports, [80, 443]);
//!
//! assert_eq!(
//!     deser_yaml::to_string(&config).unwrap(),
//!     "name: web\nports:\n  - 80\n  - 443\n"
//! );
//! ```
//!
//! # Data Model
//!
//! The YAML parser passes the complete official
//! [YAML test suite](https://github.com/yaml/yaml-test-suite).  YAML maps
//! onto the deser data model as follows:
//!
//! | YAML                                  | deser                                   |
//! |---------------------------------------|-----------------------------------------|
//! | `null`, `~`, empty                    | `Null`                                  |
//! | `true`, `false`                       | `Bool`                                  |
//! | integers                              | `U64`, `I64` (`u128` / `i128` if wider) |
//! | floats                                | `F64`                                   |
//! | other scalars                         | `Str`                                   |
//! | `!!binary`                            | `Bytes`                                 |
//! | `!!timestamp`                         | [`Datetime`](deser::ext::Datetime)      |
//! | mappings and sequences                | maps and sequences                      |
//!
//! Which plain (unquoted) scalars are null, booleans or numbers depends on
//! the YAML version, see [`Version`].  Quoted scalars are always strings.
//! The standard tags (`!!str`, `!!int`, `!!float`, `!!bool`, `!!null`,
//! `!!binary`, `!!timestamp`, `!!seq` and `!!map`) determine the type of a
//! value, all other tags are passed on out of band (see [`tag`]).
//! Timestamps are only recognized with an explicit `!!timestamp` tag.  They
//! are passed on as the well-known [`Datetime`](deser::ext::Datetime) type
//! (a date or an offset date-time, timestamps without time zone are in UTC)
//! which falls back to a string.  Map keys can be of any
//! type.
//!
//! Aliases are expanded: every alias produces the events of the node it
//! refers to (see [`DeserializerConfig::alias_limit`]).
//!
//! # Serialization
//!
//! [`to_string`] writes values as block collections: sequences with `-`,
//! mappings with `key: value`, empty collections as `{}` and `[]`.  How the
//! output looks can be configured with [`SerializerConfig`] (indentation,
//! quoting, null, bytes, ...).  Values are always written so that they read
//! back as the same values, also by readers of YAML 1.1 (such as PyYAML)
//! unless configured otherwise with [`SerializerConfig::compat`]:
//!
//! | deser                                   | YAML                                          |
//! |-----------------------------------------|-----------------------------------------------|
//! | `Null`                                  | `null` (see [`NullStyle`])                    |
//! | `Bool`, integers                        | `true`, `false`, `42`                         |
//! | `F64`                                   | `1.5`, `1.0e+20`, `.inf`, `.nan`               |
//! | `Str`                                   | plain if possible, otherwise quoted (see [`QuoteStyle`]), with line breaks as literal block scalar (see [`MultilineStyle`]) |
//! | `Bytes`                                 | `!!binary` (see [`SerializerConfig::binary`]) |
//! | [`Datetime`](deser::ext::Datetime)      | timestamp (see [`SerializerConfig::timestamp_tag`]) |
//! | maps and sequences                      | block mappings and sequences, keys that are collections or long use `? key` |
//!
//! Tags are written with [`Tagged`] or [`set_tag`] (see [`tag`]).  Streams of
//! multiple documents are written with [`Serializer`].
//!
//! # Documents
//!
//! A YAML stream can contain multiple documents.  [`from_str`] expects at
//! most one document, [`Deserializer`] can read them one by one:
//!
//! ```rust
//! let mut de = deser_yaml::Deserializer::from_str("--- a\n--- b\n");
//! let docs = de.iter::<String>().collect::<Result<Vec<_>, _>>().unwrap();
//! assert_eq!(docs, ["a", "b"]);
//! ```
//!
//! # Features
//!
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
mod de;
mod emit;
mod event;
mod parser;
mod quote;
mod resolve;
mod scanner;
mod ser;
pub mod tag;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice, from_str};
pub use self::resolve::Version;
pub use self::ser::{
    MultilineStyle, NullStyle, QuoteStyle, Serializer, SerializerConfig, to_string,
};
pub use self::tag::{Tagged, set_tag, take_tag};

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;
