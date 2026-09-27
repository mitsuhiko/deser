//! The integration tests are compiled into a single binary.
mod test_de;
#[cfg(feature = "io")]
mod test_io;
#[cfg(feature = "io")]
mod test_rust_csv;
mod test_ser;
