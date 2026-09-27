//! The template of the parsers of `deser-json`, `deser-jsonc` and
//! `deser-json5`.
//!
//! This crate is not published and not used by anything.  The modules
//! other than this one are the source the parsers of the dialect crates are
//! generated from with `generate.py` (see there and the README).  This file
//! mirrors the `lib.rs` of the dialect crates just enough for the template
//! to compile (as the dialect with all capabilities).
mod de;
#[cfg(feature = "io")]
mod io;
mod parser;
mod scan;

pub use self::de::{Deserializer, DeserializerConfig, Iter, from_slice, from_str};
#[cfg(feature = "io")]
pub use self::io::{StreamState, from_reader};
pub use deser_json::Trailing;
