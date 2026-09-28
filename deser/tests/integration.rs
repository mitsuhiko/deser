//! The integration tests are compiled into a single binary.  Every test
//! binary has to be linked and on macOS the first launch of a new binary
//! is slow, so separate binaries make the tests slower.
mod test_adapters;
mod test_borrow;
mod test_bound;
mod test_bytes;
mod test_container_as;
mod test_custom_map;
mod test_de;
mod test_de_derive;
mod test_derive_unscoped;
mod test_duplicates;
mod test_enums;
mod test_event_data;
mod test_ext;
mod test_implicit;
#[cfg(feature = "io")]
mod test_io;
mod test_layers;
mod test_lexical;
mod test_names;
mod test_other;
mod test_other_crates;
mod test_ser;
mod test_ser_derive;
mod test_skip;
mod test_soundness;
mod test_std;
mod test_structs;
mod test_tagged;
mod test_unknown;
mod test_update;
mod test_variant_adapters;
mod test_well_known;
