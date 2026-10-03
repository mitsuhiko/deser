//! Writing JSON streams (reading is tested by deser-template-json).
use deser::Event;
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};

const STOP: DeserializerConfig = DeserializerConfig::builder()
    .trailing(Trailing::Stop)
    .build();

#[test]
fn test_writer() {
    let mut writer = SerializerConfig::builder()
        .trailing(Trailing::Stop)
        .build()
        .writer(Vec::new());
    writer.write(&1).unwrap();
    writer.write(&vec![2, 3]).unwrap();
    let out = writer.into_inner();
    assert_eq!(out, b"1\n[2,3]");

    // the output can be read again
    let mut reader = STOP.reader(&out[..]);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2, 3]));

    let mut writer = SerializerConfig::builder()
        .trailing(Trailing::Newline)
        .build()
        .writer(Vec::new());
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
    let mut writer = SerializerConfig::new().writer(Vec::new());
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

    let mut writer = SerializerConfig::builder()
        .trailing(Trailing::Newline)
        .build()
        .writer(Vec::new());
    writer
        .write_with(&vec![1u64, 2], |driver| driver.push_layer(NumbersAsStrings))
        .unwrap();
    assert_eq!(writer.into_inner(), b"[\"1\",\"2\"]\n");
}

/// Writes a value with a writer that passes on output once it's `limit`
/// bytes long, returns the output and the number of writes.
fn streamed<T: deser::Serialize + ?Sized>(
    config: &SerializerConfig,
    value: &T,
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

    let mut writer = config.writer(Pieces(Vec::new(), 0));
    writer.set_buffer_limit(limit);
    writer.write(value).unwrap();
    let Pieces(out, writes) = writer.into_inner();
    (String::from_utf8(out).unwrap(), writes)
}

#[test]
fn test_partial_same_output() {
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

    // miri is slow, it checks smaller values, fewer configurations and
    // limits (small limits pause at every value)
    let miri = cfg!(miri);
    let items: Vec<Item> = (0..if miri { 4 } else { 20 })
        .map(|idx| Item {
            name: format!("item {idx}"),
            tags: vec!["a", "b"],
            nested: BTreeMap::from([("x", vec![idx, idx + 1]), ("y", vec![])]),
            empty: vec![],
            text: "\u{e4}\u{f6}\u{fc} with \"quotes\"",
        })
        .collect();
    let numbers: Vec<Vec<u64>> = (0..if miri { 12 } else { 50 })
        .map(|x| (0..x).collect())
        .collect();
    let empty = Vec::<u32>::new();
    let values: [deser::ser::SerializeRef<'_>; 4] = [
        deser::ser::SerializeRef::new(&items),
        deser::ser::SerializeRef::new(&numbers),
        deser::ser::SerializeRef::new(&"scalar"),
        deser::ser::SerializeRef::new(&empty),
    ];
    let configs = [
        SerializerConfig::new(),
        SerializerConfig::builder()
            .pretty(Indent::Spaces(2))
            .build(),
        SerializerConfig::builder()
            .pretty(Indent::Tab)
            .compact(true)
            .build(),
        SerializerConfig::builder()
            .pretty(Indent::Spaces(2))
            .inline(InlinePolicy::LeafIfFits(40))
            .build(),
        SerializerConfig::builder()
            .indent(Indent::Spaces(4))
            .inline(InlinePolicy::LeafIfFits(20))
            .build(),
    ];
    let configs = if miri { &configs[3..] } else { &configs[..] };
    let limits: &[usize] = if miri {
        &[1, 16, usize::MAX]
    } else {
        &[1, 3, 16, 100, usize::MAX]
    };
    for config in configs {
        for value in values {
            let expected = config.to_string(&value).unwrap();
            for &limit in limits {
                let (out, _) = streamed(config, &value, limit);
                assert_eq!(out, expected, "limit {limit}");
            }
        }
    }

    // large values are written in pieces (plain values in pieces of a few
    // hundred atoms)
    let numbers: Vec<Vec<u64>> = (0..if miri { 60 } else { 200 })
        .map(|x| (0..x).collect())
        .collect();
    let limit = if miri { 128 } else { 1024 };
    let (out, writes) = streamed(&SerializerConfig::new(), &numbers, limit);
    assert!(writes > if miri { 5 } else { 20 }, "{writes}");
    assert_eq!(out, deser_json::to_string(&numbers).unwrap());
    let (_, writes) = streamed(&SerializerConfig::new(), &numbers, usize::MAX);
    assert_eq!(writes, 1);

    // also a single large plain sequence
    let (len, min_writes) = if miri { (3_000, 10) } else { (100_000, 100) };
    let numbers: Vec<u64> = (0..len).collect();
    let (out, writes) = streamed(&SerializerConfig::new(), &numbers, 1024);
    assert!(writes > min_writes, "{writes}");
    assert_eq!(out, deser_json::to_string(&numbers).unwrap());

    // and one in a struct
    #[derive(deser::Serialize)]
    struct Wrapper {
        name: &'static str,
        items: Vec<String>,
    }
    let wrapper = Wrapper {
        name: "x",
        items: (0..if miri { 2_000 } else { 10_000 })
            .map(|x| x.to_string())
            .collect(),
    };
    let (out, writes) = streamed(&SerializerConfig::new(), &wrapper, 1024);
    assert!(writes > if miri { 5 } else { 20 }, "{writes}");
    assert_eq!(out, deser_json::to_string(&wrapper).unwrap());
}

