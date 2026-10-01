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
//! | `F32`, `F64`            | half, single or double precision floats           |
//! | `Str`, `Char`           | text strings                                      |
//! | `Bytes`                 | byte strings                                      |
//! | maps and sequences      | maps and arrays                                   |
//! | [`Simple`]              | unassigned simple values                          |
//!
//! Offset date-times are written with tag 0, local dates with tag 1004 and
//! other date-times as plain text strings.  Timestamps are written with tag
//! 1 unless they have a fraction of a second, then they are written as
//! date/time string (tag 0) which retains the precision.  Other extension
//! values (such as [`Duration`](deser_core::ext::Duration)) are written as their
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
//! numbers, [`Timestamp`] accepts them.  All floats are read as `F64`: RFC
//! 8949 does not distinguish the precisions in the data model, the shortest
//! one that preserves the value is picked when writing.
//!
//! [`BigInt`]: deser_core::ext::BigInt
//! [`Datetime`]: deser_core::ext::Datetime
//! [`Timestamp`]: deser_core::ext::Timestamp
//! [`Uuid`]: deser_core::ext::Uuid
//! [`Decimal`]: deser_core::ext::Decimal
//!
//! # Raw Values
//!
//! [`RawCbor`] holds the CBOR encoding of a value.  The encoding of values
//! that are deserialized from CBOR is kept as it is (including tags and
//! how lengths and integers were encoded): it's validated but not
//! deserialized, and written out again unchanged unless canonical output is
//! requested.  Values of other formats are encoded as CBOR.
//!
//! ```rust
//! use deser_cbor::RawCbor;
//!
//! #[derive(deser::Deserialize, deser::Serialize)]
//! struct Record {
//!     id: u32,
//!     payload: RawCbor<'static>,
//! }
//!
//! // {"id": 1, "payload": [_ 1, 2]}
//! let input = [
//!     0xa2, 0x62, b'i', b'd', 0x01, 0x67, b'p', b'a', b'y', b'l', b'o',
//!     b'a', b'd', 0x9f, 0x01, 0x02, 0xff,
//! ];
//! let record: Record = deser_cbor::from_slice(&input).unwrap();
//! assert_eq!(record.payload.as_bytes(), [0x9f, 0x01, 0x02, 0xff]);
//! assert_eq!(record.payload.deserialize::<Vec<u32>>().unwrap(), [1, 2]);
//! assert_eq!(deser_cbor::to_vec(&record).unwrap(), input);
//! ```
//!
//! See [`Raw`](deser_core::ext::Raw) for more information.
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
//! Data items are read from a [`Read`](std::io::Read) with [`from_reader`]
//! and written to a [`Write`](std::io::Write) with [`to_writer`].  To read
//! or write [CBOR sequences](https://www.rfc-editor.org/rfc/rfc8742) (data
//! items that follow each other, for instance on a socket) the
//! configurations create readers and writers of
//! [`deser::io`](deser_core::io) ([`DeserializerConfig::reader`] and
//! [`SerializerConfig::writer`]).  The reader only buffers until an item
//! is complete:
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! use deser_cbor::{DeserializerConfig, SerializerConfig};
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
//!
//! # Tags
//!
//! Tags are not part of the deser data model.  Instead they are exchanged
//! out of band through the [`State`](deser_core::State):
//!
//! * When deserializing, the tags in front of a data item are published into
//!   the state for the first event of the item (the atom or the start of the
//!   map or sequence).  Types can pick them up with
//!   [`take_tag`].  Types which do not care about tags never see them, which
//!   means that unknown tags are transparent.
//! * When serializing, [`push_tag`] registers a tag of a value in the state
//!   and the serializer writes it in front of the data item.
//! * [`Tagged`] captures the outermost tag of a value and writes it.
//!
//! Both directions use the same event data.  This means that values which
//! capture event data (such as [`Recording`](deser_core::de::Recording)) keep
//! the tags: CBOR that is deserialized into a recording and serialized again
//! retains its tags.
//!
//! The simplest way to work with tags is the [`Tagged`] wrapper.
//!
//! The bignum tags 2 and 3 are handled by the format itself: they are
//! converted to and from integers (and [`BigInt`](deser_core::ext::BigInt) for
//! bignums that do not fit into 128 bits).  The same applies to the tags of
//! the well-known types: date/time strings (tag 0), decimal fractions (tag
//! 4), UUIDs (tag 37) and full-date strings (tag 1004) are converted to and
//! from the respective [well-known types](deser_core::ext) if their content is
//! valid.  Otherwise they are passed on as tagged values.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod copy;
mod de;
mod float;
mod parser;
mod raw;
mod ser;
mod simple;
mod stream;
mod tag;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice};
pub use self::raw::{Cbor, RawCbor};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Serializer, SerializerConfig, to_vec};
pub use self::simple::Simple;
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
pub use self::tag::{Tagged, push_tag, take_tag};
