//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
mod test_adapters;
mod test_borrow;
mod test_bound;
mod test_bytes;
mod test_custom_map;
mod test_de;
mod test_de_derive;
mod test_derive_unscoped;
mod test_enums;
mod test_event_data;
mod test_ext;
mod test_layers;
mod test_other;
mod test_ser;
mod test_ser_derive;
mod test_soundness;
mod test_tagged;
mod test_well_known;
