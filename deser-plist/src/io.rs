//! Reading and writing property lists from and to streams.
use std::io::{Read, Write};

use deser_core::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser_core::io::{Decoder, Encoder, Frame};
use deser_core::ser::{Serialize, SerializeDriver};
use deser_core::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::SerializerConfig;

/// Reads a property list from a stream (see [`deser::io`](deser_core::io)).
///
/// Property lists cannot be split: binary property lists end with the
/// table of their objects.  The stream is a single property list which is
/// parsed once the whole stream was read.
///
/// ```
/// use deser::io::Reader;
/// use deser_plist::DeserializerConfig;
///
/// let mut reader = Reader::new(&b"(1, 2)"[..], DeserializerConfig::new());
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
/// ```
impl Decoder for DeserializerConfig {
    /// `true` once the property list was read.
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

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        Deserializer::from_slice_with_config(input, self).deserialize_with(setup)
    }
}

/// Writes a property list to a stream (see [`deser::io`](deser_core::io)).
///
/// A stream holds a single property list, writing a second value fails.
impl Encoder for SerializerConfig {
    /// `true` once the value was written.
    type State = bool;

    fn encode(
        &self,
        written: &mut bool,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        if *written {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "a property list stream holds a single value",
            ));
        }
        out.extend_from_slice(&self.serialize_driver(driver)?);
        *written = true;
        Ok(())
    }
}

impl DeserializerConfig {
    /// Deserializes a property list from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, self)
    }
}

impl SerializerConfig {
    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser_core::io::to_writer(writer, self, value)
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
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes a value to a writer as XML property list.
///
/// The property list is written with a single write.  To write other
/// formats use [`SerializerConfig::to_writer`].
///
/// ```
/// let mut out = Vec::new();
/// deser_plist::to_writer(&mut out, &true).unwrap();
/// assert!(out.ends_with(b"<true/>\n</plist>\n"));
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
