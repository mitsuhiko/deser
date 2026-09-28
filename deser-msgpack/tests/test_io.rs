use std::io::Read;

use crate::common::Chunked;

use deser::de::{Limits, Recording};
use deser::io::{Reader, Writer};
use deser::{ErrorKind, Event};
use deser_msgpack::{Deserializer, DeserializerConfig, SerializerConfig};

/// Returns the chunk sizes to read an input of `len` bytes in.
///
/// Miri is too slow for all sizes, it checks small sizes (which split the
/// input at every position), powers of two and the whole input.
fn chunk_sizes(len: usize) -> impl Iterator<Item = usize> {
    (1..=len).filter(move |&size| !cfg!(miri) || size <= 3 || size.is_power_of_two() || size == len)
}

/// A reader that panics when it's read after its input was returned.
struct Blocking<'a>(&'a [u8]);

impl Read for Blocking<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        assert!(!self.0.is_empty(), "read would block");
        let len = buf.len().min(self.0.len());
        buf[..len].copy_from_slice(&self.0[..len]);
        self.0 = &self.0[len..];
        Ok(len)
    }
}

fn events(value: Recording) -> Vec<Event<'static>> {
    value.events().cloned().collect()
}

fn sequence() -> Vec<u8> {
    let mut input = vec![
        0x01, // 1
        0x92, 0x01, 0x92, 0x02, 0x03, // [1, [2, 3]]
        0xd9, 0x03, b'a', b'b', b'c', // "abc" (str 8)
        0x81, 0xa1, b'a', 0x01, // {"a": 1}
        0xd6, 0xff, 0x51, 0x4b, 0x67, 0xb0, // timestamp 1363896240
        0xc7, 0x02, 0x07, 0xaa, 0xbb, // ext 7
        0x82, 0xa1, b'x', 0x90, 0xa1, b'y', 0x80, // {"x": [], "y": {}}
        0xcb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a, // 1.1
        0xc0, // nil
        0xd3, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // i64::MIN
    ];
    // binary data with a two byte length
    input.extend([0xc5, 0x01, 0x00]);
    input.extend(std::iter::repeat_n(0xaa, 256));
    input
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_sequence_in_chunks() {
    let input = sequence();
    let mut de = Deserializer::from_slice(&input);
    let expected = de
        .iter::<Recording>()
        .map(|x| events(x.unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), 11);
    for size in chunk_sizes(input.len()) {
        let mut reader = Reader::new(
            Chunked {
                input: &input,
                size,
            },
            DeserializerConfig::new(),
        );
        let mut values = Vec::new();
        while let Some(value) = reader.read::<Recording>().unwrap() {
            values.push(events(value));
        }
        assert_eq!(values, expected, "size {size}");
    }
}

#[test]
fn test_no_read_while_an_item_is_complete() {
    let input = [0x01, 0x92, 0x02, 0x03, 0xa1, b'x'];
    let mut reader = Reader::new(Blocking(&input), DeserializerConfig::new());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2, 3]));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("x"));
}

#[test]
fn test_errors() {
    // items that do not match the type are skipped
    let input = [0x01, 0xa1, b'x', 0x02];
    let mut reader = Reader::new(&input[..], DeserializerConfig::new());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    let err = reader.read::<u32>().unwrap_err();
    assert_eq!(err.offset(), Some(1));
    assert_eq!(reader.read::<u32>().unwrap(), Some(2));

    // items that are not well-formed end the stream
    for input in [
        &[0x01, 0xc1, 0x02][..],
        &[0x01, 0x92, 0xc1],
        &[0x01, 0x81, 0xa1, b'a', 0xc1, 0x02],
    ] {
        let mut reader = Reader::new(input, DeserializerConfig::new());
        assert_eq!(reader.read::<u32>().unwrap(), Some(1));
        assert!(reader.read::<Recording>().is_err());
        assert!(reader.read::<Recording>().is_err());
    }

    // truncated items
    let mut reader = Reader::new(&[0x01, 0x92, 0x01][..], DeserializerConfig::new());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
}

#[test]
fn test_from_reader_and_to_writer() {
    let mut out = Vec::new();
    deser_msgpack::to_writer(&mut out, &vec!["a", "b"]).unwrap();
    let value: Vec<String> = deser_msgpack::from_reader(&out[..]).unwrap();
    assert_eq!(value, ["a", "b"]);

    out.push(0x01);
    let err = deser_msgpack::from_reader::<Vec<String>, _>(&out[..]).unwrap_err();
    assert_eq!(err.offset(), Some(5));
    let err = deser_msgpack::from_reader::<u32, _>(&b""[..]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
}

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&1u32).unwrap();
    writer.write(&vec![2u32]).unwrap();
    assert_eq!(writer.into_inner(), [0x01, 0x91, 0x02]);
}

#[test]
fn test_borrowed() {
    let input = [0xa2, b'h', b'i'];
    let mut reader = Reader::new(&input[..], DeserializerConfig::new());
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "hi");
}

#[test]
fn test_feeding_skips_items_that_fail() {
    // [1, "x", [2]] does not fit, the items around it do
    let input = [0x91, 0x01, 0x93, 0x01, 0xa1, b'x', 0x91, 0x02, 0x91, 0x03];
    for size in 1..=input.len() {
        let mut reader = Reader::new(
            Chunked {
                input: &input,
                size,
            },
            DeserializerConfig::new(),
        );
        assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
        let err = reader.read::<Vec<u32>>().unwrap_err();
        assert_eq!(err.offset(), Some(4));
        assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![3]));
        assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
    }
}

#[test]
fn test_feeding_with_limits() {
    // [[[1]]] exceeds a depth of 2, [[1]] does not
    let input = [0x91, 0x91, 0x91, 0x01, 0x91, 0x91, 0x01];
    let mut reader = Reader::new(
        Chunked {
            input: &input,
            size: 2,
        },
        DeserializerConfig::new(),
    );
    let limits = |driver: &mut deser::de::DeserializeDriver<'_, '_>| {
        driver.push_layer(Limits::new().max_depth(2))
    };
    let err = reader.read_with::<Recording, _>(limits).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: recursion limit exceeded at offset 2"
    );
    // the item is skipped, the layer is added for every item
    assert_eq!(
        reader.read_with::<Vec<Vec<u32>>, _>(limits).unwrap(),
        Some(vec![vec![1]])
    );
}

#[test]
fn test_feeding_bounds_the_buffer() {
    use deser::de::DeserializeDriver;
    use deser::io::{DecodeBuffer, Status};

    let value = (0..10_000u32)
        .map(|idx| (idx, "x".repeat(50)))
        .collect::<Vec<_>>();
    let bytes = deser_msgpack::to_vec(&value).unwrap();
    let mut buffer = DecodeBuffer::new(DeserializerConfig::new());
    let mut out = None::<Vec<(u32, String)>>;
    let mut max_buffered = 0;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for chunk in bytes.chunks(1024) {
            buffer.extend_from_slice(chunk);
            if buffer.feed(&mut driver).unwrap() == Status::Ready {
                break;
            }
            max_buffered = max_buffered.max(buffer.buffered());
        }
    }
    assert_eq!(out.unwrap(), value);
    assert!(max_buffered < 100, "{max_buffered} bytes buffered");
}
