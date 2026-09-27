use std::io::Read;

use deser::de::DeserializeDriver;
use deser::io::{DecodeBuffer, Decoder, Encoder, Frame, Reader, Status, Writer};
use deser::ser::SerializeDriver;
use deser::{Atom, Error, ErrorKind, Event};

/// A format with a string or number per line.  Blank lines and leading
/// spaces are skipped.
///
/// The decoder remembers how far it scanned so it does not scan again.
#[derive(Default, Clone, Copy)]
struct Lines;

impl Decoder for Lines {
    /// How far the input was scanned.
    type State = usize;

    fn frame(&self, scanned: &mut usize, input: &[u8], eof: bool) -> Result<Frame, Error> {
        // skip blank lines
        let blank = input
            .iter()
            .position(|&b| b != b'\n')
            .unwrap_or(input.len());
        if blank > 0 {
            *scanned = scanned.saturating_sub(blank);
            return Ok(if eof && blank == input.len() {
                Frame::End
            } else {
                Frame::Incomplete { consumed: blank }
            });
        }
        if input.first() == Some(&b'!') {
            return Err(Error::new(ErrorKind::Unexpected, "bang").with_offset(0));
        }
        match input[*scanned..].iter().position(|&b| b == b'\n') {
            Some(index) => {
                let end = *scanned + index;
                *scanned = 0;
                Ok(Frame::Value {
                    start: input[..end].iter().take_while(|&&b| b == b' ').count(),
                    end,
                    consumed: end + 1,
                })
            }
            None if eof && input.is_empty() => Ok(Frame::End),
            None if eof => {
                *scanned = 0;
                Ok(Frame::Value {
                    start: 0,
                    end: input.len(),
                    consumed: input.len(),
                })
            }
            None => {
                *scanned = input.len();
                Ok(Frame::Incomplete { consumed: 0 })
            }
        }
    }

    fn drive<'de>(
        &self,
        _scanned: &mut usize,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let line = std::str::from_utf8(frame).unwrap();
        driver.state_mut().set_input_range(0, frame.len());
        match line.parse::<u64>() {
            Ok(value) => driver.emit(value),
            Err(_) => driver.emit_borrowed(line),
        }
        // errors of the format refer to the frame
        .map_err(|err| err.with_position(0, 1, 1))
    }
}

impl Encoder for Lines {
    type State = ();

    fn encode(
        &self,
        _state: &mut (),
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let mut line = String::new();
        driver.drive(|event, _| {
            match event {
                Event::Atom(Atom::U64(value)) => line.push_str(&value.to_string()),
                Event::Atom(Atom::Str(value)) => line.push_str(&value),
                _ => return Err(Error::new(ErrorKind::UnsupportedType, "unsupported")),
            }
            Ok(())
        })?;
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
        Ok(())
    }
}

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

const INPUT: &[u8] = b"1\n\n22\nhello\n\n333";

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_read_in_chunks() {
    for size in chunk_sizes(INPUT.len()) {
        let mut reader = Reader::new(Chunked { input: INPUT, size }, Lines);
        assert_eq!(reader.read::<u64>().unwrap(), Some(1));
        assert_eq!(reader.read::<u64>().unwrap(), Some(22));
        assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("hello"));
        assert_eq!(reader.read::<u64>().unwrap(), Some(333));
        assert_eq!(reader.read::<u64>().unwrap(), None);
        assert_eq!(reader.read::<u64>().unwrap(), None);
        reader.end().unwrap();
    }
}

#[test]
fn test_read_borrowed() {
    let mut reader = Reader::new(&b"a\nb\n"[..], Lines);
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "a");
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "b");
    assert_eq!(reader.read_borrowed::<&str>().unwrap(), None);
}

#[test]
fn test_iter() {
    let mut reader = Reader::new(&b"1\n2\n3"[..], Lines);
    let values = reader.iter::<u64>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2, 3]);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_errors_refer_to_the_stream() {
    for size in 1..=8 {
        // errors of values continue with the next value
        let input = b"1\n\nx\n2\n";
        let mut reader = Reader::new(Chunked { input, size }, Lines);
        assert_eq!(reader.read::<u64>().unwrap(), Some(1));
        let err = reader.read::<u64>().unwrap_err();
        assert_eq!(err.offset(), Some(3));
        assert_eq!((err.line(), err.column()), (Some(3), Some(1)));
        assert_eq!(reader.read::<u64>().unwrap(), Some(2));

        // frames that do not start at the start of a line (columns are
        // counted in characters)
        let input = "ä\n  x\n".as_bytes();
        let mut reader = Reader::new(Chunked { input, size }, Lines);
        assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("ä"));
        let err = reader.read::<u64>().unwrap_err();
        assert_eq!(err.offset(), Some(5));
        assert_eq!((err.line(), err.column()), (Some(2), Some(3)));
    }
}

