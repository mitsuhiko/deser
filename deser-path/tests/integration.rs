//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
mod de;
mod ser;
