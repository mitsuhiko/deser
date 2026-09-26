use std::io::Read;

use deser::de::Recording;
use deser::io::{Reader, Writer};
use deser::{ErrorKind, Event};
use deser_cbor::{Decoder, Deserializer, Encoder};

/// A reader that returns the input in chunks of a fixed size.
struct Chunked<'a> {
    input: &'a [u8],
    size: usize,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.size.min(buf.len()).min(self.input.len());
        buf[..len].copy_from_slice(&self.input[..len]);
        self.input = &self.input[len..];
        Ok(len)
    }
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
        0x9f, 0x01, 0x82, 0x02, 0x03, 0xff, // [_ 1, [2, 3]]
        0x7f, 0x62, b'a', b'b', 0x61, b'c', 0xff, // (_ "ab", "c")
        0xbf, 0x61, b'a', 0x01, 0xff, // {_ "a": 1}
        0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0, // 1(1363896240)
        0xa2, 0x61, b'x', 0x80, 0x61, b'y', 0xa0, // {"x": [], "y": {}}
        0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a, // 1.1
        0xf6, // null
        0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // -2^64
    ];
    // a byte string with a two byte length
    input.extend([0x59, 0x01, 0x00]);
    input.extend(std::iter::repeat_n(0xaa, 256));
    input
}

#[test]
fn test_sequence_in_chunks() {
    let input = sequence();
    let mut de = Deserializer::from_slice(&input);
    let expected = de
        .iter::<Recording>()
        .map(|x| events(x.unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), 10);
    for size in 1..=input.len() {
        let mut reader = Reader::new(
            Chunked {
                input: &input,
                size,
            },
            Decoder::default(),
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
    let input = [0x01, 0x82, 0x02, 0x03, 0x61, b'x'];
    let mut reader = Reader::new(Blocking(&input), Decoder::default());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2, 3]));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("x"));
}

#[test]
fn test_errors() {
    // items that do not match the type are skipped
    let input = [0x01, 0x61, b'x', 0x02];
    let mut reader = Reader::new(&input[..], Decoder::default());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    let err = reader.read::<u32>().unwrap_err();
    assert_eq!(err.offset(), Some(1));
    assert_eq!(reader.read::<u32>().unwrap(), Some(2));

    // items that are not well-formed end the stream
    for input in [
        &[0x01, 0x1c, 0x02][..],
        &[0x01, 0xff, 0x02],
        &[0x01, 0x82, 0xff],
    ] {
        let mut reader = Reader::new(input, Decoder::default());
        assert_eq!(reader.read::<u32>().unwrap(), Some(1));
        assert!(reader.read::<Recording>().is_err());
        assert!(reader.read::<Recording>().is_err());
    }

    // truncated items
    let mut reader = Reader::new(&[0x01, 0x82, 0x01][..], Decoder::default());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
}

#[test]
fn test_from_reader_and_to_writer() {
    let mut out = Vec::new();
    deser_cbor::to_writer(&mut out, &vec!["a", "b"]).unwrap();
    let value: Vec<String> = deser_cbor::from_reader(&out[..]).unwrap();
    assert_eq!(value, ["a", "b"]);

    out.push(0x01);
    let err = deser_cbor::from_reader::<Vec<String>, _>(&out[..]).unwrap_err();
    assert_eq!(err.offset(), Some(5));
    let err = deser_cbor::from_reader::<u32, _>(&b""[..]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
}

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), Encoder::default());
    writer.write(&1u32).unwrap();
    writer.write(&vec![2u32]).unwrap();
    assert_eq!(writer.into_inner(), [0x01, 0x81, 0x02]);
}

#[test]
fn test_borrowed() {
    let input = [0x62, b'h', b'i'];
    let mut reader = Reader::new(&input[..], Decoder::default());
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "hi");
}
