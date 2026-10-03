//! Reading TOML from streams.
#[cfg(feature = "io")]
use std::io::Read;

use deser_core::Error;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};

use crate::de::{Deserializer, DeserializerConfig};

/// Reads a TOML document from a stream (see
/// [`deser::stream`](deser_core::stream)).
///
/// TOML documents cannot be split: tables can be extended anywhere in the
/// document.  The stream is a single document which is parsed once the
/// whole stream was read.  An empty stream is an empty table.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use std::collections::BTreeMap;
/// use deser_toml::DeserializerConfig;
///
/// let mut reader =
///     DeserializerConfig::new().reader(&b"a = 1\nb = 2\n"[..]);
/// let value: BTreeMap<String, u32> = reader.read().unwrap().unwrap();
/// assert_eq!(value["b"], 2);
/// assert!(reader.read::<BTreeMap<String, u32>>().unwrap().is_none());
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
    fn context(&self) -> deser_core::Context {
        self.config.context().clone()
    }

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
    /// Creates a reader of a TOML document (see
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
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, u32> =
///     deser_toml::from_reader(&b"a = 1"[..]).unwrap();
/// assert_eq!(value["a"], 1);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
