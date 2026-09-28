//! The template of the parsers of `deser-json`, `deser-jsonc`,
//! `deser-json5` and `deser-hj`.
//!
//! This crate is not published and not used by anything.  The modules
//! other than this one are the source the parsers of the dialect crates are
//! generated from with `generate.py` (see there and the README).  This file
//! mirrors the `lib.rs` of the dialect crates just enough for the template
//! to compile (as the dialect with all capabilities).
//!
//! The integration tests of this crate are the tests of reading JSON,
//! JSONC, JSON5 and Hjson (see `tests/integration.rs`).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod de;
mod parser;
mod scan;
mod stream;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice, from_str};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;
pub use deser_json::Trailing;
