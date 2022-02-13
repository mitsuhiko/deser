//! Parse and serialize JSON compatible with deser.
//!
//! This library is very bare bones at this point and not at all optimized.  It is
//! based on microserde which in turn is based on miniserde to achieve the most
//! minimal implementation of a serializer and serializer.
//!
//! ```rust
//! let vec: Vec<u64> = deser_json::from_str("[1, 2, 3, 4]").unwrap();
//! let json = deser_json::to_string(&vec).unwrap();
//! ```
//!
//! By default this crate has no dependency crates other than `deser`, but optionally
//! the `speedups` feature can be enabled in which case this also uses `ryu` and `itoa`
//! crates are used for number formatting.
mod de;
mod ser;

pub use self::de::{from_str, Deserializer};
pub use self::ser::{to_string, Serializer};
