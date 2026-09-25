//! Parse YAML compatible with deser.
//!
//! ```rust
//! use deser::Deserialize;
//!
//! #[derive(Deserialize, Debug)]
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
//! ```
//!
//! **This crate is work in progress.**  It can deserialize but not yet
//! serialize.
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
//! | mappings and sequences                | maps and sequences                      |
//!
//! Which plain (unquoted) scalars are null, booleans or numbers depends on
//! the YAML version, see [`Version`].  Quoted scalars are always strings.
//! The standard tags (`!!str`, `!!int`, `!!float`, `!!bool`, `!!null`,
//! `!!binary`, `!!seq` and `!!map`) determine the type of a value, all other
//! tags are passed on out of band (see [`tag`]).  Map keys can be of any
//! type.
//!
//! Aliases are expanded: every alias produces the events of the node it
//! refers to (see [`Deserializer::alias_limit`]).
//!
//! # Documents
//!
//! A YAML stream can contain multiple documents.  [`from_str`] expects at
//! most one document, [`Deserializer`] can read them one by one:
//!
//! ```rust
//! let mut de = deser_yaml::Deserializer::new("--- a\n--- b\n");
//! let docs = de.iter::<String>().collect::<Result<Vec<_>, _>>().unwrap();
//! assert_eq!(docs, ["a", "b"]);
//! ```
//!
//! # Features
//!
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
//! * `locations`: enables [`Deserializer::track_locations`] which publishes
//!   source locations through [`deser_location`](https://docs.rs/deser-location).
mod de;
mod event;
mod parser;
mod resolve;
mod scanner;
pub mod tag;

pub use self::de::{from_slice, from_str, Deserializer, Iter};
pub use self::resolve::Version;
pub use self::tag::{take_tag, Tagged};

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;
