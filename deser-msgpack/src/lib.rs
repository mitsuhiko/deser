//! Parse and serialize [MessagePack](https://msgpack.org/) compatible with
//! deser.
//!
//! ```rust
//! let bytes = deser_msgpack::to_vec(&vec![1u64, 2, 3, 4]).unwrap();
//! assert_eq!(bytes, [0x94, 0x01, 0x02, 0x03, 0x04]);
//! let vec: Vec<u64> = deser_msgpack::from_slice(&bytes).unwrap();
//! assert_eq!(vec, [1, 2, 3, 4]);
//! ```
//!
//! # Data Model
//!
//! The deser data model maps onto MessagePack as follows:
//!
//! | deser                   | MessagePack                                   |
//! |-------------------------|-----------------------------------------------|
//! | `Null`                  | nil                                           |
//! | `Bool`                  | false / true                                  |
//! | `U64`, `I64`            | integers                                      |
//! | `u128`, `i128`          | integers (if they fit into 64 bits)           |
//! | [`Timestamp`]           | the timestamp extension (type `-1`)           |
//! | `F32`                   | float 32                                      |
//! | `F64`                   | float 64                                      |
//! | `Str`, `Char`           | str                                           |
//! | `Bytes`                 | bin                                           |
//! | maps and sequences      | maps and arrays                               |
//! | [`Ext`]                 | other extensions                              |
//!
//! Integers and lengths are written in their shortest form.  Floats keep
//! their precision.  Other extension values (such as
//! [`Datetime`](deser_core::ext::Datetime) or [`Uuid`](deser_core::ext::Uuid))
//! are written as their fallback, which usually is a string.  Integers
//! which do not fit into 64 bits cannot be written.  With
//! [`SerializerConfig::canonical`] map entries are sorted to produce a
//! deterministic encoding.
//!
//! Deserialization accepts all well-formed MessagePack.  Map keys can be of
//! any type.  Signed integers that are not negative are passed on as
//! `U64`, negative integers as `I64`.  Extensions are passed on as
//! extension atoms: timestamps as [`Timestamp`], all others as [`Ext`] whose
//! fallback is the binary data.  Strings are validated as UTF-8.
//!
//! [`Timestamp`]: deser_core::ext::Timestamp
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams, see
//!   [streams](#streams).
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
//!
//! # Streams
//!
//! Items are read from a [`Read`](std::io::Read) with [`from_reader`] and
//! written to a [`Write`](std::io::Write) with [`to_writer`].  To read or
//! write items that follow each other (for instance on a socket) the
//! configurations are used with [`deser::io`](deser_core::io) (or an
//! adapter for an async runtime such as `deser-tokio`).  The reader only
//! buffers until an item is complete:
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! use deser::io::{Reader, Writer};
//! use deser_msgpack::{DeserializerConfig, SerializerConfig};
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
//! # }
//! ```
mod de;
mod ext;
mod head;
#[cfg(feature = "io")]
mod io;
mod parser;
mod ser;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice};
pub use self::ext::Ext;
#[cfg(feature = "io")]
pub use self::io::{StreamState, from_reader, to_writer};
pub use self::ser::{Serializer, SerializerConfig, to_vec};
