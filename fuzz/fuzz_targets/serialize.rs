//! Serializes generated values with every format and checks that the
//! output can be deserialized again (see `check_roundtrip`) and that the
//! stream serializers write it like the serializers (see `check_writer`).
#![no_main]

use arbitrary::Unstructured;
use deser_fuzz::formats::*;
use deser_fuzz::generate;
use deser_fuzz::{Format, check_roundtrip, check_writer};
use libfuzzer_sys::fuzz_target;

/// Checks the round trips and the stream serializer of a format, the
/// upper byte of the flags is the seed of the stream serializer (see
/// `check_writer`).
fn check<F: Format>(ser_flags: u32, value: &deser_value::Value) {
    check_roundtrip::<F>(ser_flags, value);
    check_writer::<F, _>(ser_flags, (ser_flags >> 24) as u8, value);
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let (Ok(format), Ok(ser_flags)) = (u.int_in_range(0..=15u8), u.arbitrary::<u32>()) else {
        return;
    };
    let Ok(value) = generate::value(&mut u, 5) else {
        return;
    };
    match format {
        0 => check::<Json>(ser_flags, &value),
        1 => check::<Jsonc>(ser_flags, &value),
        2 => check::<Json5>(ser_flags, &value),
        3 => check::<Hjson>(ser_flags, &value),
        4 => check::<Yaml>(ser_flags, &value),
        5 => check::<Toml>(ser_flags, &value),
        6 => check::<Ini>(ser_flags, &value),
        7 => check::<Cbor>(ser_flags, &value),
        8 => check::<Msgpack>(ser_flags, &value),
        9 => check::<Xml>(ser_flags, &value),
        10 => check::<Plist>(ser_flags, &value),
        11 => check::<Php>(ser_flags, &value),
        12 => check::<Pickle>(ser_flags, &value),
        13 => check::<Csv>(ser_flags, &value),
        14 => check::<Urlencoded>(ser_flags, &value),
        _ => {
            let _ = deser_debug::ToDebug::new(&value).to_string();
        }
    }
});
