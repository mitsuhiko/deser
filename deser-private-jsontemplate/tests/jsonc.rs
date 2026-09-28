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

#[path = "common.rs"]
mod common;
#[path = "test_bytes.rs"]
mod test_bytes;
#[path = "test_collect.rs"]
mod test_collect;
#[path = "test_comments.rs"]
mod test_comments;
#[path = "test_de.rs"]
mod test_de;
#[path = "test_io.rs"]
mod test_io;
#[path = "test_locations.rs"]
mod test_locations;
#[path = "test_nesting.rs"]
mod test_nesting;
#[path = "test_recover.rs"]
mod test_recover;
#[path = "test_stream.rs"]
mod test_stream;
#[path = "test_tags.rs"]
mod test_tags;