#[test]
fn test_decoder_errors_are_fatal() {
    let mut reader = Reader::new(&b"1\n\n!\n2\n"[..], Lines);
    assert_eq!(reader.read::<u64>().unwrap(), Some(1));
    let err = reader.read::<u64>().unwrap_err();
    assert_eq!(err.message(), "bang");
    assert_eq!(err.offset(), Some(3));
    let err = reader.read::<u64>().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: cannot continue after an error"
    );
}

#[test]
fn test_from_reader() {
    assert_eq!(
        deser::io::from_reader::<u64, _, _>(&b"42\n\n"[..], Lines).unwrap(),
        42
    );
    let err = deser::io::from_reader::<u64, _, _>(&b""[..], Lines).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = deser::io::from_reader::<u64, _, _>(&b"1\n\n2\n"[..], Lines).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected value after the end at line 3 column 1"
    );
}

#[test]
fn test_io_errors() {
    struct Failing;

    impl Read for Failing {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken pipe"))
        }
    }

    let err = Reader::new(Failing, Lines).read::<u64>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    assert_eq!(
        std::error::Error::source(&err).unwrap().to_string(),
        "broken pipe"
    );
}

#[test]
fn test_decode_buffer() {
    let mut buffer = DecodeBuffer::new(Lines);
    assert_eq!(buffer.poll().unwrap(), Status::NeedInput);
    buffer.extend_from_slice(b"1\n2");
    assert_eq!(buffer.poll().unwrap(), Status::Ready);
    assert_eq!(buffer.deserialize::<u64>().unwrap(), 1);
    assert_eq!(buffer.poll().unwrap(), Status::NeedInput);
    assert_eq!(buffer.offset(), 2);
    buffer.set_eof();
    assert_eq!(buffer.poll().unwrap(), Status::Ready);
    assert_eq!(buffer.deserialize::<u64>().unwrap(), 2);
    assert_eq!(buffer.poll().unwrap(), Status::End);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_large_values() {
    // larger than a few reads, miri needs smaller values
    let long = "x".repeat(if cfg!(miri) { 20_000 } else { 100_000 });
    let input = format!("{long}\n{long}\n");
    let mut reader = Reader::new(input.as_bytes(), Lines);
    assert_eq!(reader.read::<String>().unwrap().unwrap(), long);
    assert_eq!(reader.read::<String>().unwrap().unwrap(), long);
    assert_eq!(reader.read::<String>().unwrap(), None);
}

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), Lines);
    writer.write(&1u64).unwrap();
    writer.write(&"hello").unwrap();
    // failed values write nothing
    assert!(writer.write(&vec![1u64]).is_err());
    writer.write(&2u64).unwrap();
    assert_eq!(writer.into_inner(), b"1\nhello\n2\n");

    let mut out = Vec::new();
    deser::io::to_writer(&mut out, Lines, &42u64).unwrap();
    assert_eq!(out, b"42\n");
}

#[test]
fn test_provided_methods() {
    // `from_slice` finds the value with the frames of the decoder
    assert_eq!(Lines.from_slice::<u64>(b"\n\n42\n\n").unwrap(), 42);
    let value: &str = Lines.from_slice(b"hello").unwrap();
    assert_eq!(value, "hello");
    let err = Lines.from_slice::<u64>(b"").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = Lines.from_slice::<u64>(b"1\n\n2").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected value after the end at line 3 column 1"
    );
    let err = Lines.from_slice::<u64>(b"\n  x\n").unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(2), Some(3)));

    assert_eq!(Lines.from_reader::<u64, _>(&b"7\n"[..]).unwrap(), 7);

    assert_eq!(Lines.to_vec(&42u64).unwrap(), b"42\n");
    let mut out = Vec::new();
    Lines.to_writer(&mut out, &"x").unwrap();
    assert_eq!(out, b"x\n");
}

/// A format with a line of column names followed by lines of values that
/// are separated by spaces.  Rows are maps of the column names to the
/// values.
struct Columns;

/// The state of a stream of [`Columns`].
#[derive(Default, Debug, Clone, PartialEq)]
struct ColumnsState {
    names: Option<Vec<String>>,
}

impl Decoder for Columns {
    type State = ColumnsState;

    fn frame(&self, state: &mut ColumnsState, input: &[u8], eof: bool) -> Result<Frame, Error> {
        let (end, consumed) = match input.iter().position(|&b| b == b'\n') {
            Some(end) => (end, end + 1),
            None if eof && input.is_empty() => return Ok(Frame::End),
            None if eof => (input.len(), input.len()),
            None => return Ok(Frame::Incomplete { consumed: 0 }),
        };
        if state.names.is_none() {
            // the first line holds the names of the columns
            let line = std::str::from_utf8(&input[..end]).unwrap();
            state.names = Some(line.split(' ').map(str::to_string).collect());
            return Ok(Frame::Incomplete { consumed });
        }
        Ok(Frame::Value {
            start: 0,
            end,
            consumed,
        })
    }

