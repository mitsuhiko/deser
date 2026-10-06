//! Serializes generated values with every format and checks that the
//! output can be deserialized again (see `check_roundtrip`).
#![no_main]

use arbitrary::Unstructured;
use deser_fuzz::check_roundtrip;
use deser_fuzz::formats::*;
use deser_fuzz::generate;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let (Ok(format), Ok(ser_flags)) = (u.int_in_range(0..=15u8), u.arbitrary::<u32>()) else {
        return;
    };
    let Ok(value) = generate::value(&mut u, 5) else {
        return;
    };
    match format {
        0 => check_roundtrip::<Json>(ser_flags, &value),
        1 => check_roundtrip::<Jsonc>(ser_flags, &value),
        2 => check_roundtrip::<Json5>(ser_flags, &value),
        3 => check_roundtrip::<Hjson>(ser_flags, &value),
        4 => check_roundtrip::<Yaml>(ser_flags, &value),
        5 => check_roundtrip::<Toml>(ser_flags, &value),
        6 => check_roundtrip::<Ini>(ser_flags, &value),
        7 => check_roundtrip::<Cbor>(ser_flags, &value),
        8 => check_roundtrip::<Msgpack>(ser_flags, &value),
        9 => check_roundtrip::<Xml>(ser_flags, &value),
        10 => check_roundtrip::<Plist>(ser_flags, &value),
        11 => check_roundtrip::<Php>(ser_flags, &value),
        12 => check_roundtrip::<Pickle>(ser_flags, &value),
        13 => check_roundtrip::<Csv>(ser_flags, &value),
        14 => check_roundtrip::<Urlencoded>(ser_flags, &value),
        _ => {
            let _ = deser_debug::ToDebug::new(&value).to_string();
        }
    }
});
