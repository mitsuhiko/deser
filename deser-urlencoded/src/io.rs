//! Reading and writing query strings and form data from and to streams.
use std::io::{Read, Write};

use deser::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser::io::{Decoder, Encoder, Frame};
use deser::ser::{Serialize, SerializeDriver};
use deser::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::SerializerConfig;

/// Reads form data from a stream (see [`deser::io`]).
///
/// The stream is a single value (like the body of a request) which is
/// parsed once the whole stream was read.  An empty stream is an empty map.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::io::Reader;
/// use deser_urlencoded::DeserializerConfig;
///
/// let mut reader = Reader::new(&b"a=1&b=2"[..], DeserializerConfig::new());
/// let value: BTreeMap<String, u32> = reader.read().unwrap().unwrap();
/// assert_eq!(value["b"], 2);
/// assert!(reader.read::<BTreeMap<String, u32>>().unwrap().is_none());
/// ```
impl Decoder for DeserializerConfig {
    /// `true` once the value was read.
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

/// Writes form data to a stream (see [`deser::io`]).
///
/// A stream holds a single value, writing a second value fails.
impl Encoder for SerializerConfig {
    fn encode(
        &self,
        driver: &mut SerializeDriver<'_>,
        index: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        if index > 0 {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "a form data stream holds a single value",
            ));
        }
        let mut data = String::new();
        self.serialize_driver(driver, &mut data)?;
        out.extend_from_slice(data.as_bytes());
        Ok(())
    }
}

impl DeserializerConfig {
    /// Deserializes form data from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser::io::from_reader(reader, self)
    }
}

impl SerializerConfig {
    /// Serializes a value as form data to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser::io::to_writer(writer, self, value)
    }
}

/// Deserializes form data from a reader.
///
/// The reader is read to the end, it does not need to be buffered.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, u32> = deser_urlencoded::from_reader(&b"a=1"[..]).unwrap();
/// assert_eq!(value["a"], 1);
/// ```
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes a value as form data to a writer.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let mut out = Vec::new();
/// deser_urlencoded::to_writer(&mut out, &BTreeMap::from([("a", 1)])).unwrap();
/// assert_eq!(out, b"a=1");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
