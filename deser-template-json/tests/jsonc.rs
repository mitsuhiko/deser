//! The tests of JSONC (see `integration.rs`).
pub use deser_jsonc as dialect;

#[allow(dead_code)]
pub const DIALECT: crate::Dialect = crate::Dialect {
    comments: true,
    trailing_commas: true,
    single_quotes: false,
    json5: false,
    hjson: false,
};

// The dialects share their parser, under miri (which is slow) the shared
// tests only run for JSON and the tests of a capability for the first
// dialect that has it.
/// The raw text values of the dialect (Hjson has none, JSON encodes).
#[allow(dead_code)]
pub type RawText<'a> = deser_jsonc::RawJsonc<'a>;

/// `true` if raw text values keep their input.
#[allow(dead_code)]
pub const KEEPS_INPUT: bool = true;

#[path = "common.rs"]
mod common;
#[cfg(not(miri))]
#[path = "test_bytes.rs"]
mod test_bytes;
#[cfg(not(miri))]
#[path = "test_collect.rs"]
mod test_collect;
#[path = "test_comments.rs"]
mod test_comments;
#[cfg(not(miri))]
#[path = "test_context.rs"]
mod test_context;
#[path = "test_de.rs"]
mod test_de;
#[cfg(not(miri))]
#[path = "test_io.rs"]
mod test_io;
#[cfg(not(miri))]
#[path = "test_locations.rs"]
mod test_locations;
#[cfg(not(miri))]
#[path = "test_nesting.rs"]
mod test_nesting;
#[path = "test_raw.rs"]
mod test_raw;
#[cfg(not(miri))]
#[path = "test_recover.rs"]
mod test_recover;
#[cfg(not(miri))]
#[path = "test_stream.rs"]
mod test_stream;
#[path = "test_tags.rs"]
mod test_tags;
