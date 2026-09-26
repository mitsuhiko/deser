//! Reading and writing TOML from and to streams.
use std::io::{Read, Write};

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::io::Frame;
use deser::ser::Serialize;
use deser::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::SerializerConfig;

/// Reads a TOML document from a stream (see [`deser::io`]).
///
/// TOML documents cannot be split: tables can be extended anywhere in the
/// document.  The stream is a single document which is parsed once the
/// whole stream was read.  An empty stream is an empty table.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::io::Reader;
///
/// let mut reader = Reader::new(&b"a = 1\nb = 2\n"[..], deser_toml::Decoder::default());
/// let value: BTreeMap<String, u32> = reader.read().unwrap().unwrap();
/// assert_eq!(value["b"], 2);
/// assert!(reader.read::<BTreeMap<String, u32>>().unwrap().is_none());
/// ```
pub struct Decoder {
    config: DeserializerConfig,
    done: bool,
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
            done: false,
        }
    }
}

impl deser::io::Decoder for Decoder {
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

    fn drive<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_slice_with_config(frame, &self.config).drive(driver)
    }
}

/// Writes a TOML document to a stream (see [`deser::io`]).
///
/// A stream holds a single document, writing a second value fails.
pub struct Encoder {
    config: SerializerConfig,
    done: bool,
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
            config: config.clone(),
            done: false,
        }
    }
}

impl deser::io::Encoder for Encoder {
    fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error> {
        if self.done {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "a TOML stream holds a single document",
            ));
        }
        let toml = self.config.to_string(value)?;
        out.extend_from_slice(toml.as_bytes());
        self.done = true;
        Ok(())
    }
}

impl DeserializerConfig {
    /// Creates a [`Decoder`] to read a stream with this configuration.
    pub fn decoder(&self) -> Decoder {
        Decoder::new(self)
    }

    /// Deserializes a document from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser::io::from_reader(reader, self.decoder())
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

/// Deserializes a document from a reader.
///
/// The reader is read to the end, it does not need to be buffered.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, u32> = deser_toml::from_reader(&b"a = 1"[..]).unwrap();
/// assert_eq!(value["a"], 1);
/// ```
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes a value to a writer.
///
/// The document is written with a single write.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let mut out = Vec::new();
/// deser_toml::to_writer(&mut out, &BTreeMap::from([("a", 1)])).unwrap();
/// assert_eq!(out, b"a = 1\n");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
