use std::io::Read;

use deser::io::{Decoder, Encoder, Reader, Writer};
use deser::{Deserialize, Serialize};
use deser_csv::{DeserializerConfig, Headers, SerializerConfig, StreamState, WriterState};

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
fn chunk_sizes(len: usize) -> impl Iterator<Item = usize> {
    (1..=len).filter(move |&size| !cfg!(miri) || size <= 3 || size.is_power_of_two() || size == len)
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Row {
    name: String,
    age: u32,
}

fn row(name: &str, age: u32) -> Row {
    Row {
        name: name.into(),
        age,
    }
}

#[test]
fn test_read_in_chunks() {
    let config = DeserializerConfig::new().comment(Some(b'#')).sep_line(true);
    let input =
        "\u{feff}sep=;\r\n# comment\r\nname;age\r\n\r\n\"ja\r\nne\";42\r\"jo\"\"hn\";23\r\n\r\n";
    for size in chunk_sizes(input.len()) {
        let mut reader = Reader::new(
            Chunked {
                input: input.as_bytes(),
                size,
            },
            &config,
        );
        assert_eq!(
            reader.read::<Row>().unwrap(),
            Some(row("ja\r\nne", 42)),
            "size {}",
            size
        );
        assert_eq!(reader.state().headers().unwrap(), ["name", "age"]);
        assert_eq!(reader.read::<Row>().unwrap(), Some(row("jo\"hn", 23)));
        assert_eq!(reader.read::<Row>().unwrap(), None);
        reader.end().unwrap();
    }
}

#[test]
fn test_errors_continue() {
    let input = b"name,age\njane,42\njohn,x\n\"max\"x,1\nmoritz,1,2\nanna,7";
    for size in chunk_sizes(input.len()) {
        let mut reader = Reader::new(Chunked { input, size }, DeserializerConfig::new());
        let mut results = Vec::new();
        loop {
            match reader.read::<Row>() {
                Ok(Some(row)) => results.push(Ok(row.name)),
                Ok(None) => break,
                Err(err) => {
                    results.push(Err((err.message().to_string(), err.line(), err.column())))
                }
            }
        }
        assert_eq!(
            results,
            [
                Ok("jane".into()),
                Err(("invalid value \"x\", expected u32".into(), Some(3), Some(6))),
                Err((
                    "unexpected character after a quoted field".into(),
                    Some(4),
                    Some(6)
                )),
                Err(("record has 3 fields, expected 2".into(), Some(5), Some(1))),
                Ok("anna".into()),
            ],
            "size {}",
            size
        );
    }
}

#[test]
fn test_stream_errors_end_the_stream() {
    let config = DeserializerConfig::new().max_record_len(16);
    let input = b"name,age\n\"this record never ends,1\njane,42\n";
    let mut reader = Reader::new(Chunked { input, size: 4 }, config);
    let err = reader.read::<Row>().unwrap_err();
    assert_eq!(
        err.message(),
        "record is longer than the maximum of 16 bytes"
    );
    assert_eq!((err.line(), err.column()), (Some(2), Some(1)));
    assert!(reader.read::<Row>().is_err());
}

#[test]
fn test_read_borrowed() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Row<'a> {
        name: &'a str,
        age: u32,
    }

    let mut reader = Reader::new(&b"name,age\njane,42\n"[..], DeserializerConfig::new());
    let row: Row = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(
        row,
        Row {
            name: "jane",
            age: 42
        }
    );
}

#[test]
fn test_reader_with_state() {
    let state = StreamState::with_headers(["name", "age"]);
    let mut reader = Reader::with_state(&b"jane,42\n"[..], DeserializerConfig::new(), state);
    assert_eq!(reader.read::<Row>().unwrap(), Some(row("jane", 42)));

    // without names
    let config = DeserializerConfig::new().headers(Headers::None);
    let mut reader = Reader::new(&b"jane,42\n"[..], config);
    assert_eq!(
        reader.read::<(String, u32)>().unwrap(),
        Some(("jane".into(), 42))
    );
    assert_eq!(reader.state().headers(), None);
}

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&row("jane", 42)).unwrap();
    assert!(writer.write(&42).is_err());
    writer.write(&row("john,jr", 23)).unwrap();
    assert_eq!(writer.state().headers().unwrap(), ["name", "age"]);
    assert_eq!(writer.into_inner(), b"name,age\njane,42\n\"john,jr\",23\n");

    // appending to a stream with names
    let state = WriterState::with_headers(["age", "name"]);
    let mut writer = Writer::with_state(Vec::new(), SerializerConfig::new(), state);
    writer.write(&row("jane", 42)).unwrap();
    assert_eq!(writer.into_inner(), b"42,jane\n");
}

