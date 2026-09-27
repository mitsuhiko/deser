//! The tests of reading JSON, JSONC and JSON5.
//!
//! The dialects share their parser (see the README), so they share their
//! tests too: every test file is a module of the dialects it applies to
//! (compiled once per dialect).  The dialect crate is imported as
//! `dialect` and [`Dialect`] tells the capabilities of the dialect for the
//! few tests that differ:
//!
//! ```ignore
//! use super::{DIALECT, dialect};
//!
//! if !DIALECT.trailing_commas {
//!     assert!(dialect::from_str::<Vec<u32>>("[1,]").is_err());
//! }
//! ```
//!
//! Tests of a capability are in their own file which is a module of the
//! dialects with the capability.  The tests of writing JSON are in
//! `deser-json` and the JSON5 test suite is in `deser-json5`.  All tests are
//! compiled into a single binary.

// every test file is a module of every dialect it applies to
#![allow(clippy::duplicate_mod)]

/// The capabilities of a dialect (see `generate.py`).
#[allow(dead_code)]
pub struct Dialect {
    pub comments: bool,
    pub trailing_commas: bool,
    pub json5: bool,
}

mod json;
mod json5;
mod jsonc;
