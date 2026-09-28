//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
//!
//! These are the tests of writing JSON.  Reading is tested for all dialects
//! (JSON, JSONC and JSON5) by `deser-template-json`.
mod test_pretty;
mod test_ser;
#[cfg(feature = "io")]
mod test_write;
