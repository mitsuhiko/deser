//! Parse and serialize JSON compatible with deser.
//!
//! This library is very bare bones at this point and not at all optimized.
mod de;
mod ser;

pub use self::de::{from_str, Deserializer};
pub use self::ser::{to_string, Serializer};
