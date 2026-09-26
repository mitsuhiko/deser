use std::io::Read;

use deser::Event;
use deser::de::Recording;
use deser::io::{Reader, Writer};
use deser_yaml::{Deserializer, DeserializerConfig, SerializerConfig};

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

fn read_in_memory(input: &str) -> Vec<Vec<Event<'static>>> {
    let mut de = Deserializer::from_str(input);
    de.iter::<Recording>().map(|x| events(x.unwrap())).collect()
}

fn read_chunked(input: &str, size: usize) -> Vec<Vec<Event<'static>>> {
    let mut reader = Reader::new(
        Chunked {
            input: input.as_bytes(),
            size,
        },
        DeserializerConfig::new(),
    );
    let mut rv = Vec::new();
    while let Some(value) = reader.read::<Recording>().unwrap() {
        rv.push(events(value));
    }
    rv
}

const STREAMS: &[(&str, usize)] = &[
    (
        "a: 1\n--- b\n---\n- x\n...\n# comment\n%YAML 1.1\n--- yes\n---\n",
        5,
    ),
    ("# only a comment\n\n", 0),
    ("", 0),
    ("\u{feff}# with a byte order mark\n--- a\n--- b", 2),
    ("text: |\n  line\n  # not a comment\n\n  more\n--- 2\n", 2),
    ("a\r\n---\r\nb\r\n", 2),
    ("--- |\n  foo\n...\n--- >\n  bar\n  baz\n", 2),
    ("--- # comment\n[1, 2]\n...\n...\n{a: b}", 2),
    ("- &anchor x\n- *anchor\n---\n- y", 2),
];

#[test]
fn test_documents_in_chunks() {
    for &(input, count) in STREAMS {
        let expected = read_in_memory(input);
        assert_eq!(expected.len(), count, "{input:?}");
        for size in 1..=input.len().max(1) {
            assert_eq!(read_chunked(input, size), expected, "{input:?} size {size}");
        }
    }
}

#[test]
fn test_no_read_while_a_document_is_complete() {
    let mut reader = Reader::new(Blocking(b"a\n...\n--- b\n...\n"), DeserializerConfig::new());
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("a"));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("b"));
}

#[test]
fn test_errors_only_discard_their_document() {
    for size in [1, 5, 100] {
        let input = b"a: 1\n---\nb: [1\n---\nc: 3\n---\nd: x\n";
        let mut reader = Reader::new(Chunked { input, size }, DeserializerConfig::new());
        let mut results = Vec::new();
        while let Some(result) = reader
            .read::<std::collections::BTreeMap<String, u32>>()
            .transpose()
        {
            results.push(
                result
                    .map(|x| x.into_keys().collect::<Vec<_>>())
                    .map_err(|err| err.line()),
            );
        }
        assert_eq!(
            results,
            [
                Ok(vec!["a".to_string()]),
                // the sequence is not a number (before the syntax error)
                Err(Some(3)),
                Ok(vec!["c".to_string()]),
                Err(Some(7)),
            ]
        );
    }
}

#[test]
fn test_from_reader() {
    let value: Vec<u32> = deser_yaml::from_reader(&b"--- [1, 2]\n...\n"[..]).unwrap();
    assert_eq!(value, [1, 2]);

    // an empty stream is null
    let value: Option<u32> = deser_yaml::from_reader(&b"# nothing\n"[..]).unwrap();
    assert_eq!(value, None);

    let err = deser_yaml::from_reader::<u32, _>(&b"1\n--- 2\n"[..]).unwrap_err();
    assert_eq!(err.line(), Some(2));
}

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&vec![1, 2]).unwrap();
    writer.write(&"b").unwrap();
    let output = writer.into_inner();
    assert_eq!(output, b"- 1\n- 2\n---\nb\n");

    let mut reader = Reader::new(&output[..], DeserializerConfig::new());
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("b"));

    let mut out = Vec::new();
    deser_yaml::to_writer(&mut out, &"x").unwrap();
    assert_eq!(out, b"x\n");
}
