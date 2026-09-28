//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
#[macro_use]
mod common;
mod test_corpus;
mod test_de;
#[cfg(feature = "io")]
mod test_io;
mod test_ser;
