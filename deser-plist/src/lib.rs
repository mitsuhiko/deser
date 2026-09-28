//! Parse and serialize [property lists](https://en.wikipedia.org/wiki/Property_list)
//! compatible with deser.
//!
//! Property lists are the configuration and serialization format of
//! Apple's platforms (`Info.plist`, preferences, Xcode projects, keyed
//! archives, ...).  They come in three formats which are all supported:
//! XML, binary and the older OpenStep (ASCII) format.  Deserialization
//! detects the format, the serializer writes the format of its
//! [`SerializerConfig`] (XML by default).
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//! use deser_plist::{Format, SerializerConfig};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! #[deser(rename_all = "PascalCase")]
//! struct Info {
//!     bundle_name: String,
//!     bundle_version: u32,
//! }
//!
//! let info = Info { bundle_name: "Demo".into(), bundle_version: 42 };
//!
//! let xml = deser_plist::to_string(&info).unwrap();
//! assert!(xml.contains("<key>BundleName</key>\n\t<string>Demo</string>"));
//! assert_eq!(deser_plist::from_slice::<Info>(xml.as_bytes()).unwrap(), info);
//!
//! let binary = SerializerConfig::new().format(Format::Binary).to_vec(&info).unwrap();
//! assert_eq!(deser_plist::from_slice::<Info>(&binary).unwrap(), info);
//! ```
//!
//! # Data Model
//!
//! Property lists map onto the deser data model as follows:
//!
//! | Property list               | deser                                    |
//! |-----------------------------|------------------------------------------|
//! | dictionaries                | maps (keys are lexical atoms)            |
//! | arrays, sets                | sequences                                |
//! | strings                     | `Str` (OpenStep: lexical atoms)          |
//! | integers                    | `U64`, `I64`, `i128`                     |
//! | reals                       | `F64`                                    |
//! | booleans                    | `Bool`                                   |
//! | dates                       | [`Timestamp`]                            |
//! | data                        | `Bytes`                                  |
//! | UIDs                        | [`Uid`]                                  |
//!
//! Dates are passed through deser as the well-known [`Timestamp`] type,
//! so `std::time::SystemTime` and the timestamp types of `jiff`, `chrono`
//! and `time` work with the respective features of deser.  Binary
//! property lists store dates as `f64` seconds, they are rounded to
//! microseconds when read.  XML property lists store dates without
//! fraction, it's truncated when written.
//!
//! UIDs (references of `NSKeyedArchiver` archives) are passed through as
//! the [`Uid`] extension type which falls back to an integer.  Binary
//! property lists have a type for them, the text formats write them as
//! dictionaries with a single `CF$UID` key.  Like Core Foundation, such
//! dictionaries are read back as UIDs from XML.
//!
//! Keys of dictionaries are passed on as lexical atoms, so maps with keys
//! that are not strings (such as `BTreeMap<u32, _>`) work.  The OpenStep
//! format only knows strings: all of its strings are lexical atoms which
//! can be deserialized into numbers and booleans (`YES` and `NO`).  Like
//! Core Foundation the reader also understands `.strings` files, which are
//! a dictionary without braces.
//!
//! When serializing, property lists have no null value: map entries with
//! null values (such as `None`) are skipped, null values elsewhere are an
//! error.  Bytes are always written as data.  Offset date-times
//! ([`Datetime`](deser_core::ext::Datetime)) are dates, other extension
//! types (such as UUIDs and decimals) are written as their fallback,
//! usually a string.  Keys have to be strings, numbers and booleans are
//! converted into strings.  Integers can be of the range of `i128` in
//! binary property lists, `i64` and `u64` in XML.  In the OpenStep format
//! numbers and dates are written as strings and booleans as `YES` and
//! `NO`.
//!
//! [`Timestamp`]: deser_core::ext::Timestamp
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams with
//!   [`from_reader`] and [`to_writer`] and the configurations with
//!   [`deser::io`](deser_core::io).  As property lists cannot be split,
//!   the whole stream is read before it's parsed.  Requires `std`.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod common;
mod de;
mod format;
#[cfg(feature = "io")]
mod io;
mod read_ascii;
mod read_binary;
mod read_xml;
mod ser;
mod uid;
mod write_ascii;
mod write_binary;
mod write_text;
mod write_xml;

pub use self::de::{Deserializer, DeserializerConfig, from_slice};
pub use self::format::Format;
#[cfg(feature = "io")]
pub use self::io::{WriterState, from_reader, to_writer};
pub use self::ser::{Serializer, SerializerConfig, to_string, to_vec};
pub use self::uid::Uid;

// the examples of the readme are tested
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