#[test]
fn test_partial_streams() {
    // values of a stream are separated like with a single write
    for trailing in [Trailing::Newline, Trailing::Stop] {
        let config = SerializerConfig::builder().trailing(trailing).build();
        let mut writer = config.writer(Vec::new());
        writer.set_buffer_limit(2);
        writer.write(&vec![1, 2, 3]).unwrap();
        writer.write(&"abc").unwrap();
        writer.write(&vec![4]).unwrap();
        let mut expected = deser_json::Serializer::with_config(config);
        expected.serialize(&vec![1, 2, 3]).unwrap();
        expected.serialize(&"abc").unwrap();
        expected.serialize(&vec![4]).unwrap();
        assert_eq!(writer.into_inner(), expected.finish().as_bytes());
    }
}

#[test]
fn test_partial_errors() {
    use deser::ser::Emit;
    use deser::{Error, ErrorKind, State};

    /// Fails to serialize.
    struct Fail;

    impl deser::Serialize for Fail {
        fn serialize<'a>(_value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
            Err(Error::new(ErrorKind::Custom, "fail"))
        }
    }

    let config = SerializerConfig::builder()
        .trailing(Trailing::Newline)
        .build();

    // a value that fails before anything was written is not written, the
    // stream continues
    let mut writer = config.writer(Vec::new());
    writer.set_buffer_limit(1000);
    let value: (Vec<u32>, Fail) = ((0..10).collect(), Fail);
    assert!(writer.write(&value).is_err());
    writer.write(&1).unwrap();
    assert_eq!(writer.into_inner(), b"1\n");

    // after a part of the value was written, the stream is broken
    let mut writer = config.writer(Vec::new());
    writer.set_buffer_limit(4);
    assert!(writer.write(&value).is_err());
    let err = writer.write(&1).unwrap_err();
    assert!(err.message().contains("partially written"), "{err}");
    assert!(writer.into_inner().starts_with(b"[[0,1,"));
}

#[test]
fn test_serializer_in_parts() {
    use deser::ser::{SerializeDriver, StreamSerializer};
    use deser_json::Serializer;

    // without IO: the output is taken while the value is written
    let config = SerializerConfig::builder()
        .trailing(Trailing::Newline)
        .build();
    let mut serializer = Serializer::with_config(config);
    // miri is slow, it checks a smaller value
    let len = if cfg!(miri) { 60 } else { 200 };
    let value: Vec<Vec<u64>> = (0..len).map(|x| (0..x).collect()).collect();
    let mut out = Vec::new();
    let mut parts = 0;
    let mut driver = SerializeDriver::new(&value);
    loop {
        let done = serializer.drive_partial(&mut driver, 64).unwrap();
        out.extend_from_slice(serializer.output());
        serializer.clear_output();
        parts += 1;
        if done {
            break;
        }
        assert!(serializer.in_progress());
        // no other value can be written in the meantime
        assert!(serializer.serialize(&1).is_err());
    }
    assert!(parts > 5, "{parts}");
    assert!(!serializer.in_progress());
    serializer.serialize(&1).unwrap();
    out.extend_from_slice(serializer.output());
    let mut expected = deser_json::to_string(&value).unwrap();
    expected.push_str("\n1\n");
    assert_eq!(String::from_utf8(out).unwrap(), expected);
    assert_eq!(serializer.written(), 2);

    // output that was not taken is kept, pretty output keeps its columns
    let config = SerializerConfig::builder()
        .trailing(Trailing::Newline)
        .pretty(deser_json::Indent::Spaces(2))
        .inline(deser_json::InlinePolicy::LeafIfFits(20))
        .build();
    let mut serializer = Serializer::with_config(config.clone());
    let value = vec![vec![1, 2], (0..20).collect()];
    for _ in 0..2 {
        let mut driver = SerializeDriver::new(&value);
        while !serializer.drive_partial(&mut driver, 1).unwrap() {}
    }
    let single = config.to_string(&value).unwrap();
    assert_eq!(serializer.as_str(), format!("{single}\n{single}\n"));

    // continuing a stream
    let config = SerializerConfig::builder().trailing(Trailing::Stop).build();
    let mut serializer = Serializer::with_written(config, 1);
    serializer.serialize(&2).unwrap();
    assert_eq!(serializer.finish(), "\n2");
}
