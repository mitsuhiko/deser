//! Reading and writing CBOR streams.
use std::io::{Read, Write};

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::io::Frame;
use deser::ser::Serialize;
use deser::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig};
use crate::ser::SerializerConfig;

const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;
const MAJOR_SIMPLE: u8 = 7;
const INDEFINITE: u8 = 31;

/// Splits a stream of CBOR data items into items (see [`deser::io`]).
///
/// The stream is a [CBOR sequence](https://www.rfc-editor.org/rfc/rfc8742)
/// of data items that follow each other.  An item is complete once its last
/// byte was read.  Reading continues after items that fail to deserialize,
/// items that are not well-formed end the stream.
///
/// ```
/// use deser::io::Reader;
///
/// let mut reader = Reader::new(&[0x01, 0x62, b'h', b'i'][..], deser_cbor::Decoder::default());
/// assert_eq!(reader.read::<u32>().unwrap(), Some(1));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("hi"));
/// assert_eq!(reader.read::<u32>().unwrap(), None);
/// ```
///
/// Items are parsed like with a [`Deserializer`], so they can borrow from
/// the stream's buffer (see [`deser::io::Reader::read_borrowed`]).
pub struct Decoder {
    config: DeserializerConfig,
    // the position up to which the item was scanned
    pos: usize,
    // the number of items the open containers (and tags) still need, `None`
    // for indefinite lengths.
    stack: Vec<Option<u64>>,
    // an item was not well-formed
    failed: bool,
}

impl Default for Decoder {
    fn default() -> Decoder {
        Decoder::new(&DeserializerConfig::new())
    }
}

/// The result of scanning an item.
enum Scan {
    /// The item ends at the given offset.
    Complete(usize),
    /// More input is needed.
    Incomplete,
    /// The item is not well-formed at the given offset.
    Malformed(usize),
}

impl Decoder {
    /// Creates a decoder with the given configuration.
    pub fn new(config: &DeserializerConfig) -> Decoder {
        Decoder {
            config: config.clone(),
            pos: 0,
            stack: Vec::new(),
            failed: false,
        }
    }

    /// Scans the item from the current position.
    fn scan(&mut self, input: &[u8]) -> Scan {
        loop {
            let head_start = self.pos;
            let Some(&initial) = input.get(head_start) else {
                return Scan::Incomplete;
            };
            let major = initial >> 5;
            let info = initial & 0x1f;
            let arg_len = match info {
                0..=23 => 0,
                24 => 1,
                25 => 2,
                26 => 4,
                27 => 8,
                INDEFINITE => 0,
                _ => return Scan::Malformed(head_start),
            };
            let Some(arg_bytes) = input.get(head_start + 1..head_start + 1 + arg_len) else {
                return Scan::Incomplete;
            };
            let arg = if info < 24 {
                u64::from(info)
            } else {
                arg_bytes
                    .iter()
                    .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
            };
            let mut pos = head_start + 1 + arg_len;

            let item_done = if info == INDEFINITE {
                match major {
                    MAJOR_BYTES | MAJOR_TEXT | MAJOR_ARRAY | MAJOR_MAP => {
                        self.stack.push(None);
                        false
                    }
                    // the end of an indefinite length item
                    MAJOR_SIMPLE => match self.stack.pop() {
                        Some(None) => true,
                        _ => return Scan::Malformed(head_start),
                    },
                    _ => return Scan::Malformed(head_start),
                }
            } else {
                match major {
                    MAJOR_BYTES | MAJOR_TEXT => {
                        let end = usize::try_from(arg)
                            .ok()
                            .and_then(|len| pos.checked_add(len));
                        match end {
                            Some(end) if end <= input.len() => pos = end,
                            Some(_) => return Scan::Incomplete,
                            None => return Scan::Malformed(head_start),
                        }
                        true
                    }
                    MAJOR_ARRAY | MAJOR_MAP | MAJOR_TAG => {
                        let items = match major {
                            MAJOR_MAP => match arg.checked_mul(2) {
                                Some(items) => items,
                                None => return Scan::Malformed(head_start),
                            },
                            MAJOR_TAG => 1,
                            _ => arg,
                        };
                        if items == 0 {
                            true
                        } else {
                            self.stack.push(Some(items));
                            false
                        }
                    }
                    _ => true,
                }
            };
            self.pos = pos;

            if item_done {
                // account for the item in the containers it completes
                loop {
                    match self.stack.last_mut() {
                        None => return Scan::Complete(pos),
                        Some(None) => break,
                        Some(Some(remaining)) => {
                            *remaining -= 1;
                            if *remaining > 0 {
                                break;
                            }
                            self.stack.pop();
                        }
                    }
                }
            }
        }
    }
}

impl deser::io::Decoder for Decoder {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        if self.failed {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "cannot continue after an item that is not well-formed",
            ));
        }
        if input.is_empty() && eof {
            return Ok(Frame::End);
        }
        let end = match self.scan(input) {
            Scan::Complete(end) => end,
            Scan::Incomplete if !eof => return Ok(Frame::Incomplete { consumed: 0 }),
            // the parser reports the error
            Scan::Incomplete => input.len(),
            Scan::Malformed(offset) => {
                self.failed = true;
                offset + 1
            }
        };
        self.pos = 0;
        self.stack.clear();
        Ok(Frame::Value {
            start: 0,
            end,
            consumed: end,
        })
    }

    fn drive<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut de = Deserializer::from_slice_with_config(frame, &self.config);
        de.drive(driver)?;
        de.end()
    }
}

/// Writes CBOR data items to a stream (see [`deser::io`]).
///
/// The items follow each other which makes the stream a [CBOR
/// sequence](https://www.rfc-editor.org/rfc/rfc8742).
///
/// ```
/// use deser::io::Writer;
///
/// let mut writer = Writer::new(Vec::new(), deser_cbor::Encoder::default());
/// writer.write(&1u32).unwrap();
/// writer.write(&"hi").unwrap();
/// assert_eq!(writer.into_inner(), [0x01, 0x62, b'h', b'i']);
/// ```
pub struct Encoder {
    config: SerializerConfig,
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
        }
    }
}

impl deser::io::Encoder for Encoder {
    fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error> {
        let bytes = self.config.to_vec(value)?;
        out.extend_from_slice(&bytes);
        Ok(())
    }
}

impl DeserializerConfig {
    /// Creates a [`Decoder`] to read a stream with this configuration.
    pub fn decoder(&self) -> Decoder {
        Decoder::new(self)
    }

    /// Deserializes a data item from a reader.
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

/// Deserializes a data item from a reader.
///
/// The reader is read to the end, no data may follow the item.  The reader
/// does not need to be buffered.  To read more than one item (a CBOR
/// sequence) use a [`deser::io::Reader`] with a [`Decoder`].
///
/// ```
/// let value: Vec<u32> = deser_cbor::from_reader(&[0x82, 0x01, 0x02][..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes a value to a writer.
///
/// The value is written with a single write.
///
/// ```
/// let mut out = Vec::new();
/// deser_cbor::to_writer(&mut out, &vec![1u32, 2]).unwrap();
/// assert_eq!(out, [0x82, 0x01, 0x02]);
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
