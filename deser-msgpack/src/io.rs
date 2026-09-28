//! Reading and writing MessagePack streams.
use std::io::{Read, Write};

use deser_core::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser_core::io::Encoder;
use deser_core::io::{Decoder, Frame, Progress};
use deser_core::ser::{Serialize, SerializeDriver};
use deser_core::{Error, ErrorKind, State};

use crate::de::{Deserializer, DeserializerConfig};
use crate::head::{Head, HeadError, decode_head};
use crate::parser::{Copying, Discard, Parser, Progress as ParseProgress};
use crate::ser::SerializerConfig;

/// The state of a MessagePack stream that is read.
///
/// See [`Decoder::State`].
#[derive(Default)]
pub struct StreamState {
    // the position up to which the item was scanned
    pos: usize,
    // the number of items the open containers still need
    stack: Vec<u64>,
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

impl core::fmt::Debug for StreamState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
            let (head, len) = match decode_head(&input[head_start..]) {
                Ok(rv) => rv,
                Err(HeadError::Incomplete) => return Scan::Incomplete,
                Err(HeadError::Reserved) => return Scan::Malformed(head_start),
            };
            let mut pos = head_start + len;
            let items = match head {
                Head::Str(len) | Head::Bin(len) | Head::Ext(_, len) => {
                    match pos.checked_add(len as usize) {
                        Some(end) if end <= input.len() => pos = end,
                        _ => return Scan::Incomplete,
                    }
                    0
                }
                Head::Array(len) => u64::from(len),
                Head::Map(len) => u64::from(len) * 2,
                _ => 0,
            };
            self.pos = pos;

            if items > 0 {
                self.stack.push(items);
                continue;
            }
            // account for the item in the containers it completes
            loop {
                match self.stack.last_mut() {
                    None => return Scan::Complete(pos),
                    Some(remaining) => {
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

/// Splits a stream of MessagePack items into items (see [`deser::io`](deser_core::io)).
///
/// The items of the stream follow each other without separators.  An item
/// is complete once its last byte was read.  Reading continues after items that fail to deserialize,
/// items that are not well-formed end the stream.
///
/// Items which do not borrow are deserialized while their input arrives
/// (see [`Decoder::feed`]) so only incomplete atoms (like strings) are
/// buffered.
///
/// ```
/// use deser::io::Reader;
/// use deser_msgpack::DeserializerConfig;
///
/// let mut reader =
///     Reader::new(&[0x01, 0xa2, b'h', b'i'][..], DeserializerConfig::new());
/// assert_eq!(reader.read::<u32>().unwrap(), Some(1));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("hi"));
/// assert_eq!(reader.read::<u32>().unwrap(), None);
/// ```
///
/// Items are parsed like with a [`Deserializer`], so they can borrow from
/// the stream's buffer (see [`deser::io::Reader::read_borrowed`](deser_core::io::Reader::read_borrowed)).
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
        _state: &mut Self::State,
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

/// Writes MessagePack items to a stream (see [`deser::io`](deser_core::io)).
///
/// The items follow each other without separators.
///
/// ```
/// use deser::io::Writer;
/// use deser_msgpack::SerializerConfig;
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
/// writer.write(&1u32).unwrap();
/// writer.write(&"hi").unwrap();
/// assert_eq!(writer.into_inner(), [0x01, 0xa2, b'h', b'i']);
/// ```
impl Encoder for SerializerConfig {
    type State = ();

    fn encode(
        &self,
        _state: &mut (),
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let bytes = self.serialize_driver(driver)?;
        out.extend_from_slice(&bytes);
        Ok(())
    }
}

impl DeserializerConfig {
    /// Deserializes an item from a reader.
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

/// Deserializes an item from a reader.
///
/// The reader is read to the end, no data may follow the item.  The reader
/// does not need to be buffered.  To read more than one item use a [`deser::io::Reader`](deser_core::io::Reader) with a [`DeserializerConfig`].
///
/// ```
/// let value: Vec<u32> =
///     deser_msgpack::from_reader(&[0x92, 0x01, 0x02][..]).unwrap();
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
/// deser_msgpack::to_writer(&mut out, &vec![1u32, 2]).unwrap();
/// assert_eq!(out, [0x92, 0x01, 0x02]);
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