#[test]
fn test_single_values() {
    let rows = vec![row("jane", 42), row("john", 23)];

    // single values are all records
    let csv = SerializerConfig::new().to_vec(&rows).unwrap();
    assert_eq!(csv, b"name,age\njane,42\njohn,23\n");
    let mut out = Vec::new();
    deser_csv::to_writer(&mut out, &rows).unwrap();
    assert_eq!(out, csv);

    let back: Vec<Row> = DeserializerConfig::new().from_slice(&csv).unwrap();
    assert_eq!(back, rows);
    let back: Vec<Row> = Decoder::from_reader(&DeserializerConfig::new(), &csv[..]).unwrap();
    assert_eq!(back, rows);
    let back: Vec<Row> = deser_csv::from_reader(&csv[..]).unwrap();
    assert_eq!(back, rows);
}

#[test]
fn test_roundtrip_stream() {
    let rows: Vec<Row> = (0..100)
        .map(|index| row(&format!("name \"{}\"\n,", index), index))
        .collect();
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    for row in &rows {
        writer.write(row).unwrap();
    }
    let csv = writer.into_inner();
    let mut reader = Reader::new(
        Chunked {
            input: &csv,
            size: 7,
        },
        DeserializerConfig::new(),
    );
    let back = reader.iter::<Row>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(back, rows);
}

#[test]
fn test_to_writer_streams_records() {
    use std::collections::BTreeMap;

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

    #[derive(deser::Serialize)]
    struct Row {
        name: String,
        age: u32,
        note: Option<&'static str>,
    }

    // the output is written in pieces of 8 KiB.  Miri is slow, it writes
    // fewer of them.
    let miri = cfg!(miri);
    let rows: Vec<Row> = (0..if miri { 700 } else { 5000 })
        .map(|idx| Row {
            name: format!("person {idx}"),
            age: idx % 100,
            note: (idx % 3 == 0).then_some("with, comma"),
        })
        .collect();
    let mut out = Pieces(Vec::new(), 0);
    deser_csv::to_writer(&mut out, &rows).unwrap();
    assert_eq!(out.0, deser_csv::to_string(&rows).unwrap().as_bytes());
    assert!(out.1 > if miri { 1 } else { 5 }, "{}", out.1);

    // maps whose keys come in another order than the columns
    let maps: Vec<BTreeMap<String, u32>> = (0..if miri { 300 } else { 3000 })
        .map(|idx| {
            let mut map = BTreeMap::from([("b".to_string(), idx), ("a".to_string(), idx * 2)]);
            if idx % 2 == 0 {
                map.insert("c".into(), 1);
            }
            map
        })
        .collect();
    const FLEXIBLE: SerializerConfig = SerializerConfig::new().flexible(true);
    let expected = FLEXIBLE.to_string(&maps);
    let mut out = Pieces(Vec::new(), 0);
    let rv = FLEXIBLE.to_writer(&mut out, &maps);
    match expected {
        Ok(expected) => {
            rv.unwrap();
            assert_eq!(out.0, expected.as_bytes());
        }
        Err(err) => assert_eq!(rv.unwrap_err().message(), err.message()),
    }

    let maps: Vec<BTreeMap<String, u32>> = (0..if miri { 1200 } else { 3000 })
        .map(|idx| BTreeMap::from([("b".to_string(), idx), ("a".to_string(), idx * 2)]))
        .collect();
    let mut out = Pieces(Vec::new(), 0);
    deser_csv::to_writer(&mut out, &maps).unwrap();
    assert_eq!(out.0, deser_csv::to_string(&maps).unwrap().as_bytes());
    assert!(out.1 > 1, "{}", out.1);
}
