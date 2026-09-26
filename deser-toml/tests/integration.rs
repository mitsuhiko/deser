//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
#[macro_use]
mod common;
mod test_bytes;
mod test_de;
#[cfg(feature = "io")]
mod test_io;
mod test_locations;
mod test_ser;
mod test_well_known;
