//! Reading and writing CBOR streams.
use std::io::{Read, Write};

use deser::de::{Decoder, Frame, Limits, Progress};
use deser::de::{Deserialize, DeserializeDriver, DeserializeOwned, Format};
use deser::ser::Encoder;
use deser::ser::{Serialize, SerializeDriver};
use deser::{Error, ErrorKind, State};

use crate::de::{Deserializer, DeserializerConfig};
use crate::parser::{Copying, Discard, Parser, Progress as ParseProgress};
use crate::ser::SerializerConfig;

const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;
const MAJOR_SIMPLE: u8 = 7;
const INDEFINITE: u8 = 31;

/// The state of a CBOR stream that is read.
///
/// See [`Decoder::State`].
#[derive(Default)]
pub struct StreamState {
    // the position up to which the item was scanned
    pos: usize,
    // the number of items the open containers (and tags) still need, `None`
    // for indefinite lengths.
    stack: Vec<Option<u64>>,
    // an item was not well-formed
    failed: bool,
    // parses items incrementally (see `Decoder::feed`)
    parser: Parser,
    // the driver of the current item was set up
    started: bool,
    // the rest of an item that failed in a sink is skipped from the
    // position in the input
    skipping: Option<usize>,
    // parsing failed, the stream cannot be continued
    feed_failed: bool,
    // the stream ended within an item
    ended: bool,
}

impl std::fmt::Debug for StreamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamState").finish_non_exhaustive()
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

impl StreamState {
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

/// Splits a stream of CBOR data items into items (see [`deser::io`]).
///
/// The stream is a [CBOR sequence](https://www.rfc-editor.org/rfc/rfc8742)
/// of data items that follow each other.  An item is complete once its last
/// byte was read.  Reading continues after items that fail to deserialize,
/// items that are not well-formed end the stream.
///
/// Items which do not borrow are deserialized while their input arrives
/// (see [`Decoder::feed`]) so only incomplete data items (like strings)
/// are buffered.
///
/// ```
/// use deser::io::Reader;
/// use deser_cbor::DeserializerConfig;
///
/// let mut reader = Reader::new(&[0x01, 0x62, b'h', b'i'][..], DeserializerConfig::new());
/// assert_eq!(reader.read::<u32>().unwrap(), Some(1));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("hi"));
/// assert_eq!(reader.read::<u32>().unwrap(), None);
/// ```
///
/// Items are parsed like with a [`Deserializer`], so they can borrow from
/// the stream's buffer (see [`deser::io::Reader::read_borrowed`]).
impl Decoder for DeserializerConfig {
    type State = StreamState;

    fn frame(&self, state: &mut StreamState, input: &[u8], eof: bool) -> Result<Frame, Error> {
        if state.failed {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "cannot continue after an item that is not well-formed",
            ));
        }
        if input.is_empty() && eof {
            return Ok(Frame::End);
        }
        let end = match state.scan(input) {
            Scan::Complete(end) => end,
            Scan::Incomplete if !eof => return Ok(Frame::Incomplete { consumed: 0 }),
            // the parser reports the error
            Scan::Incomplete => input.len(),
            Scan::Malformed(offset) => {
                state.failed = true;
                offset + 1
            }
        };
        state.pos = 0;
        state.stack.clear();
        Ok(Frame::Value {
            start: 0,
            end,
            consumed: end,
        })
    }

    fn drive<'de>(
        &self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut de = Deserializer::from_slice_with_config(frame, self);
        de.drive(driver)?;
        de.end()
    }

    fn supports_feed(&self) -> bool {
        true
    }

    fn feed(
        &self,
        state: &mut StreamState,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        if state.ended {
            return Ok(Progress::End);
        }
        if state.feed_failed {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "cannot continue after an error",
            ));
        }

        // skip the rest of an item that failed in a sink
        let mut pos = 0;
        if let Some(skip) = state.skipping {
            let mut discard = Discard(State::new());
            match state.parser.parse(input, skip, eof, offset, &mut discard) {
                Ok(ParseProgress::Done(end)) => {
                    state.skipping = None;
                    pos = end;
                }
                Ok(ParseProgress::NeedMore(consumed)) => {
                    state.skipping = Some(0);
                    return Ok(Progress::NeedMore { consumed });
                }
                Err(err) => {
                    state.skipping = None;
                    return Err(fail(state, err, eof));
                }
            }
        }

        if !state.started {
            if pos == input.len() {
                return Ok(if eof {
                    Progress::End
                } else {
                    Progress::NeedMore { consumed: pos }
                });
            }
            if let Some(max_depth) = self.max_depth_limit() {
                driver.push_layer(Limits::new().max_depth(max_depth));
            }
            state.started = true;
        }
        match state
            .parser
            .parse(input, pos, eof, offset, &mut Copying(driver))
        {
            Ok(ParseProgress::Done(end)) => {
                state.started = false;
                Ok(Progress::Done { consumed: end })
            }
            Ok(ParseProgress::NeedMore(consumed)) => Ok(Progress::NeedMore { consumed }),
            Err(err) => {
                state.started = false;
                match state.parser.recoverable() {
                    // a sink failed, the next call continues after the
                    // item.  The input is not consumed on errors.
                    Some(resume) => {
                        state.skipping = Some(resume);
                        Err(err)
                    }
                    None => Err(fail(state, err, eof)),
                }
            }
        }
    }

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        let mut de = Deserializer::from_slice_with_config(input, self);
        let rv = de.deserialize_with(setup)?;
        de.end()?;
        Ok(rv)
    }
}

/// Writes CBOR data items to a stream (see [`deser::io`]).
///
/// The items follow each other which makes the stream a [CBOR
/// sequence](https://www.rfc-editor.org/rfc/rfc8742).
///
/// ```
/// use deser::io::Writer;
/// use deser_cbor::SerializerConfig;
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
/// writer.write(&1u32).unwrap();
/// writer.write(&"hi").unwrap();
/// assert_eq!(writer.into_inner(), [0x01, 0x62, b'h', b'i']);
/// ```
impl Encoder for SerializerConfig {
    fn encode(
        &self,
        driver: &mut SerializeDriver<'_>,
        _index: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let bytes = self.serialize_driver(driver)?;
        out.extend_from_slice(&bytes);
        Ok(())
    }
}

impl DeserializerConfig {
    /// Deserializes a data item from a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser::io::from_reader(reader, self)
    }
}

impl SerializerConfig {
    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser::io::to_writer(writer, self, value)
    }
}

/// Deserializes a data item from a reader.
///
/// The reader is read to the end, no data may follow the item.  The reader
/// does not need to be buffered.  To read more than one item (a CBOR
/// sequence) use a [`deser::io::Reader`] with a [`DeserializerConfig`].
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

/// Ends the stream after an error that cannot be recovered from.
fn fail(state: &mut StreamState, err: Error, eof: bool) -> Error {
    state.parser.reset();
    state.feed_failed = true;
    // after an incomplete item at the end there are no more items
    state.ended = eof && err.kind() == ErrorKind::EndOfFile;
    err
}
