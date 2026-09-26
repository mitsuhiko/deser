//! Reading and writing YAML streams.
use std::io::{Read, Write};

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::io::Frame;
use deser::ser::Serialize;
use deser::{Atom, Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::{Serializer, SerializerConfig};

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

/// Splits a YAML stream into documents (see [`deser::io`]).
///
/// A document ends where the next one starts (at a `---` line) or at a
/// document end marker (`...`).  When reading a stream that stays open
/// (for instance a socket), the writer should end every document with `...`
/// (see [`Encoder::end_documents`]), otherwise a document is only complete
/// once the next one starts.  Comments and directives before a document
/// belong to it.
///
/// ```
/// use deser::io::Reader;
///
/// let mut reader = Reader::new(&b"--- a\n--- b\n"[..], deser_yaml::Decoder::default());
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("a"));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("b"));
/// assert_eq!(reader.read::<String>().unwrap(), None);
/// ```
///
/// Documents are parsed like with a [`Deserializer`], so they can borrow
/// from the stream's buffer (see [`deser::io::Reader::read_borrowed`]).
/// Errors (including syntax errors) only discard their document, reading
/// continues with the next one.
pub struct Decoder {
    config: DeserializerConfig,
    // the start of the next line to scan
    pos: usize,
    // the lines scanned so far contain a document
    has_document: bool,
}

impl Default for Decoder {
    fn default() -> Decoder {
        Decoder::new(&DeserializerConfig::new())
    }
}

impl Decoder {
    /// Creates a decoder with the given configuration.
    pub fn new(config: &DeserializerConfig) -> Decoder {
        Decoder {
            config: config.clone(),
            pos: 0,
            has_document: false,
        }
    }

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

impl deser::io::Decoder for Decoder {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        loop {
            let line_end = match input[self.pos..].iter().position(|&b| b == b'\n') {
                Some(index) => self.pos + index + 1,
                None if eof => input.len(),
                // wait for the whole line
                None => return Ok(Frame::Incomplete { consumed: 0 }),
            };
            if line_end == self.pos {
                // the end of the stream
                return Ok(if self.has_document {
                    self.document(input.len())
                } else {
                    Frame::End
                });
            }
            match classify(&input[self.pos..line_end]) {
                Line::DocumentStart if self.has_document => return Ok(self.document(self.pos)),
                Line::DocumentStart | Line::Content => self.has_document = true,
                Line::DocumentEnd if self.has_document => return Ok(self.document(line_end)),
                Line::DocumentEnd | Line::Other => {}
            }
            self.pos = line_end;
        }
    }

    fn drive<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut de = Deserializer::from_slice_with_config(frame, &self.config);
        de.drive(driver)?;
        de.end()
    }
}

/// Writes YAML documents to a stream (see [`deser::io`]).
///
/// Every value is written as a document, documents after the first start
/// with `---` (like with a [`Serializer`]).
///
/// ```
/// use deser::io::Writer;
/// use deser_yaml::SerializerConfig;
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new().encoder());
/// writer.write(&"a").unwrap();
/// writer.write(&vec![1, 2]).unwrap();
/// assert_eq!(writer.into_inner(), b"a\n---\n- 1\n- 2\n");
/// ```
pub struct Encoder {
    serializer: Serializer,
    end_documents: bool,
}

impl Default for Encoder {
    fn default() -> Encoder {
        Encoder::new(&SerializerConfig::new())
    }
}

impl Encoder {
    /// Creates an encoder with the given configuration.
    pub fn new(config: &SerializerConfig) -> Encoder {
        Encoder {
            serializer: Serializer::new(config.clone()),
            end_documents: false,
        }
    }

    /// Ends every document with a document end marker (`...`).
    ///
    /// This allows readers of a stream that stays open to see where a
    /// document ends without waiting for the next document.
    ///
    /// ```
    /// use deser::io::Writer;
    ///
    /// let encoder = deser_yaml::Encoder::default().end_documents();
    /// let mut writer = Writer::new(Vec::new(), encoder);
    /// writer.write(&"a").unwrap();
    /// writer.write(&"b").unwrap();
    /// assert_eq!(writer.into_inner(), b"a\n...\n---\nb\n...\n");
    /// ```
    pub fn end_documents(mut self) -> Encoder {
        self.end_documents = true;
        self
    }
}

impl deser::io::Encoder for Encoder {
    fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error> {
        let document = self.serializer.document_with(value, |_| {})?;
        out.extend_from_slice(document.as_bytes());
        if self.end_documents {
            out.extend_from_slice(b"...\n");
        }
        Ok(())
    }
}

impl DeserializerConfig {
    /// Creates a [`Decoder`] to read a stream with this configuration.
    pub fn decoder(&self) -> Decoder {
        Decoder::new(self)
    }

    /// Deserializes a value from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        let mut reader = deser::io::Reader::new(reader, self.decoder());
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
    /// Creates an [`Encoder`] to write a stream with this configuration.
    pub fn encoder(&self) -> Encoder {
        Encoder::new(self)
    }

    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser::io::to_writer(writer, self.encoder(), value)
    }
}

/// Deserializes a value from a reader.
///
/// This works like [`from_str`](crate::from_str): the stream must contain
/// at most one document, an empty stream is null.  The reader is read to
/// the end, it does not need to be buffered.  To read more than one
/// document use a [`deser::io::Reader`] with a [`Decoder`].
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
