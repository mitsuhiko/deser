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
//! [`deser::adapters::bytes`] and [`SerializerConfig::bytes`]).
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
//! are written on a single line in indented output.
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
//! follows.  The serializer never writes line breaks, so JSON Lines are
//! written by adding a newline after every value:
//!
//! ```rust
//! let mut out = String::new();
//! for value in [vec![1, 2], vec![3]] {
//!     out.push_str(&deser_json::to_string(&value).unwrap());
//!     out.push('\n');
//! }
//! assert_eq!(out, "[1,2]\n[3]\n");
//! ```
//!
//! By default this crate has no dependency crates other than `deser`, but optionally
//! the `speedups` feature can be enabled in which case the `ryu` and `itoa` crates are
//! used for number formatting and `simdutf8` is used to validate UTF-8 when parsing
//! byte slices.
mod buf;
mod de;
mod pretty;
mod scan;
mod ser;

pub use self::de::{Deserializer, DeserializerConfig, Iter, Trailing, from_slice, from_str};
pub use self::ser::{Indent, SerializerConfig, to_string};
