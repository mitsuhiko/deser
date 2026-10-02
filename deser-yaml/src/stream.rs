//! Reading YAML streams.
#[cfg(feature = "io")]
use std::io::Read;

use deser_core::Error;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};
#[cfg(feature = "io")]
use deser_core::{Atom, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};

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
#[derive(Debug, Default)]
struct StreamState {
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

/// Reads a stream of YAML documents (see [`deser::stream`](deser_core::stream)).
///
/// A document ends where the next one starts (at a `---` line) or at a
/// document end marker (`...`).  When reading a stream that stays open
/// (for instance a socket), the writer should end every document with `...`
/// (see [`SerializerConfig::set_end_documents`](crate::SerializerConfig::set_end_documents)), otherwise a document is only
/// complete once the next one starts.  Comments and directives before a
/// document belong to it.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_yaml::DeserializerConfig;
///
/// let mut reader =
///     DeserializerConfig::new().reader(&b"--- a\n--- b\n"[..]);
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("a"));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("b"));
/// assert_eq!(reader.read::<String>().unwrap(), None);
/// # }
/// ```
///
/// Documents are parsed like with a [`Deserializer`], so they can borrow
/// from the stream's buffer (see
/// [`InputBuffer::deserialize`](deser_core::stream::InputBuffer::deserialize)).
/// Errors (including syntax errors) only discard their document, reading
/// continues with the next one.
#[derive(Debug)]
pub struct StreamDeserializer {
    config: DeserializerConfig,
    state: StreamState,
}

impl Default for StreamDeserializer {
    fn default() -> StreamDeserializer {
        StreamDeserializer::new()
    }
}

impl StreamDeserializer {
    /// Creates a stream deserializer.
    pub fn new() -> StreamDeserializer {
        StreamDeserializer::with_config(&DeserializerConfig::new())
    }

    /// Creates a stream deserializer with the given configuration.
    pub fn with_config(config: &DeserializerConfig) -> StreamDeserializer {
        StreamDeserializer {
            config: config.clone(),
            state: StreamState::default(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }
}

impl de::StreamDeserializer for StreamDeserializer {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        let state = &mut self.state;
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

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut de = Deserializer::from_slice_with_config(frame, &self.config);
        de.drive(driver)?;
        de.end()
    }

    fn is_text(&self) -> bool {
        true
    }
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of a stream of YAML documents (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// See [`StreamDeserializer`] for how the stream is split into
    /// documents.
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self))
    }

    /// Deserializes a value from a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        let mut reader = self.reader(reader);
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

/// Deserializes a value from a reader.
///
/// This works like [`from_str`](crate::from_str): the stream must contain
/// at most one document, an empty stream is null.  The reader is read to
/// the end, it does not need to be buffered.  To read more than one
/// document use [`DeserializerConfig::reader`].
///
/// ```
/// let value: Vec<u32> =
///     deser_yaml::from_reader(&b"- 1\n- 2\n"[..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
