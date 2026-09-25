//! Parse and serialize [CBOR](https://www.rfc-editor.org/rfc/rfc8949) compatible
//! with deser.
//!
//! ```rust
//! let bytes = deser_cbor::to_vec(&vec![1u64, 2, 3, 4]).unwrap();
//! assert_eq!(bytes, [0x84, 0x01, 0x02, 0x03, 0x04]);
//! let vec: Vec<u64> = deser_cbor::from_slice(&bytes).unwrap();
//! assert_eq!(vec, [1, 2, 3, 4]);
//! ```
//!
//! # Data Model
//!
//! The deser data model maps onto CBOR as follows:
//!
//! | deser                   | CBOR                                              |
//! |-------------------------|---------------------------------------------------|
//! | `Null`                  | `null` (`undefined` is also read as `Null`)       |
//! | `Bool`                  | `false` / `true`                                  |
//! | `U64`, `I64`            | unsigned and negative integers                    |
//! | `u128`, `i128`          | integers or bignums (tags 2 and 3)                |
//! | `F64`                   | half, single or double precision floats           |
//! | `Str`, `Char`           | text strings                                      |
//! | `Bytes`                 | byte strings                                      |
//! | maps and sequences      | maps and arrays                                   |
//! | [`Simple`]              | unassigned simple values                          |
//!
//! Serialization produces the preferred serialization of RFC 8949: the
//! shortest form is used for integers and lengths, floats are written in the
//! shortest form that preserves their value and all maps and arrays have a
//! definite length.  With [`to_canonical_vec`] map entries are additionally
//! sorted to produce a deterministic encoding.
//!
//! Deserialization accepts all well-formed CBOR including indefinite length
//! strings, arrays and maps.  Map keys can be of any type.  Integers that
//! do not fit into 64 bits are passed on as `u128` / `i128` extension atoms.
//!
//! # Tags
//!
//! Tags are not part of the data model.  Unknown tags are transparent: a
//! tagged value deserializes like the untagged value.  To read or write tags
//! use [`Tagged`] or see the [`tag`] module.
mod buf;
mod de;
mod float;
mod ser;
mod simple;
pub mod tag;

pub use self::de::{from_slice, Deserializer, Iter};
pub use self::ser::{to_canonical_vec, to_vec, Serializer};
pub use self::simple::Simple;
pub use self::tag::{take_tag, Tagged};
