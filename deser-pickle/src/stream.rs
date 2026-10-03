//! Reading values from streams.
#[cfg(feature = "io")]
use std::io::Read;

#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};
use deser_core::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::vm;

/// Reads pickles from a stream (see [`deser::stream`](deser_core::stream)).
///
/// A stream holds pickles one after another (what `pickle.dump` writes to
/// a file when it's called more than once).  A pickle is read up to its
/// `STOP` opcode before it's run, the rest of the stream is not waited for.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_pickle::DeserializerConfig;
///
/// let mut reader = DeserializerConfig::new().reader(&b"K\x01.\x8c\x03two."[..]);
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
        match vm::find_end(input, 0)? {
            Some(end) => Ok(Frame::Value {
                start: 0,
                end,
                consumed: end,
            }),
            None if !eof => Ok(Frame::Incomplete { consumed: 0 }),
            None if input.is_empty() => Ok(Frame::End),
            None => Err(Error::with_offset(
                ErrorKind::EndOfFile,
                "unexpected end of input",
                input.len(),
            )),
        }
    }

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_slice_with_config(frame, self.config.clone()).drive(driver)
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

/// Deserializes a pickle from a reader.
///
/// The reader does not need to be buffered.  The stream must contain
/// exactly one pickle.
///
/// ```
/// let value: Vec<u32> = deser_pickle::from_reader(&b"\x80\x04](K\x01K\x02e."[..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
