use std::io::Read;

use deser::de::Recording;
use deser::io::{Reader, Writer};
use deser::{ErrorKind, Event};
use deser_json::{Deserializer, DeserializerConfig, SerializerConfig, Trailing};

const STRICT: DeserializerConfig = DeserializerConfig::new();
const NEWLINE: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

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

/// A reader that returns its input at once and then blocks forever.
///
/// Blocking is simulated by a panic: the reader must not read while a
/// value is complete.
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

/// Reads all values of a stream in memory.
fn read_in_memory(config: &DeserializerConfig, input: &str) -> Vec<Vec<Event<'static>>> {
    let mut de = Deserializer::from_str_with_config(input, config);
    let mut rv = Vec::new();
    while !de.is_end() {
        rv.push(events(de.deserialize::<Recording>().unwrap()));
    }
    rv
}

/// Reads all values of a stream in chunks.
fn read_chunked(config: &DeserializerConfig, input: &str, size: usize) -> Vec<Vec<Event<'static>>> {
    let mut reader = Reader::new(
        Chunked {
            input: input.as_bytes(),
            size,
        },
        config,
    );
    let mut rv = Vec::new();
    while let Some(value) = reader.read::<Recording>().unwrap() {
        rv.push(events(value));
    }
    rv
}

const VALUES: &str = r#" 1 -2.5e3 "a\"b\\" true null [] {} [1, [2, [3]]]
{"a": {"b": "}]"}, "c": [false]} "\u00e4ä" 42"#;

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_stop_in_chunks() {
    let expected = read_in_memory(&STOP, VALUES);
    assert_eq!(expected.len(), 11);
    for size in chunk_sizes(VALUES.len()) {
        assert_eq!(read_chunked(&STOP, VALUES, size), expected, "size {size}");
    }
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_values_without_whitespace() {
    let input = r#"[1]{"a":2}"x"3"#;
    let expected = read_in_memory(&STOP, input);
    assert_eq!(expected.len(), 4);
    for size in chunk_sizes(input.len()) {
        assert_eq!(read_chunked(&STOP, input, size), expected);
    }
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_newline_in_chunks() {
    let input = "[1, 2]\n\n  {\"a\": \"b\"}  \r\n\"x\"\n   \n3";
    let expected = read_in_memory(&NEWLINE, input);
    assert_eq!(expected.len(), 4);
    for size in chunk_sizes(input.len()) {
        assert_eq!(read_chunked(&NEWLINE, input, size), expected);
    }
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_strict_in_chunks() {
    let input = " [1, {\"a\": [true]}] \n";
    let expected = read_in_memory(&STRICT, input);
    for size in chunk_sizes(input.len()) {
        assert_eq!(read_chunked(&STRICT, input, size), expected);
    }
    assert_eq!(read_chunked(&STRICT, "  \n", 1), Vec::<Vec<Event>>::new());
}

#[test]
fn test_no_read_while_a_value_is_complete() {
    // everything after the complete values is only read when needed
    let mut reader = Reader::new(Blocking(b"1\n\n[2]\n"), NEWLINE);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2]));

    let mut reader = Reader::new(Blocking(b"[1] \"x\" {} 2"), STOP);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("x"));
    assert_eq!(
        reader.read::<Recording>().unwrap().map(events),
        Some(vec![Event::map_start(), Event::MapEnd])
    );
}

#[test]
fn test_from_reader() {
    let value: Vec<u32> = deser_json::from_reader(Chunked {
        input: b" [1, 2,\n 3] ",
        size: 2,
    })
    .unwrap();
    assert_eq!(value, [1, 2, 3]);

    let err = deser_json::from_reader::<Vec<u32>, _>(&b"[1, 2]\n [3]"[..]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: garbage after input at line 2 column 2"
    );
    let err = deser_json::from_reader::<Vec<u32>, _>(&b"  "[..]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);

    // `Trailing::Stop` reads the first value, but it must be the only one
    let value: u32 = STOP.from_reader(&b" 1 "[..]).unwrap();
    assert_eq!(value, 1);
    let err = STOP.from_reader::<u32, _>(&b"1 2"[..]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected value after the end at line 1 column 3"
    );
}

#[test]
fn test_borrowed() {
    let mut reader = Reader::new(&b"{\"name\": \"Peter\"}\n"[..], NEWLINE);
    let value: std::collections::BTreeMap<&str, &str> = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value["name"], "Peter");
}

#[test]
fn test_errors() {
    // lines continue after errors, positions refer to the stream
    for size in [1, 3, 100] {
        let input = b"[1]\n[\"x\"]\n  [2, x]\n[3]\n";
        let mut reader = Reader::new(Chunked { input, size }, NEWLINE);
        let mut results = Vec::new();
        while let Some(result) = reader.read::<Vec<u32>>().transpose() {
            results.push(result.map_err(|err| err.to_string()));
        }
        assert_eq!(
            results,
            [
                Ok(vec![1]),
                Err("Unexpected: unexpected string, expected u32 at line 2 column 2".into()),
                Err("Unexpected: unexpected character at line 3 column 7".into()),
                Ok(vec![3]),
            ]
        );
    }

    // values continue after errors with `Trailing::Stop`
    let mut reader = Reader::new(&b"[1] [\"x\"] {]\n[3]"[..], STOP);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected string, expected u32 at line 1 column 6"
    );
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(err.offset(), Some(10));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![3]));

    // incomplete values at the end
    let mut reader = Reader::new(&b"[1] [2"[..], STOP);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
}

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

#[test]
fn test_generic_formats() {
    use deser::de::{Decoder, DeserializeOwned};
    use deser::ser::{Encoder, Serialize};

    /// Roundtrips a value through any format.
    fn roundtrip<T, D, E>(decoder: &D, encoder: &E, value: &T) -> T
    where
        T: Serialize + DeserializeOwned,
        D: Decoder,
        E: Encoder,
    {
        let bytes = encoder.to_vec(value).unwrap();
        decoder.from_slice(&bytes).unwrap()
    }

    let value = vec![(1u32, "a".to_string())];
    assert_eq!(roundtrip(&STRICT, &SerializerConfig::new(), &value), value);
    assert_eq!(
        roundtrip(
            &deser_json::DeserializerConfig::new(),
            &SerializerConfig::new(),
            &value
        ),
        value
    );

    // the configurations deserialize from slices like `from_slice`
    let value: Vec<&str> = Decoder::from_slice(&STRICT, br#"["a", "b"]"#).unwrap();
    assert_eq!(value, ["a", "b"]);
    assert!(Decoder::from_slice::<u32>(&STRICT, b"1 2").is_err());
    assert_eq!(Decoder::from_slice::<u32>(&STOP, b"1 2").unwrap(), 1);
}
