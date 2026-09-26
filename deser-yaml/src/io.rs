//! Reading and writing YAML streams.
use std::io::{Read, Write};

use deser::de::{Decoder, Frame};
use deser::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser::ser::Encoder;
use deser::ser::{Serialize, SerializeDriver};
use deser::{Atom, Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::SerializerConfig;

/// The kind of a line for splitting documents.
#[derive(PartialEq, Eq)]
enum Line {
    /// `---`
    DocumentStart,
    /// `...`
    DocumentEnd,
    /// Blank lines, comments and directives.
    Other,
    /// Anything else.
    Content,
}

/// Classifies a line (including its line break).
fn classify(line: &[u8]) -> Line {
    // byte order marks can precede every document
    let line = line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(line);
    let is_marker = |marker: &[u8]| {
        line.starts_with(marker) && matches!(line.get(3), None | Some(b' ' | b'\t' | b'\r' | b'\n'))
    };
    if is_marker(b"---") {
        return Line::DocumentStart;
    }
    if is_marker(b"...") {
        return Line::DocumentEnd;
    }
    match line
        .iter()
        .find(|&&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
    {
        None | Some(b'#') => Line::Other,
        Some(b'%') if line[0] == b'%' => Line::Other,
        Some(_) => Line::Content,
    }
}

/// The state of a YAML stream that is read.
///
/// See [`Decoder::State`].
#[derive(Debug, Default)]
pub struct StreamState {
    // the start of the next line to scan
    pos: usize,
    // the lines scanned so far contain a document
    has_document: bool,
}

impl StreamState {
    fn document(&mut self, end: usize) -> Frame {
        self.pos = 0;
        self.has_document = false;
        Frame::Value {
            start: 0,
            end,
            consumed: end,
        }
    }
}

/// Splits a YAML stream into documents (see [`deser::io`]).
///
/// A document ends where the next one starts (at a `---` line) or at a
/// document end marker (`...`).  When reading a stream that stays open
/// (for instance a socket), the writer should end every document with `...`
/// (see [`SerializerConfig::end_documents`]), otherwise a document is only
/// complete once the next one starts.  Comments and directives before a
/// document belong to it.
///
/// ```
/// use deser::io::Reader;
/// use deser_yaml::DeserializerConfig;
///
/// let mut reader = Reader::new(&b"--- a\n--- b\n"[..], DeserializerConfig::new());
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("a"));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("b"));
/// assert_eq!(reader.read::<String>().unwrap(), None);
/// ```
///
/// Documents are parsed like with a [`Deserializer`], so they can borrow
/// from the stream's buffer (see [`deser::io::Reader::read_borrowed`]).
/// Errors (including syntax errors) only discard their document, reading
/// continues with the next one.
impl Decoder for DeserializerConfig {
    type State = StreamState;

    fn frame(&self, state: &mut StreamState, input: &[u8], eof: bool) -> Result<Frame, Error> {
        loop {
            let line_end = match input[state.pos..].iter().position(|&b| b == b'\n') {
                Some(index) => state.pos + index + 1,
                None if eof => input.len(),
                // wait for the whole line
                None => return Ok(Frame::Incomplete { consumed: 0 }),
            };
            if line_end == state.pos {
                // the end of the stream
                return Ok(if state.has_document {
                    state.document(input.len())
                } else {
                    Frame::End
                });
            }
            match classify(&input[state.pos..line_end]) {
                Line::DocumentStart if state.has_document => return Ok(state.document(state.pos)),
                Line::DocumentStart | Line::Content => state.has_document = true,
                Line::DocumentEnd if state.has_document => return Ok(state.document(line_end)),
                Line::DocumentEnd | Line::Other => {}
            }
            state.pos = line_end;
        }
    }

    fn drive<'de>(
        &self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut de = Deserializer::from_slice_with_config(frame, self);
        de.drive(driver)?;
        de.end()
    }

    fn is_text(&self) -> bool {
        true
    }

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        crate::de::deserialize_single(Deserializer::from_slice_with_config(input, self), setup)
    }
}

/// Writes YAML documents to a stream (see [`deser::io`]).
///
/// Every value is written as a document, documents after the first start
/// with `---` (like with a [`Serializer`](crate::Serializer)).
///
/// ```
/// use deser::io::Writer;
/// use deser_yaml::SerializerConfig;
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
/// writer.write(&"a").unwrap();
/// writer.write(&vec![1, 2]).unwrap();
/// assert_eq!(writer.into_inner(), b"a\n---\n- 1\n- 2\n");
/// ```
impl Encoder for SerializerConfig {
    fn encode(
        &self,
        driver: &mut SerializeDriver<'_>,
        index: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let document = self.document(driver, index)?;
        out.extend_from_slice(document.as_bytes());
        Ok(())
    }
}

impl DeserializerConfig {
    /// Deserializes a value from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        let mut reader = deser::io::Reader::new(reader, self);
        let value = match reader.read()? {
            Some(value) => value,
            None => {
                // an empty stream is null
                let mut out = None;
                {
                    let mut driver = DeserializeDriver::new(&mut out);
                    driver.emit(Atom::Null)?;
                }
                out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty document"))?
            }
        };
        reader.end()?;
        Ok(value)
    }
}

impl SerializerConfig {
    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser::io::to_writer(writer, self, value)
    }
}

/// Deserializes a value from a reader.
///
/// This works like [`from_str`](crate::from_str): the stream must contain
/// at most one document, an empty stream is null.  The reader is read to
/// the end, it does not need to be buffered.  To read more than one
/// document use a [`deser::io::Reader`] with a [`DeserializerConfig`].
///
/// ```
/// let value: Vec<u32> = deser_yaml::from_reader(&b"- 1\n- 2\n"[..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes a value to a writer.
///
/// The document is written with a single write.
///
/// ```
/// let mut out = Vec::new();
/// deser_yaml::to_writer(&mut out, &vec![1, 2]).unwrap();
/// assert_eq!(out, b"- 1\n- 2\n");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
