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
//! | [`BigInt`]              | bignums (tags 2 and 3)                            |
//! | [`Datetime`]            | date/time (tag 0), full-date (tag 1004) or text   |
//! | [`Timestamp`]           | epoch date/time (tag 1) or date/time (tag 0)      |
//! | [`Uuid`]                | UUIDs (tag 37)                                    |
//! | [`Decimal`]             | decimal fractions (tag 4)                         |
//! | `F64`                   | half, single or double precision floats           |
//! | `Str`, `Char`           | text strings                                      |
//! | `Bytes`                 | byte strings                                      |
//! | maps and sequences      | maps and arrays                                   |
//! | [`Simple`]              | unassigned simple values                          |
//!
//! Offset date-times are written with tag 0, local dates with tag 1004 and
//! other date-times as plain text strings.  Timestamps are written with tag
//! 1 unless they have a fraction of a second, then they are written as
//! date/time string (tag 0) which retains the precision.  Other extension
//! values (such as [`Duration`](deser::ext::Duration)) are written as their
//! fallback.
//!
//! Serialization produces the preferred serialization of RFC 8949: the
//! shortest form is used for integers and lengths, floats are written in the
//! shortest form that preserves their value and all maps and arrays have a
//! definite length.  With [`SerializerConfig::canonical`] map entries are
//! additionally sorted to produce a deterministic encoding.
//!
//! Deserialization accepts all well-formed CBOR including indefinite length
//! strings, arrays and maps.  Map keys can be of any type.  Integers that
//! do not fit into 64 bits are passed on as `u128` / `i128` extension atoms
//! and larger ones as [`BigInt`].  The tags 0, 4, 37 and 1004 are turned
//! into the well-known types [`Datetime`], [`Decimal`] and [`Uuid`] if their
//! content is valid.  Epoch based date/times (tag 1) are passed on as tagged
//! numbers, [`Timestamp`] accepts them.
//!
//! [`BigInt`]: deser::ext::BigInt
//! [`Datetime`]: deser::ext::Datetime
//! [`Timestamp`]: deser::ext::Timestamp
//! [`Uuid`]: deser::ext::Uuid
//! [`Decimal`]: deser::ext::Decimal
//!
//! # Features
//!
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
//!
//! # Streams
//!
//! Data items are read from a [`Read`](std::io::Read) with [`from_reader`]
//! and written to a [`Write`](std::io::Write) with [`to_writer`].  To read
//! or write [CBOR sequences](https://www.rfc-editor.org/rfc/rfc8742) (data
//! items that follow each other, for instance on a socket) the
//! configurations are used with [`deser::io`] (or an adapter for an async
//! runtime such as `deser-tokio`).  The reader only buffers until an item
//! is complete:
//!
//! ```rust
//! use deser::io::{Reader, Writer};
//! use deser_cbor::{DeserializerConfig, SerializerConfig};
//!
//! let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
//! writer.write(&vec![1u32, 2]).unwrap();
//! writer.write(&"three").unwrap();
//! let bytes = writer.into_inner();
//!
//! let mut reader = Reader::new(&bytes[..], DeserializerConfig::new());
//! assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
//! assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("three"));
//! assert_eq!(reader.read::<String>().unwrap(), None);
//! ```
//!
//! # Tags
//!
//! Tags are not part of the data model.  Unknown tags are transparent: a
//! tagged value deserializes like the untagged value.  To read or write tags
//! use [`Tagged`] or see the [`tag`] module.
mod buf;
mod de;
mod float;
mod io;
mod ser;
mod simple;
pub mod tag;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice};
pub use self::io::{StreamState, from_reader, to_writer};
pub use self::ser::{SerializerConfig, to_vec};
pub use self::simple::Simple;
pub use self::tag::{Tagged, take_tag};
