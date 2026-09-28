//! Reading and writing XML documents from and to streams.
use std::io::{Read, Write};

use deser_core::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser_core::io::{Decoder, Encoded, Encoder, Frame};
use deser_core::ser::{Serialize, SerializeDriver};
use deser_core::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::{SerializerConfig, Writer};

/// Reads an XML document from a stream (see [`deser::io`](deser_core::io)).
///
/// The stream is a single document which is parsed once the whole stream
/// was read (what follows the root element can only be checked at the
/// end).  Like with [`from_slice`](crate::from_slice) the input has to be
/// UTF-8.
///
/// ```
/// use deser::io::Reader;
/// use deser_xml::DeserializerConfig;
///
/// #[derive(deser::Deserialize)]
/// struct Link {
///     #[deser(rename = "@href")]
///     href: String,
/// }
///
/// let mut reader =
///     Reader::new(&br#"<a href="/x"/>"#[..], DeserializerConfig::new());
/// let link: Link = reader.read().unwrap().unwrap();
/// assert_eq!(link.href, "/x");
/// assert!(reader.read::<Link>().unwrap().is_none());
/// ```
impl Decoder for DeserializerConfig {
    /// `true` once the document was read.
    type State = bool;

    fn frame(&self, done: &mut bool, input: &[u8], eof: bool) -> Result<Frame, Error> {
        Ok(if !eof {
            Frame::Incomplete { consumed: 0 }
        } else if *done {
            Frame::End
        } else {
            *done = true;
            Frame::Value {
                start: 0,
                end: input.len(),
                consumed: input.len(),
            }
        })
    }

    fn drive<'de>(
        &self,
        _state: &mut Self::State,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_slice_with_config(frame, self).drive(driver)
    }

    fn is_text(&self) -> bool {
        true
    }

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        Deserializer::from_slice_with_config(input, self).deserialize_with(setup)
    }
}

/// Writes an XML document to a stream (see [`deser::io`](deser_core::io)).
///
/// A stream holds a single document, writing a second value fails.  The
/// document is written incrementally (see [`Encoder::encode_incremental`]):
/// the output is written in pieces while the value is serialized.  Output
/// is final once the start tag it's in is complete: the start tag of an
/// element is held back until no more attributes can come, which for maps
/// (whose keys are not known upfront) is the end of the element.
///
/// The namespaces that are found once the start tag of the root element
/// was written are declared on the elements that use them rather than on
/// the root element (configured [namespaces](SerializerConfig::namespaces)
/// are always declared on the root element).
///
/// ```
/// use deser::io::Writer;
/// use deser_xml::SerializerConfig;
///
/// #[derive(deser::Serialize)]
/// #[deser(rename = "feed")]
/// struct Feed {
///     entry: Vec<u32>,
/// }
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
/// writer.set_buffer_limit(8);
/// writer.write(&Feed { entry: vec![1, 2, 3] }).unwrap();
/// assert_eq!(
///     writer.into_inner(),
///     b"<feed><entry>1</entry><entry>2</entry><entry>3</entry></feed>"
/// );
/// ```
impl Encoder for SerializerConfig {
    type State = WriterState;

    fn encode(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.encode_incremental(state, driver, out, usize::MAX)
            .map(|_| ())
    }

    fn supports_incremental(&self) -> bool {
        true
    }

    fn encode_incremental(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<Encoded, Error> {
        if state.written && state.document.is_none() {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "an XML stream holds a single document",
            ));
        }
        if !self.serialize_part(&mut state.document, driver, out, limit)? {
            return Ok(Encoded::Partial);
        }
        state.written = true;
        Ok(Encoded::Done)
    }
}

/// The state of an XML stream that is written.
///
/// This holds if the document was written and the progress of the
/// document while it's being written.  See [`Encoder::State`].
#[derive(Default)]
pub struct WriterState {
    written: bool,
    document: Option<Box<Writer>>,
}

impl WriterState {
    /// Returns `true` once the document was written.
    pub fn written(&self) -> bool {
        self.written
    }
}

impl std::fmt::Debug for WriterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterState")
            .field("written", &self.written)
            .field("in_progress", &self.document.is_some())
            .finish()
    }
}

impl DeserializerConfig {
    /// Deserializes a document from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, self)
    }
}

impl SerializerConfig {
    /// Serializes a value as XML document to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser_core::io::to_writer(writer, self, value)
    }
}

/// Deserializes a document from a reader.
///
/// The reader is read to the end, it does not need to be buffered.
///
/// ```
/// #[derive(deser::Deserialize)]
/// struct Link {
///     #[deser(rename = "@href")]
///     href: String,
/// }
///
/// let link: Link = deser_xml::from_reader(&br#"<a href="/x"/>"#[..]).unwrap();
/// assert_eq!(link.href, "/x");
/// ```
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes a value as XML document to a writer.
///
/// The output is written in pieces while the value is serialized (see
/// [`Encoder for SerializerConfig`](SerializerConfig#impl-Encoder-for-SerializerConfig)),
/// the writer does not need to be buffered.
///
/// ```
/// #[derive(deser::Serialize)]
/// #[deser(rename = "a")]
/// struct Link {
///     #[deser(rename = "@href")]
///     href: String,
/// }
///
/// let mut out = Vec::new();
/// deser_xml::to_writer(&mut out, &Link { href: "/x".into() }).unwrap();
/// assert_eq!(out, br#"<a href="/x"/>"#);
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
