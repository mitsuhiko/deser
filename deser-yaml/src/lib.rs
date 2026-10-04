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
//! | `!!timestamp`                         | [`Datetime`](deser_core::ext::Datetime)      |
//! | mappings and sequences                | maps and sequences                      |
//!
//! Which plain (unquoted) scalars are null, booleans or numbers depends on
//! the YAML version, see [`Version`].  As their type is inferred from
//! their text, they are passed on as
//! [`Implicit`](deser_core::Atom::Implicit) atoms: types that expect strings
//! receive the text, all others the value.  `version: 1.10` is `1.1` for an
//! `f64` and `"1.10"` for a `String`, `~` is `None` for an `Option<String>`
//! and `"~"` for a `String`, and keys like `200` work for maps with string
//! keys.  Quoted scalars are always strings and scalars with a standard tag
//! (like `!!int 42`) are always of the type of their tag.
//!
//! ```rust
//! #[derive(deser::Deserialize)]
//! struct Package {
//!     version: String,
//!     port: u16,
//! }
//!
//! let package: Package =
//!     deser_yaml::from_str("version: 1.10\nport: 0x1F").unwrap();
//! assert_eq!(package.version, "1.10");
//! assert_eq!(package.port, 31);
//! ```
//! The standard tags (`!!str`, `!!int`, `!!float`, `!!bool`, `!!null`,
//! `!!binary`, `!!timestamp`, `!!seq` and `!!map`) determine the type of a
//! value, all other tags are passed on out of band (see [Tags](#tags)).
//! Timestamps are only recognized with an explicit `!!timestamp` tag.  They
//! are passed on as the well-known [`Datetime`](deser_core::ext::Datetime) type
//! (a date or an offset date-time, timestamps without time zone are in UTC)
//! which falls back to a string.  Map keys can be of any
//! type.
//!
//! Aliases are expanded: every alias produces the events of the node it
//! refers to (see [`DeserializerConfig::set_alias_limit`]).
//!
//! # Serialization
//!
//! [`to_string`] writes values as block collections: sequences with `-`,
//! mappings with `key: value`, empty collections as `{}` and `[]`.  How the
//! output looks can be configured with [`SerializerConfig`] (indentation,
//! quoting, null, bytes, ...).  Values are always written so that they read
//! back as the same values, also by readers of YAML 1.1 (such as PyYAML)
//! unless configured otherwise with [`SerializerConfig::set_compat`]:
//!
//! | deser                                   | YAML                                          |
//! |-----------------------------------------|-----------------------------------------------|
//! | `Null`                                  | `null` (see [`NullStyle`])                    |
//! | `Bool`, integers                        | `true`, `false`, `42`                         |
//! | `F32`, `F64`                            | `1.5`, `1.0e+20`, `.inf`, `.nan` (the shortest text for the precision) |
//! | `Str`                                   | plain if possible, otherwise quoted (see [`QuoteStyle`]), with line breaks as literal block scalar (see [`MultilineStyle`]) |
//! | [`Implicit`](deser_core::Atom::Implicit) | its text if readers read it as the same value (`1.10`, `0x1F`, `~`), otherwise the value |
//! | `Bytes`                                 | `!!binary` (see [`SerializerConfig::set_binary`]) |
//! | [`Datetime`](deser_core::ext::Datetime)      | timestamp (see [`SerializerConfig::set_timestamp_tag`]) |
//! | maps and sequences                      | block mappings and sequences, flow style (`[a, b]`, `{a: 1}`) if compact (see [`FlowPolicy`]), keys that are collections or long use `? key` |
//!
//! The style of individual values can be requested with hints: the
//! well-known [`Layout`](deser_core::hints::Layout) for collections (flow or
//! block) and [`ScalarStyle`](style::ScalarStyle) for strings (see
//! [`style`]).  Values set them with adapters, layers can set them for
//! instance by path.  Hints are preferences: a value is written in another
//! style if the requested one cannot represent it.  When reading, flow
//! collections are reported as compact so that they stay flow collections
//! through a [`Recording`](deser_core::de::Recording).  Plain scalars keep
//! their text the same way, `version: 1.10` read into a `deser_value::Value` or a
//! recording is written as `version: 1.10` again.
//!
//! Tags are written with [`Tagged`] or [`set_tag`] (see [Tags](#tags)).  Streams of
//! multiple documents are written with [`Serializer`].
//!
//! # Tags
//!
//! Tags are not part of the deser data model.  The standard tags determine
//! the type of a value (see [Data Model](#data-model)), all
//! other tags are exchanged out of band through the
//! [`State`](deser_core::State):
//!
//! * When deserializing, the tag of a node is published into the state for
//!   the first event of the node (the atom or the start of the map or
//!   sequence).  Types can pick it up with [`take_tag`].
//!   Types which do not care about tags never see them, which means that
//!   unknown tags are transparent: the value of `!color red` is the string
//!   `red`.
//! * When serializing, [`set_tag`] registers the tag of a value in the state
//!   and the serializer writes it in front of the node.
//! * [`Tagged`] captures the tag of a value and writes it.
//!
//! Both directions use the same event data, so values which capture event
//! data (such as [`Recording`](deser_core::de::Recording)) keep the tags.
//!
//! Tags are reported fully resolved: `!foo` stays `!foo` but `!!set`
//! becomes `tag:yaml.org,2002:set` and tag handles declared with `%TAG`
//! directives are expanded.
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
//! or write streams of documents the configurations create readers and
//! writers of [`deser::io`](deser_core::io) ([`DeserializerConfig::reader`]
//! and [`SerializerConfig::writer`]).  The reader only buffers until a
//! document is complete:
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! use deser_yaml::{DeserializerConfig, SerializerConfig};
//!
//! const ENDED: SerializerConfig =
//!     SerializerConfig::builder().end_documents(true).build();
//! let mut writer = ENDED.writer(Vec::new());
//! writer.write(&vec![1, 2]).unwrap();
//! writer.write(&"done").unwrap();
//! let output = writer.into_inner();
//! assert_eq!(output, b"- 1\n- 2\n...\n---\ndone\n...\n");
//!
//! let mut reader = DeserializerConfig::new().reader(&output[..]);
//! assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
//! assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("done"));
//! assert_eq!(reader.read::<String>().unwrap(), None);
//! # }
//! ```
//!
//! The stream serializer ([`Serializer`]) and the stream deserializer
//! ([`StreamDeserializer`]) do not do IO themselves (see
//! [`deser::stream`](deser_core::stream)), they also work with other kinds
//! of IO (for instance async runtimes with `deser-tokio`).
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library, see [streams](#streams).
//! * `speedups` (enabled by default): validates UTF-8 with
//!   [`simdutf8`](https://docs.rs/simdutf8).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

mod copy;
mod de;
mod emit;
mod event;
mod num;
mod parser;
mod quote;
mod resolve;
mod scanner;
mod ser;
mod stream;
pub mod style;
mod tag;

pub use self::de::{
    Deserializer, DeserializerConfig, DeserializerConfigBuilder, Iter, from_slice, from_str,
};
pub use self::resolve::Version;
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{
    FlowPolicy, Indent, MultilineStyle, NullStyle, QuoteStyle, Serializer, SerializerConfig,
    SerializerConfigBuilder, to_string,
};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
pub use self::tag::{Tagged, set_tag, take_tag};

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;
