use std::io::Read;

use deser::de::DeserializeDriver;
use deser::io::{DecodeBuffer, Decoder, Encoder, Frame, Reader, Status, Writer};
use deser::ser::SerializeDriver;
use deser::{Atom, Error, ErrorKind, Event, Serialize};

/// A format with a string or number per line.  Blank lines and leading
/// spaces are skipped.
///
/// The decoder remembers how far it scanned so it does not scan again.
#[derive(Default)]
struct Lines {
    scanned: usize,
}

impl Decoder for Lines {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        // skip blank lines
        let blank = input
            .iter()
            .position(|&b| b != b'\n')
            .unwrap_or(input.len());
        if blank > 0 {
            self.scanned = self.scanned.saturating_sub(blank);
            return Ok(if eof && blank == input.len() {
                Frame::End
            } else {
                Frame::Incomplete { consumed: blank }
            });
        }
        if input.first() == Some(&b'!') {
            return Err(Error::new(ErrorKind::Unexpected, "bang").with_offset(0));
        }
        match input[self.scanned..].iter().position(|&b| b == b'\n') {
            Some(index) => {
                let end = self.scanned + index;
                self.scanned = 0;
                Ok(Frame::Value {
                    start: input[..end].iter().take_while(|&&b| b == b' ').count(),
                    end,
                    consumed: end + 1,
                })
            }
            None if eof && input.is_empty() => Ok(Frame::End),
            None if eof => {
                self.scanned = 0;
                Ok(Frame::Value {
                    start: 0,
                    end: input.len(),
                    consumed: input.len(),
                })
            }
            None => {
                self.scanned = input.len();
                Ok(Frame::Incomplete { consumed: 0 })
            }
        }
    }

    fn drive<'de>(
        &mut self,
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
    fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut line = String::new();
        SerializeDriver::new(value).drive(|event, _| {
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

const INPUT: &[u8] = b"1\n\n22\nhello\n\n333";

#[test]
fn test_read_in_chunks() {
    for size in 1..=INPUT.len() {
        let mut reader = Reader::new(Chunked { input: INPUT, size }, Lines::default());
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
    let mut reader = Reader::new(&b"a\nb\n"[..], Lines::default());
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "a");
    let value: &str = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value, "b");
    assert_eq!(reader.read_borrowed::<&str>().unwrap(), None);
}

#[test]
fn test_iter() {
    let mut reader = Reader::new(&b"1\n2\n3"[..], Lines::default());
    let values = reader.iter::<u64>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2, 3]);
}

#[test]
fn test_errors_refer_to_the_stream() {
    for size in 1..=8 {
        // errors of values continue with the next value
        let input = b"1\n\nx\n2\n";
        let mut reader = Reader::new(Chunked { input, size }, Lines::default());
        assert_eq!(reader.read::<u64>().unwrap(), Some(1));
        let err = reader.read::<u64>().unwrap_err();
        assert_eq!(err.offset(), Some(3));
        assert_eq!((err.line(), err.column()), (Some(3), Some(1)));
        assert_eq!(reader.read::<u64>().unwrap(), Some(2));

        // frames that do not start at the start of a line (columns are
        // counted in characters)
        let input = "ä\n  x\n".as_bytes();
        let mut reader = Reader::new(Chunked { input, size }, Lines::default());
        assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("ä"));
        let err = reader.read::<u64>().unwrap_err();
        assert_eq!(err.offset(), Some(5));
        assert_eq!((err.line(), err.column()), (Some(2), Some(3)));
    }
}

#[test]
fn test_decoder_errors_are_fatal() {
    let mut reader = Reader::new(&b"1\n\n!\n2\n"[..], Lines::default());
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
        deser::io::from_reader::<u64, _, _>(&b"42\n\n"[..], Lines::default()).unwrap(),
        42
    );
    let err = deser::io::from_reader::<u64, _, _>(&b""[..], Lines::default()).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = deser::io::from_reader::<u64, _, _>(&b"1\n\n2\n"[..], Lines::default()).unwrap_err();
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

    let err = Reader::new(Failing, Lines::default())
        .read::<u64>()
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    assert_eq!(
        std::error::Error::source(&err).unwrap().to_string(),
        "broken pipe"
    );
}

#[test]
fn test_decode_buffer() {
    let mut buffer = DecodeBuffer::new(Lines::default());
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
fn test_large_values() {
    let long = "x".repeat(100_000);
    let input = format!("{long}\n{long}\n");
    let mut reader = Reader::new(input.as_bytes(), Lines::default());
    assert_eq!(reader.read::<String>().unwrap().unwrap(), long);
    assert_eq!(reader.read::<String>().unwrap().unwrap(), long);
    assert_eq!(reader.read::<String>().unwrap(), None);
}

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), Lines::default());
    writer.write(&1u64).unwrap();
    writer.write(&"hello").unwrap();
    // failed values write nothing
    assert!(writer.write(&vec![1u64]).is_err());
    writer.write(&2u64).unwrap();
    assert_eq!(writer.into_inner(), b"1\nhello\n2\n");

    let mut out = Vec::new();
    deser::io::to_writer(&mut out, Lines::default(), &42u64).unwrap();
    assert_eq!(out, b"42\n");
}
