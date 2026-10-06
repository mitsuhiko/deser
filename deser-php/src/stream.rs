//! Reading values from streams.
#[cfg(feature = "io")]
use std::io::Read;

use deser_core::Error;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};

use crate::de::{Deserializer, DeserializerConfig};
use crate::parser;

/// Reads values of PHP's serialization format from a stream (see
/// [`deser::stream`](deser_core::stream)).
///
/// Values are validated before they are emitted, which needs the whole
/// value: the stream is read to the end and then its values (which can be
/// concatenated) are deserialized one after another.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_php::DeserializerConfig;
///
/// let mut reader = DeserializerConfig::new().reader(&b"i:1;s:3:\"two\";"[..]);
/// assert_eq!(reader.read::<u32>().unwrap(), Some(1));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("two"));
/// assert_eq!(reader.read::<String>().unwrap(), None);
/// # }
/// ```
#[derive(Debug, Default)]
pub struct StreamDeserializer {
    config: DeserializerConfig,
}

impl StreamDeserializer {
    /// Creates a stream deserializer.
    pub fn new() -> StreamDeserializer {
        StreamDeserializer::with_config(DeserializerConfig::new())
    }

    /// Creates a stream deserializer with the given configuration.
    pub fn with_config(config: DeserializerConfig) -> StreamDeserializer {
        StreamDeserializer { config }
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
        if !eof {
            return Ok(Frame::Incomplete { consumed: 0 });
        }
        if input.is_empty() {
            return Ok(Frame::End);
        }
        let end = parser::scan(input, 0)?.end;
        Ok(Frame::Value {
            start: 0,
            end,
            consumed: end,
        })
    }

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_slice_with_config(frame, self.config.without_context()).drive(driver)
    }
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of values (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// See [`StreamDeserializer`].
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self.clone()))
    }

    /// Deserializes a value from a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, StreamDeserializer::with_config(self.clone()))
    }
}

/// Deserializes a value from a reader.
///
/// The reader is read to the end, it does not need to be buffered.  The
/// stream must contain exactly one value.
///
/// ```
/// let value: Vec<u32> = deser_php::from_reader(&b"a:2:{i:0;i:1;i:1;i:2;}"[..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
