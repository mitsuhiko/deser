//! Writing JSON streams (reading is tested by deser-template-json).
use deser::Event;
use deser::io::{Reader, Writer};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};

const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new().trailing(Trailing::Stop));
    writer.write(&1).unwrap();
    writer.write(&vec![2, 3]).unwrap();
    let out = writer.into_inner();
    assert_eq!(out, b"1\n[2,3]");

    // the output can be read again
    let mut reader = Reader::new(&out[..], STOP);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2, 3]));

    let mut writer = Writer::new(
        Vec::new(),
        SerializerConfig::new().trailing(Trailing::Newline),
    );
    writer.write(&"a").unwrap();
    writer.write(&"b").unwrap();
    assert_eq!(writer.into_inner(), b"\"a\"\n\"b\"\n");

    let mut out = Vec::new();
    deser_json::to_writer(&mut out, &vec!["x"]).unwrap();
    assert_eq!(out, b"[\"x\"]");
}

#[test]
fn test_writer_strict_and_layers() {
    use deser::ser::{Layer, Next};
    use deser::{Atom, Error};

    // a strict stream holds a single value
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&1).unwrap();
    assert!(writer.write(&2).is_err());
    assert_eq!(writer.into_inner(), b"1");

    /// Writes all numbers as strings.
    struct NumbersAsStrings;

    impl Layer for NumbersAsStrings {
        fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
            match event {
                Event::Atom(Atom::U64(value)) => next.emit(value.to_string().into()),
                event => next.emit(event),
            }
        }
    }

    let mut writer = Writer::new(
        Vec::new(),
        SerializerConfig::new().trailing(Trailing::Newline),
    );
    writer
        .write_with(&vec![1u64, 2], |driver| driver.push_layer(NumbersAsStrings))
        .unwrap();
    assert_eq!(writer.into_inner(), b"[\"1\",\"2\"]\n");
}

/// Writes a value with a writer that passes on output once it's `limit`
/// bytes long, returns the output and the number of writes.
fn streamed(
    config: &SerializerConfig,
    value: &dyn deser::Serialize,
    limit: usize,
) -> (String, usize) {
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

    let mut writer = Writer::new(Pieces(Vec::new(), 0), config);
    writer.set_buffer_limit(limit);
    writer.write(value).unwrap();
    let Pieces(out, writes) = writer.into_inner();
    (String::from_utf8(out).unwrap(), writes)
}

#[test]
fn test_incremental_same_output() {
    use std::collections::BTreeMap;

    use deser_json::{Indent, InlinePolicy};

    #[derive(deser::Serialize)]
    struct Item {
        name: String,
        tags: Vec<&'static str>,
        nested: BTreeMap<&'static str, Vec<u32>>,
        empty: Vec<u32>,
        text: &'static str,
    }

    let items: Vec<Item> = (0..20)
        .map(|idx| Item {
            name: format!("item {idx}"),
            tags: vec!["a", "b"],
            nested: BTreeMap::from([("x", vec![idx, idx + 1]), ("y", vec![])]),
            empty: vec![],
            text: "\u{e4}\u{f6}\u{fc} with \"quotes\"",
        })
        .collect();
    let numbers: Vec<Vec<u64>> = (0..50).map(|x| (0..x).collect()).collect();
    let values: [&dyn deser::Serialize; 4] = [&items, &numbers, &"scalar", &Vec::<u32>::new()];
    let configs = [
        SerializerConfig::new(),
        SerializerConfig::new().pretty(Indent::Spaces(2)),
        SerializerConfig::new().pretty(Indent::Tab).compact(true),
        SerializerConfig::new()
            .pretty(Indent::Spaces(2))
            .inline(InlinePolicy::LeafIfFits(40)),
        SerializerConfig::new()
            .indent(Indent::Spaces(4))
            .inline(InlinePolicy::LeafIfFits(20)),
    ];
    for config in &configs {
        for value in values {
            let expected = config.to_string(value).unwrap();
            for limit in [1, 3, 16, 100, usize::MAX] {
                let (out, _) = streamed(config, value, limit);
                assert_eq!(out, expected, "limit {limit}");
            }
        }
    }

    // large values are written in pieces (plain values in pieces of a few
    // hundred atoms)
    let numbers: Vec<Vec<u64>> = (0..200).map(|x| (0..x).collect()).collect();
    let (out, writes) = streamed(&SerializerConfig::new(), &numbers, 1024);
    assert!(writes > 20, "{writes}");
    assert_eq!(out, deser_json::to_string(&numbers).unwrap());
    let (_, writes) = streamed(&SerializerConfig::new(), &numbers, usize::MAX);
    assert_eq!(writes, 1);

    // also a single large plain sequence
    let numbers: Vec<u64> = (0..100_000).collect();
    let (out, writes) = streamed(&SerializerConfig::new(), &numbers, 1024);
    assert!(writes > 100, "{writes}");
    assert_eq!(out, deser_json::to_string(&numbers).unwrap());

    // and one in a struct
    #[derive(deser::Serialize)]
    struct Wrapper {
        name: &'static str,
        items: Vec<String>,
    }
    let wrapper = Wrapper {
        name: "x",
        items: (0..10_000).map(|x| x.to_string()).collect(),
    };
    let (out, writes) = streamed(&SerializerConfig::new(), &wrapper, 1024);
    assert!(writes > 20, "{writes}");
    assert_eq!(out, deser_json::to_string(&wrapper).unwrap());
}

#[test]
fn test_incremental_streams() {
    // values of a stream are separated like with a single write
    for trailing in [Trailing::Newline, Trailing::Stop] {
        let config = SerializerConfig::new().trailing(trailing);
        let mut writer = Writer::new(Vec::new(), &config);
        writer.set_buffer_limit(2);
        writer.write(&vec![1, 2, 3]).unwrap();
        writer.write(&"abc").unwrap();
        writer.write(&vec![4]).unwrap();
        let mut expected = deser_json::Serializer::with_config(&config);
        expected.serialize(&vec![1, 2, 3]).unwrap();
        expected.serialize(&"abc").unwrap();
        expected.serialize(&vec![4]).unwrap();
        assert_eq!(writer.into_inner(), expected.finish().as_bytes());
    }
}

#[test]
fn test_incremental_errors() {
    use deser::ser::Chunk;
    use deser::{Error, ErrorKind, State};

    /// Fails to serialize.
    struct Fail;

    impl deser::Serialize for Fail {
        fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
            Err(Error::new(ErrorKind::Unexpected, "fail"))
        }
    }

    let config = SerializerConfig::new().trailing(Trailing::Newline);

    // a value that fails before anything was written is not written, the
    // stream continues
    let mut writer = Writer::new(Vec::new(), &config);
    writer.set_buffer_limit(1000);
    let value: (Vec<u32>, Fail) = ((0..10).collect(), Fail);
    assert!(writer.write(&value).is_err());
    writer.write(&1).unwrap();
    assert_eq!(writer.into_inner(), b"1\n");

    // after a part of the value was written, the stream is broken
    let mut writer = Writer::new(Vec::new(), &config);
    writer.set_buffer_limit(4);
    assert!(writer.write(&value).is_err());
    let err = writer.write(&1).unwrap_err();
    assert!(err.message().contains("partially written"), "{err}");
    assert!(writer.into_inner().starts_with(b"[[0,1,"));
}
