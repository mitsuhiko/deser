//! Parse and serialize JSON compatible with deser.
//!
//! This library is very bare bones at this point and not at all optimized.  It is
//! based on microserde which in turn is based on miniserde to achieve the most
//! minimal implementation of a serializer and serializer.
//!
//! ```rust
//! let vec: Vec<u64> = deser_json::from_str("[1, 2, 3, 4]").unwrap();
//! let json = deser_json::to_string(&vec).unwrap();
//! assert_eq!(json, "[1,2,3,4]");
//! ```
//!
//! Besides strings, JSON can also be parsed from byte slices with
//! [`from_slice`].  In that case the input is validated as UTF-8 while it is
//! parsed rather than upfront:
//!
//! ```rust
//! let vec: Vec<String> = deser_json::from_slice(b"[\"a\", \"b\"]").unwrap();
//! assert_eq!(vec, ["a", "b"]);
//! assert!(deser_json::from_slice::<String>(b"\"\xff\"").is_err());
//! ```
//!
//! Integers that do not fit into 64 bits as well as the well-known
//! [`BigInt`](deser::ext::BigInt), [`Decimal`](deser::ext::Decimal) and
//! [`Number`](deser::ext::Number) types are written as JSON numbers.  Other
//! well-known types (such as date-times and UUIDs) are written as strings.
//! JSON has no bytes, they are written as base64 strings by default and
//! types that expect bytes accept strings and arrays of integers (see
//! [`deser::adapters::bytes`] and [`SerializerConfig::bytes`]).  Floats
//! are written with the shortest text that reads back as the same value of
//! their precision (`0.1f32` as `0.1`, not `0.10000000149011612`), floats
//! that are infinite or NaN as `null`.
//!
//! When parsing, floats whose text cannot be recovered from their value as
//! `f64` (like `0.10` or `1e5`) and integers that do not fit into 128 bits
//! are passed on as [`Number`](deser::ext::Number) which carries the text of
//! the number and its value as `f64`.  Types like `f64` get the value, types
//! which deserialize decimal numbers exactly (like
//! [`Decimal`](deser::ext::Decimal)) use the text.  See
//! [`DeserializerConfig::exact_numbers`].
//!
//! Strings without escape sequences are borrowed from the input, so types
//! can borrow them:
//!
//! ```rust
//! #[derive(deser::Deserialize)]
//! struct User<'a> {
//!     name: &'a str,
//! }
//!
//! let user: User = deser_json::from_str(r#"{"name": "Peter"}"#).unwrap();
//! assert_eq!(user.name, "Peter");
//! ```
//!
//! # Pretty Printing
//!
//! By default the output is compact.  [`SerializerConfig::pretty`] indents
//! it and writes spaces after separators:
//!
//! ```rust
//! use deser_json::{Indent, SerializerConfig};
//!
//! const PRETTY: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(2));
//! let json = PRETTY.to_string(&vec![vec![1, 2]]).unwrap();
//! assert_eq!(json, "[\n  [\n    1,\n    2\n  ]\n]");
//! ```
//!
//! Indentation and spaces can also be configured on their own with
//! [`SerializerConfig::indent`] and [`SerializerConfig::compact`].  Maps
//! and sequences with the [`Layout::Compact`](deser::hints::Layout) hint
//! are written on a single line in indented output, short ones that only
//! contain scalars can be too (see [`SerializerConfig::inline`]).
//!
//! # JSON Lines
//!
//! By default only whitespace may follow a value.  What may follow is
//! controlled by [`DeserializerConfig::trailing`] (see [`Trailing`]).  With
//! [`Trailing::Newline`] a [`Deserializer`] reads
//! [JSON Lines](https://jsonlines.org/) (also known as NDJSON) one by one:
//!
//! ```rust
//! use deser_json::{Deserializer, DeserializerConfig, Trailing};
//!
//! let config = DeserializerConfig::new().trailing(Trailing::Newline);
//! let mut de = Deserializer::from_str_with_config("[1, 2]\n[3]\n", &config);
//! let lines = de.iter::<Vec<u32>>().collect::<Result<Vec<_>, _>>().unwrap();
//! assert_eq!(lines, [vec![1, 2], vec![3]]);
//! ```
//!
//! Errors only discard their line, so the remaining lines can still be
//! read.  [`Trailing::Stop`] stops after the value without looking at what
//! follows.  JSON Lines are written with a [`Serializer`] and
//! [`SerializerConfig::trailing`]:
//!
//! ```rust
//! use deser_json::{Serializer, SerializerConfig, Trailing};
//!
//! const LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);
//! let mut serializer = Serializer::with_config(&LINES);
//! for value in [vec![1, 2], vec![3]] {
//!     serializer.serialize(&value).unwrap();
//! }
//! assert_eq!(serializer.finish(), "[1,2]\n[3]\n");
//! ```
//!
//! # Streams
//!
//! Values are read from a [`Read`](std::io::Read) with [`from_reader`]
//! and written to a [`Write`](std::io::Write) with [`to_writer`].  To read
//! or write more than one value (for instance JSON Lines from a socket) the
//! configurations are used with [`deser::io`] (or an adapter for an async
//! runtime such as `deser-tokio`): [`DeserializerConfig`] splits streams
//! into values and [`SerializerConfig`] writes them.  How values are
//! separated depends on the `trailing` setting of the configurations (see
//! [`Trailing`]).  The reader only buffers until a value is complete:
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! use std::collections::BTreeMap;
//! use deser::io::{Reader, Writer};
//! use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
//!
//! const READ_LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
//! const WRITE_LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);
//!
//! let input = &b"{\"id\": 1}\n{\"id\": 2}\n"[..];
//! let mut reader = Reader::new(input, READ_LINES);
//! let mut writer = Writer::new(Vec::new(), WRITE_LINES);
//! while let Some(value) = reader.read::<BTreeMap<String, u32>>().unwrap() {
//!     writer.write(&value).unwrap();
//! }
//! assert_eq!(writer.into_inner(), b"{\"id\":1}\n{\"id\":2}\n");
//! # }
//! ```
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams, see
//!   [streams](#streams).
//! * `speedups`: uses the `ryu` and `itoa` crates for number formatting and
//!   `simdutf8` to validate UTF-8 when parsing byte slices.  Otherwise this
//!   crate has no dependencies other than `deser`.
mod buf;
mod de;
#[cfg(feature = "io")]
mod io;
mod parser;
mod pretty;
mod scan;
mod ser;

pub use self::de::{Deserializer, DeserializerConfig, Iter, Trailing, from_slice, from_str};
#[cfg(feature = "io")]
pub use self::io::{StreamState, from_reader, to_writer};
pub use self::ser::{Indent, InlinePolicy, Serializer, SerializerConfig, to_string};
