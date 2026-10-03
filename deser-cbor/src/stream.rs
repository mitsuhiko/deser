//! Reading CBOR streams.
#[cfg(feature = "io")]
use std::io::Read;

use alloc::vec::Vec;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame, Progress};
use deser_core::{Error, ErrorKind, State};

use crate::de::{Deserializer, DeserializerConfig};
use crate::parser::{Copying, Discard, Parser, Progress as ParseProgress};

const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;
const MAJOR_SIMPLE: u8 = 7;
const INDEFINITE: u8 = 31;

/// The state of a CBOR stream that is read.
#[derive(Default)]
struct StreamState {
    // the position up to which the item was scanned
    pos: usize,
    // the number of items the open containers (and tags) still need, `None`
    // for indefinite lengths.
    stack: Vec<Option<u64>>,
    // an item was not well-formed
    failed: bool,
    // parses items while their input arrives (see `drive_partial`)
    parser: Parser,
    // the driver of the current item was set up
    started: bool,
    // the rest of an item that failed in a sink is skipped from the
    // position in the input
    skipping: Option<usize>,
    // parsing failed, the stream cannot be continued
    partial_failed: bool,
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

/// Reads a stream of CBOR data items into items (see [`deser::stream`](deser_core::stream)).
///
/// The stream is a [CBOR sequence](https://www.rfc-editor.org/rfc/rfc8742)
/// of data items that follow each other.  An item is complete once its last
/// byte was read.  Reading continues after items that fail to deserialize,
/// items that are not well-formed end the stream.
///
/// Items which do not borrow are deserialized while their input arrives
/// (see
/// [`StreamDeserializer::drive_partial`](de::StreamDeserializer::drive_partial))
/// so only incomplete data items (like strings) are buffered.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_cbor::DeserializerConfig;
///
/// let mut reader =
///     DeserializerConfig::new().reader(&[0x01, 0x62, b'h', b'i'][..]);
/// assert_eq!(reader.read::<u32>().unwrap(), Some(1));
/// assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("hi"));
/// assert_eq!(reader.read::<u32>().unwrap(), None);
/// # }
/// ```
///
/// Items are parsed like with a [`Deserializer`], so they can borrow from
/// the stream's buffer (see
/// [`InputBuffer::deserialize`](deser_core::stream::InputBuffer::deserialize)).
#[derive(Debug)]
pub struct StreamDeserializer {
    config: DeserializerConfig,
    state: StreamState,
}

impl Default for StreamDeserializer {
    fn default() -> StreamDeserializer {
        StreamDeserializer::new()
    }
}

impl StreamDeserializer {
    /// Creates a stream deserializer.
    pub fn new() -> StreamDeserializer {
        StreamDeserializer::with_config(DeserializerConfig::new())
    }

    /// Creates a stream deserializer with the given configuration.
    pub fn with_config(config: DeserializerConfig) -> StreamDeserializer {
        StreamDeserializer {
            config,
            state: StreamState::default(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Skips the rest of an item that failed in a sink.
    ///
    /// Returns where the next item starts, or the progress if there is no
    /// item (yet).
    fn skip_to_item(
        &mut self,
        input: &[u8],
        offset: usize,
        eof: bool,
    ) -> Result<Result<usize, Progress>, Error> {
        let state = &mut self.state;
        if state.ended {
            return Ok(Err(Progress::End));
        }
        if state.partial_failed {
            return Err(Error::new(
                ErrorKind::InvalidState,
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
                    return Ok(Err(Progress::NeedMore { consumed }));
                }
                Err(err) => {
                    state.skipping = None;
                    return Err(fail(state, err, eof));
                }
            }
        }

        if !state.started && pos == input.len() {
            return Ok(Err(if eof {
                Progress::End
            } else {
                Progress::NeedMore { consumed: pos }
            }));
        }
        Ok(Ok(pos))
    }
}

impl de::StreamDeserializer for StreamDeserializer {
    fn context(&self) -> deser_core::Context {
        self.config.context().clone()
    }

    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        let state = &mut self.state;
        if state.failed {
            return Err(Error::new(
                ErrorKind::InvalidState,
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

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut de = Deserializer::from_slice_with_config(frame, self.config.clone());
        de.drive(driver)?;
        de.end()
    }

    fn supports_partial(&self) -> bool {
        true
    }

    fn drive_partial(
        &mut self,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        let pos = match self.skip_to_item(input, offset, eof)? {
            Ok(pos) => pos,
            Err(progress) => return Ok(progress),
        };
        let state = &mut self.state;
        state.started = true;
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

    fn peek(&mut self, input: &[u8], eof: bool) -> Result<Option<Progress>, Error> {
        Ok(Some(match self.skip_to_item(input, 0, eof)? {
            Ok(pos) => Progress::Done { consumed: pos },
            Err(progress) => progress,
        }))
    }
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of a stream of data items (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// See [`StreamDeserializer`] for how the stream is read.
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self.clone()))
    }

    /// Deserializes a data item from a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, StreamDeserializer::with_config(self.clone()))
    }
}

/// Deserializes a data item from a reader.
///
/// The reader is read to the end, no data may follow the item.  The reader
/// does not need to be buffered.  To read more than one item (a CBOR
/// sequence) use [`DeserializerConfig::reader`].
///
/// ```
/// let value: Vec<u32> =
///     deser_cbor::from_reader(&[0x82, 0x01, 0x02][..]).unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Ends the stream after an error that cannot be recovered from.
fn fail(state: &mut StreamState, err: Error, eof: bool) -> Error {
    state.parser.reset();
    state.partial_failed = true;
    // after an incomplete item at the end there are no more items
    state.ended = eof && err.kind() == ErrorKind::EndOfFile;
    err
}
