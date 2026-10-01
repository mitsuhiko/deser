//! Reading property lists from streams.
#[cfg(feature = "io")]
use std::io::Read;

use deser_core::Error;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};

use crate::de::{Deserializer, DeserializerConfig};

/// Reads a property list from a stream (see
/// [`deser::stream`](deser_core::stream)).
///
/// Property lists cannot be split: binary property lists end with the
/// table of their objects.  The stream is a single property list which is
/// parsed once the whole stream was read.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_plist::DeserializerConfig;
///
/// let mut reader = DeserializerConfig::new().reader(&b"(1, 2)"[..]);
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
/// # }
/// ```
#[derive(Debug, Default)]
pub struct StreamDeserializer {
    config: DeserializerConfig,
    // the property list was read
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
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of a property list (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// See [`StreamDeserializer`].
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self))
    }

    /// Deserializes a property list from a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, StreamDeserializer::with_config(self))
    }
}

/// Deserializes a property list from a reader.
///
/// The reader is read to the end, it does not need to be buffered.  The
/// format is detected automatically.
///
/// ```
/// let value: Vec<u32> = deser_plist::from_reader(&b"(1, 2)"[..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
