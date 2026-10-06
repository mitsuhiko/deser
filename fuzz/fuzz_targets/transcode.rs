//! Deserializes the input with one format and checks that the value
//! survives round trips through another (see `check_roundtrip`).
//!
//! The input starts with the index of the input format, the index of the
//! output format and the flags of the serializer (a little endian `u32`).
#![no_main]

use deser::Context;
use deser::de::Recording;
use deser_fuzz::formats::*;
use deser_fuzz::{Format, check_roundtrip};
use deser_value::Value;
use libfuzzer_sys::fuzz_target;

/// The number of formats.
const FORMATS: u8 = 15;

/// Calls a generic function with the format of an index.
macro_rules! with_format {
    ($index:expr, $f:ident($($arg:expr),*)) => {
        match $index % FORMATS {
            0 => $f::<Json>($($arg),*),
            1 => $f::<Jsonc>($($arg),*),
            2 => $f::<Json5>($($arg),*),
            3 => $f::<Hjson>($($arg),*),
            4 => $f::<Yaml>($($arg),*),
            5 => $f::<Toml>($($arg),*),
            6 => $f::<Ini>($($arg),*),
            7 => $f::<Cbor>($($arg),*),
            8 => $f::<Msgpack>($($arg),*),
            9 => $f::<Xml>($($arg),*),
            10 => $f::<Plist>($($arg),*),
            11 => $f::<Php>($($arg),*),
            12 => $f::<Pickle>($($arg),*),
            13 => $f::<Csv>($($arg),*),
            _ => $f::<Urlencoded>($($arg),*),
        }
    };
}

/// Deserializes the input.
fn read<F: Format>(data: &[u8]) -> Option<(Value, Recording)> {
    let config = F::config(0, Context::default());
    Some((
        F::from_slice(&config, data).ok()?,
        F::from_slice(&config, data).ok()?,
    ))
}

/// Writes the value with the other format.
fn write<F: Format>(ser_flags: u32, value: &Value, recording: &Recording) {
    let (ser, _) = F::ser_config(ser_flags, Context::default());
    // recordings carry the data of the events (like tags) of the input
    let _ = F::serialize(&ser, recording);
    check_roundtrip::<F>(ser_flags, value);
}

fuzz_target!(|data: &[u8]| {
    let Some((header, data)) = data.split_first_chunk::<6>() else {
        return;
    };
    let ser_flags = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    let Some((value, recording)) = with_format!(header[0], read(data)) else {
        return;
    };
    with_format!(header[1], write(ser_flags, &value, &recording));
});
