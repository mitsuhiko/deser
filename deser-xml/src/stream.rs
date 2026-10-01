//! Reading XML documents from streams.
#[cfg(feature = "io")]
use std::io::Read;

use deser_core::Error;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};

use crate::de::{Deserializer, DeserializerConfig};

/// Reads an XML document from a stream (see
/// [`deser::stream`](deser_core::stream)).
///
/// The stream is a single document which is parsed once the whole stream
/// was read (what follows the root element can only be checked at the
/// end).  Like with [`from_slice`](crate::from_slice) the input has to be
/// UTF-8.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_xml::DeserializerConfig;
///
/// #[derive(deser::Deserialize)]
/// struct Link {
///     #[deser(rename = "@href")]
///     href: String,
/// }
///
/// let mut reader =
///     DeserializerConfig::new().reader(&br#"<a href="/x"/>"#[..]);
/// let link: Link = reader.read().unwrap().unwrap();
/// assert_eq!(link.href, "/x");
/// assert!(reader.read::<Link>().unwrap().is_none());
/// # }
/// ```
#[derive(Debug, Default)]
pub struct StreamDeserializer {
    config: DeserializerConfig,
    // the document was read
    done: bool,
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
            done: false,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }
}

impl de::StreamDeserializer for StreamDeserializer {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        Ok(if !eof {
            Frame::Incomplete { consumed: 0 }
        } else if self.done {
            Frame::End
        } else {
            self.done = true;
            Frame::Value {
                start: 0,
                end: input.len(),
                consumed: input.len(),
            }
        })
    }

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_slice_with_config(frame, &self.config).drive(driver)
    }

    fn is_text(&self) -> bool {
        true
    }
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of an XML document (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// See [`StreamDeserializer`].
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self))
    }

    /// Deserializes a document from a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, StreamDeserializer::with_config(self))
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
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
