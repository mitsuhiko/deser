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
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library, see [streams](#streams).  Requires `std`.
//! * `speedups`: validates UTF-8 with [`simdutf8`](https://docs.rs/simdutf8).
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
//!
//! # Streams
//!
//! Items are read from a [`Read`](std::io::Read) with [`from_reader`] and
//! written to a [`Write`](std::io::Write) with [`to_writer`].  To read or
//! write items that follow each other (for instance on a socket) the
//! configurations create readers and writers of
//! [`deser::io`](deser_core::io) ([`DeserializerConfig::reader`] and
//! [`SerializerConfig::writer`]).  The reader only buffers until an item is
//! complete:
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! use deser_msgpack::{DeserializerConfig, SerializerConfig};
//!
//! let mut writer = SerializerConfig::new().writer(Vec::new());
//! writer.write(&vec![1u32, 2]).unwrap();
//! writer.write(&"three").unwrap();
//! let bytes = writer.into_inner();
//!
//! let mut reader = DeserializerConfig::new().reader(&bytes[..]);
//! assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
//! assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("three"));
//! assert_eq!(reader.read::<String>().unwrap(), None);
//! # }
//! ```
//!
//! The stream serializer ([`Serializer`]) and the stream deserializer
//! ([`StreamDeserializer`]) do not do IO themselves (see
//! [`deser::stream`](deser_core::stream)), they also work with other kinds
//! of IO (for instance async runtimes with `deser-tokio`) and without the
//! standard library.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod de;
mod ext;
mod head;
mod parser;
mod ser;
mod stream;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice};
pub use self::ext::Ext;
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Serializer, SerializerConfig, to_vec};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
