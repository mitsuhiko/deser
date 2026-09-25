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
//! By default this crate has no dependency crates other than `deser`, but optionally
//! the `speedups` feature can be enabled in which case the `ryu` and `itoa` crates are
//! used for number formatting and `simdutf8` is used to validate UTF-8 when parsing
//! byte slices.
mod buf;
mod de;
mod scan;
mod ser;

pub use self::de::{from_slice, from_str, Deserializer};
pub use self::ser::{to_string, Serializer};
