//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
mod test_bytes;
mod test_de;
mod test_io;
mod test_locations;
mod test_nesting;
mod test_pretty;
mod test_ser;
mod test_stream;
