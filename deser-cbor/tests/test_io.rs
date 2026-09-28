use std::io::Read;

use deser::de::{Limits, Recording};
use deser::io::{Reader, Writer};
use deser::{ErrorKind, Event};
use deser_cbor::{Deserializer, DeserializerConfig, SerializerConfig};

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
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_sequence_in_chunks() {
    let input = sequence();
    let mut de = Deserializer::from_slice(&input);
    let expected = de
        .iter::<Recording>()
        .map(|x| events(x.unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), 10);
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
    let input = [0x01, 0x82, 0x02, 0x03, 0x61, b'x'];
    let mut reader = Reader::new(Blocking(&input), DeserializerConfig::new());
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2, 3]));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("x"));
}

#[test]
fn test_errors() {
    // items that do not match the type are skipped
    let input = [0x01, 0x61, b'x', 0x02];
    let mut reader = Reader::new(&input[..], DeserializerConfig::new());
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
        let mut reader = Reader::new(input, DeserializerConfig::new());
        assert_eq!(reader.read::<u32>().unwrap(), Some(1));
        assert!(reader.read::<Recording>().is_err());
        assert!(reader.read::<Recording>().is_err());
    }

    // truncated items
    let mut reader = Reader::new(&[0x01, 0x82, 0x01][..], DeserializerConfig::new());
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
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&1u32).unwrap();
    writer.write(&vec![2u32]).unwrap();
    assert_eq!(writer.into_inner(), [0x01, 0x81, 0x02]);
}

#[test]
fn test_borrowed() {
    let input = [0x62, b'h', b'i'];
    let mut reader = Reader::new(&input[..], DeserializerConfig::new());
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "hi");
}

#[test]
fn test_feeding_skips_items_that_fail() {
    // [1, "x", [2]] does not fit, the items around it do
    let input = [0x81, 0x01, 0x83, 0x01, 0x61, b'x', 0x81, 0x02, 0x81, 0x03];
    for size in chunk_sizes(input.len()) {
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
    let input = [0x81, 0x81, 0x81, 0x01, 0x81, 0x81, 0x01];
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

    // many chunks, fewer under miri which is slow
    let count = if cfg!(miri) { 300 } else { 10_000 };
    let value = (0..count)
        .map(|idx| (idx, "x".repeat(50)))
        .collect::<Vec<_>>();
    let bytes = deser_cbor::to_vec(&value).unwrap();
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

mod incremental {
    use std::collections::{BTreeMap, HashMap};

    use deser::io::Writer;
    use deser::ser::{Chunk, SeqEmitter, Serialize, SerializeHandle};
    use deser::{Error, State};
    use deser_cbor::SerializerConfig;

    /// A sequence whose length is not known upfront.
    struct Unsized(Vec<u64>);

    struct UnsizedEmitter<'a>(std::slice::Iter<'a, u64>);

    impl SeqEmitter for UnsizedEmitter<'_> {
        fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
            Ok(self.0.next().map(SerializeHandle::to))
        }
    }

    impl Serialize for Unsized {
        fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
            Ok(Chunk::seq(UnsizedEmitter(self.0.iter()), state))
        }
    }

    /// Counts the writes.
    struct Pieces(Vec<u8>, usize);

    impl std::io::Write for Pieces {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            self.1 += 1;
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn streamed(
        config: &SerializerConfig,
        value: &dyn Serialize,
        limit: usize,
    ) -> (Vec<u8>, usize) {
        let mut writer = Writer::new(Pieces(Vec::new(), 0), config);
        writer.set_buffer_limit(limit);
        writer.write(value).unwrap();
        let Pieces(out, writes) = writer.into_inner();
        (out, writes)
    }

    #[test]
    fn test_same_output() {
        let numbers: Vec<Vec<u64>> = (0..40).map(|x| (0..x * 10).collect()).collect();
        let unsized_values: Vec<Unsized> = (0..20).map(|x| Unsized((0..x * 3).collect())).collect();
        let nested = (
            Unsized((0..100).collect()),
            vec![Unsized((0..300).collect())],
        );
        let maps: Vec<HashMap<String, Vec<u64>>> = (0..20)
            .map(|x| {
                (0..x)
                    .map(|y| (format!("key {y}"), (0..y as u64).collect()))
                    .collect()
            })
            .collect();
        let sorted: BTreeMap<u64, String> = (0..500).map(|x| (x, format!("value {x}"))).collect();
        let values: [&dyn Serialize; 6] = [
            &numbers,
            &unsized_values,
            &nested,
            &maps,
            &sorted,
            &"scalar",
        ];
        for config in [
            SerializerConfig::new(),
            SerializerConfig::new().canonical(true),
        ] {
            for value in values {
                let expected = config.to_vec(value).unwrap();
                for limit in [1, 5, 64, 1000, usize::MAX] {
                    assert_eq!(streamed(&config, value, limit).0, expected, "limit {limit}");
                }
            }
        }
    }

    #[test]
    fn test_pieces() {
        let config = SerializerConfig::new();
        let numbers: Vec<Vec<u64>> = (0..100).map(|x| (0..x).collect()).collect();
        let (out, writes) = streamed(&config, &numbers, 64);
        assert_eq!(out, config.to_vec(&numbers).unwrap());
        assert!(writes > 20, "{writes}");

        // containers of unknown length are held back until they are
        // complete
        let value = Unsized((0..1000).collect());
        let (out, writes) = streamed(&config, &value, 64);
        assert_eq!(out, config.to_vec(&value).unwrap());
        assert_eq!(writes, 1);

        // as are maps in canonical mode
        let config = SerializerConfig::new().canonical(true);
        let map: HashMap<u64, u64> = (0..1000).map(|x| (x, x)).collect();
        let (out, writes) = streamed(&config, &map, 64);
        assert_eq!(out, config.to_vec(&map).unwrap());
        assert_eq!(writes, 1);
    }

    #[test]
    fn test_stream() {
        let config = SerializerConfig::new();
        let mut writer = Writer::new(Vec::new(), &config);
        writer.set_buffer_limit(3);
        let mut expected = Vec::new();
        for idx in 0..10u64 {
            let value: Vec<u64> = (0..idx * 100).collect();
            writer.write(&value).unwrap();
            expected.extend(config.to_vec(&value).unwrap());
        }
        assert_eq!(writer.into_inner(), expected);
    }
}