    fn drive<'de>(
        &self,
        state: &mut ColumnsState,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let names = state.names.as_ref().expect("names are read first");
        driver.emit(Event::map_start())?;
        for (name, value) in names
            .iter()
            .zip(std::str::from_utf8(frame).unwrap().split(' '))
        {
            // the names are only valid for the call, the values borrow
            driver.emit(Atom::Lexical(name.as_str().into()))?;
            driver.emit_borrowed(Atom::Lexical(value.into()))?;
        }
        driver.emit(Event::MapEnd)
    }
}

impl Encoder for Columns {
    type State = ColumnsState;

    fn encode(
        &self,
        state: &mut ColumnsState,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let mut names = Vec::new();
        let mut values = Vec::new();
        let mut is_key = false;
        driver.drive(|event, _| {
            match event {
                Event::MapStart(_) | Event::MapEnd => {}
                Event::Atom(atom) => {
                    is_key = !is_key;
                    let text = match atom {
                        Atom::Str(value) => value.to_string(),
                        Atom::U64(value) => value.to_string(),
                        _ => return Err(Error::new(ErrorKind::UnsupportedType, "unsupported")),
                    };
                    if is_key {
                        names.push(text)
                    } else {
                        values.push(text)
                    }
                }
                _ => return Err(Error::new(ErrorKind::UnsupportedType, "unsupported")),
            }
            Ok(())
        })?;
        match state.names {
            Some(ref expected) if *expected != names => {
                return Err(Error::new(ErrorKind::Unexpected, "different columns"));
            }
            Some(_) => {}
            None => {
                out.extend_from_slice(names.join(" ").as_bytes());
                out.push(b'\n');
            }
        }
        out.extend_from_slice(values.join(" ").as_bytes());
        out.push(b'\n');
        // the state is only updated once the value was serialized
        state.names = Some(names);
        Ok(())
    }
}

#[derive(Debug, PartialEq, deser::Deserialize, deser::Serialize)]
struct Row<'a> {
    name: &'a str,
    age: u64,
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_state_in_drive() {
    let input = b"name age\njane 42\njohn 23\n";
    for size in chunk_sizes(input.len()) {
        let mut reader = Reader::new(Chunked { input, size }, Columns);
        assert_eq!(reader.state().names, None);
        let row: Row = reader.read_borrowed().unwrap().unwrap();
        assert_eq!(
            row,
            Row {
                name: "jane",
                age: 42
            }
        );
        assert_eq!(
            reader.state().names.as_deref(),
            Some(&["name".to_string(), "age".to_string()][..])
        );
        let row: Row = reader.read_borrowed().unwrap().unwrap();
        assert_eq!(
            row,
            Row {
                name: "john",
                age: 23
            }
        );
        assert_eq!(reader.read_borrowed::<Row>().unwrap(), None);
    }

    // `from_slice` passes the state from the frames to the value
    let row: Row = Columns.from_slice(b"age name\n42 jane").unwrap();
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
    let state = ColumnsState {
        names: Some(vec!["name".into(), "age".into()]),
    };
    let mut reader = Reader::with_state(&b"jane 42\n"[..], Columns, state.clone());
    let row: Row = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(
        row,
        Row {
            name: "jane",
            age: 42
        }
    );

    let mut buffer = DecodeBuffer::with_state(Columns, state);
    buffer.extend_from_slice(b"john 23");
    buffer.set_eof();
    assert_eq!(buffer.poll().unwrap(), Status::Ready);
    assert_eq!(
        buffer.deserialize::<Row>().unwrap(),
        Row {
            name: "john",
            age: 23
        }
    );
}

#[test]
fn test_writer_state() {
    #[derive(deser::Serialize)]
    struct Other {
        age: u64,
    }

    let mut writer = Writer::new(Vec::new(), Columns);
    // a failed value leaves the state as it was: the next value writes the
    // names
    assert!(writer.write(&vec![1u64]).is_err());
    assert_eq!(writer.state().names, None);
    writer
        .write(&Row {
            name: "jane",
            age: 42,
        })
        .unwrap();
    assert!(writer.write(&Other { age: 1 }).is_err());
    writer
        .write(&Row {
            name: "john",
            age: 23,
        })
        .unwrap();
    assert_eq!(writer.into_inner(), b"name age\njane 42\njohn 23\n");

    let state = ColumnsState {
        names: Some(vec!["name".into(), "age".into()]),
    };
    let mut writer = Writer::with_state(Vec::new(), Columns, state);
    writer
        .write(&Row {
            name: "jane",
            age: 42,
        })
        .unwrap();
    assert_eq!(writer.into_inner(), b"jane 42\n");
}
