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
//! By default this crate has no dependency crates other than `deser`, but optionally
//! the `speedups` feature can be enabled in which case the `ryu` and `itoa` crates are
//! used for number formatting and `simdutf8` is used to validate UTF-8 when parsing
//! byte slices.
mod buf;
mod de;
mod scan;
mod ser;

pub use self::de::{from_slice, from_str, Deserializer, DeserializerConfig};
pub use self::ser::{to_string, SerializerConfig};
