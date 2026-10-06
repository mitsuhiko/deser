//! Deserializes the input with one format and checks that the value
//! survives round trips through another (see `check_roundtrip`).
//!
//! The values of the input stream are also transcoded into a stream of the
//! other format (see `transcode_stream`).
//!
//! The input starts with the index of the input format, the index of the
//! output format and the flags of the serializer (a little endian `u32`).
#![no_main]

use deser::Context;
use deser::de::Recording;
use deser::io::{Reader, Writer};
use deser_fuzz::formats::*;
use deser_fuzz::{Chunked, Escaped, Format, check_roundtrip, check_writer};
use deser_transcode::Transcoder;
use deser_value::Value;
use libfuzzer_sys::fuzz_target;

/// The number of formats.
const FORMATS: u8 = 15;

/// Calls a generic function with the format of an index (after the
/// given types).
macro_rules! with_format {
    ($index:expr, $f:ident::<$($pre:ident),*>($($arg:expr),*)) => {
        match $index % FORMATS {
            0 => $f::<$($pre,)* Json>($($arg),*),
            1 => $f::<$($pre,)* Jsonc>($($arg),*),
            2 => $f::<$($pre,)* Json5>($($arg),*),
            3 => $f::<$($pre,)* Hjson>($($arg),*),
            4 => $f::<$($pre,)* Yaml>($($arg),*),
            5 => $f::<$($pre,)* Toml>($($arg),*),
            6 => $f::<$($pre,)* Ini>($($arg),*),
            7 => $f::<$($pre,)* Cbor>($($arg),*),
            8 => $f::<$($pre,)* Msgpack>($($arg),*),
            9 => $f::<$($pre,)* Xml>($($arg),*),
            10 => $f::<$($pre,)* Plist>($($arg),*),
            11 => $f::<$($pre,)* Php>($($arg),*),
            12 => $f::<$($pre,)* Pickle>($($arg),*),
            13 => $f::<$($pre,)* Csv>($($arg),*),
            _ => $f::<$($pre,)* Urlencoded>($($arg),*),
        }
    };
    ($index:expr, $f:ident($($arg:expr),*)) => {
        with_format!($index, $f::<>($($arg),*))
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
    check_writer::<F, _>(ser_flags, (ser_flags >> 24) as u8, recording);
}

/// Transcodes the values of a stream into another stream with
/// `deser-transcode` (with the stream deserializer reading chunks and the
/// stream serializer writing parts) and checks that this writes what
/// writing the recordings of the values writes.
fn transcode_stream<F: Format, G: Format>(data: &[u8], ser_flags: u32) {
    let seed = (ser_flags >> 24) as u8;
    let config = F::config(0, Context::default());
    let (ser, _) = G::writer_config(ser_flags, Context::default());

    let mut reader = Reader::new(Chunked::new(data, seed), F::stream(&config));
    let mut writer = Writer::new(Vec::new(), G::serializer(&ser));
    writer.set_buffer_limit(1 + usize::from(seed) % 64);
    let mut transcoder = Transcoder::new();

    let mut expected_reader = Reader::new(data, F::stream(&config));
    let mut expected_writer = Writer::new(Vec::new(), G::serializer(&ser));
    expected_writer.set_buffer_limit(usize::MAX);

    for _ in 0..=data.len() {
        let expected = match expected_reader.read::<Recording>() {
            Ok(Some(recording)) => expected_writer.write(&recording),
            Ok(None) => {
                let end = reader.is_end();
                assert!(
                    matches!(end, Ok(true)),
                    "the stream does not end after its values: {end:?}\ninput: {:?}",
                    Escaped(data)
                );
                break;
            }
            Err(err) => Err(err),
        };
        let actual = transcoder.transcode(&mut reader, &mut writer);
        match (&expected, &actual) {
            (Ok(()), Ok(())) => {}
            // the stream or the value fails in both
            (Err(_), Err(_)) => return,
            _ => panic!(
                "transcoding the stream disagrees with writing its values\n\
                 transcode: {actual:?}\nexpected: {expected:?}\ninput: {:?}",
                Escaped(data)
            ),
        }
    }
    let (expected, actual) = (expected_writer.get_ref(), writer.get_ref());
    assert!(
        expected == actual,
        "transcoding the stream writes something else\ntranscode: {:?}\nexpected: {:?}\n\
         input: {:?}",
        Escaped(actual),
        Escaped(expected),
        Escaped(data)
    );
}

/// Transcodes the stream into the format of an index.
fn transcode_from<F: Format>(format: u8, data: &[u8], ser_flags: u32) {
    with_format!(format, transcode_stream::<F>(data, ser_flags));
}

fuzz_target!(|data: &[u8]| {
    let Some((header, data)) = data.split_first_chunk::<6>() else {
        return;
    };
    let ser_flags = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    with_format!(header[0], transcode_from(header[1], data, ser_flags));
    let Some((value, recording)) = with_format!(header[0], read(data)) else {
        return;
    };
    with_format!(header[1], write(ser_flags, &value, &recording));
});
