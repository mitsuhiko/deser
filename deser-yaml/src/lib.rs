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
//! | maps and sequences                      | block mappings and sequences, flow style (`[a, b]`, `{a: 1}`) if compact (see [`FlowPolicy`]), keys that are collections or long use `? key` |
//!
//! The style of individual values can be requested with hints: the
//! well-known [`Layout`](deser::hints::Layout) for collections (flow or
//! block) and [`ScalarStyle`](style::ScalarStyle) for strings (see
//! [`style`]).  Values set them with adapters, layers can set them for
//! instance by path.  Hints are preferences: a value is written in another
//! style if the requested one cannot represent it.  When reading, flow
//! collections are reported as compact so that they stay flow collections
//! through a [`Recording`](deser::de::Recording).
//!
//! Tags are written with [`Tagged`] or [`set_tag`] (see [`tag`]).  Streams of
//! multiple documents are written with a [`deser::io::Writer`] (see
//! [streams](#streams)).
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
//! # Streams
//!
//! Values are read from a [`Read`](std::io::Read) with [`from_reader`]
//! and written to a [`Write`](std::io::Write) with [`to_writer`].  To read
//! or write streams of documents the configurations are used with
//! [`deser::io`] (or an adapter for an async runtime such as
//! `deser-tokio`).  The reader only buffers until a document is complete:
//!
//! ```rust
//! use deser::io::{Reader, Writer};
//! use deser_yaml::{DeserializerConfig, SerializerConfig};
//!
//! const ENDED: SerializerConfig = SerializerConfig::new().end_documents(true);
//! let mut writer = Writer::new(Vec::new(), ENDED);
//! writer.write(&vec![1, 2]).unwrap();
//! writer.write(&"done").unwrap();
//! let output = writer.into_inner();
//! assert_eq!(output, b"- 1\n- 2\n...\n---\ndone\n...\n");
//!
//! let mut reader = Reader::new(&output[..], DeserializerConfig::new());
//! assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
//! assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("done"));
//! assert_eq!(reader.read::<String>().unwrap(), None);
//! ```
//!
//! # Features
//!
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
mod de;
mod emit;
mod event;
mod io;
mod parser;
mod quote;
mod resolve;
mod scanner;
mod ser;
pub mod style;
pub mod tag;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice, from_str};
pub use self::io::{StreamState, from_reader, to_writer};
pub use self::resolve::Version;
pub use self::ser::{
    FlowPolicy, Indent, MultilineStyle, NullStyle, QuoteStyle, SerializerConfig, to_string,
};
pub use self::tag::{Tagged, set_tag, take_tag};

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;
