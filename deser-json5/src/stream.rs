// @generated from deser-template-json/src/stream.rs by
// deser-template-json/generate.py.  Do not edit.
//! Reading JSON streams.
#[cfg(feature = "io")]
use std::io::Read;

#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame, Progress};
use deser_core::{Error, ErrorKind, State};

use crate::Trailing;
use crate::de::{Deserializer, DeserializerConfig};
use crate::parser::Cursor;
use crate::parser::{Copying, Discard, Options, Parser, Progress as ParseProgress};
use crate::scan::LineScan;
use crate::scan::skip_to_escape;
use crate::scan::skip_to_escape_single;

/// Returns `true` for whitespace (the ASCII characters, not the Unicode
/// whitespace).
fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\t' | b'\r' | 0x0b | 0x0c)
}

/// Returns where the next token starts and if it's there.
///
/// If the input ends within a comment (and more input follows), this is
/// where the comment starts so that it's scanned again with more input,
/// the token is not there.
fn skip_whitespace(input: &[u8], pos: usize, eof: bool) -> (usize, bool) {
    let mut cursor = Cursor::new_partial(input, pos, eof);
    let token = cursor.parse_whitespace().is_some();
    (cursor.pos, token)
}

/// The state of a JSON stream that is read.
#[derive(Debug, Default)]
struct StreamState {
    // `Trailing::Strict`: the value was read
    done: bool,
    // parses values while their input arrives (see `drive_partial`)
    parser: Parser,
    // the rest of a value that failed in a sink is skipped from the
    // position in the input
    skipping: Option<usize>,
    // parsing failed, the stream cannot be continued
    failed: bool,
    // the stream ended within a value
    ended: bool,
    // the position up to which the input was scanned
    pos: usize,
    // `Trailing::Newline`: the scan of the current line
    line: LineScan,
    // `Trailing::Stop`: the value being scanned
    value: Option<Value>,
}

/// The state of the scan of a value.
#[derive(Debug)]
struct Value {
    start: usize,
    kind: ValueKind,
    depth: usize,
    in_string: bool,
    // the string is in single quotes
    single: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    /// A number or literal.
    Scalar,
    /// A string, map or sequence.
    Structure,
}

fn frame_all(state: &mut StreamState, input: &[u8], eof: bool) -> Result<Frame, Error> {
    if state.done {
        // only whitespace may follow the value
        return trailing_whitespace(input, 0, eof).map(|progress| match progress {
            Progress::End => Frame::End,
            // an incomplete comment is scanned again with more input
            Progress::NeedMore { consumed } => Frame::Incomplete { consumed },
            Progress::Done { .. } => unreachable!(),
        });
    }
    if !eof {
        return Ok(Frame::Incomplete { consumed: 0 });
    }
    // an unterminated comment is reported by the parser
    let (start, _) = skip_whitespace(input, 0, eof);
    Ok(if start < input.len() {
        state.done = true;
        Frame::Value {
            start,
            end: input.len(),
            consumed: input.len(),
        }
    } else {
        Frame::End
    })
}

/// Checks that only whitespace follows a value.
///
/// `offset` is the offset of the input in the stream for errors.
fn trailing_whitespace(input: &[u8], offset: usize, eof: bool) -> Result<Progress, Error> {
    match skip_whitespace(input, 0, eof) {
        (pos, _) if pos == input.len() && eof => Ok(Progress::End),
        // an incomplete comment is scanned again with more input
        (pos, false) if !eof => Ok(Progress::NeedMore { consumed: pos }),
        (pos, _) => {
            Err(Error::new(ErrorKind::Syntax, "garbage after input").with_offset(offset + pos))
        }
    }
}

fn frame_line(state: &mut StreamState, input: &[u8], eof: bool) -> Frame {
    // line breaks in comments and strings do not end the line
    let end = state.line.find_end(input, state.pos);
    let end = match end {
        Some(end) => end,
        None if eof => input.len(),
        None => {
            state.pos = input.len();
            return Frame::Incomplete { consumed: 0 };
        }
    };
    state.pos = 0;
    state.line = LineScan::default();
    let consumed = (end + 1).min(input.len());
    match skip_whitespace(&input[..end], 0, true) {
        (start, _) if start < end => Frame::Value {
            start,
            end,
            consumed,
        },
        _ if consumed == 0 => Frame::End,
        // blank lines are skipped
        _ => Frame::Incomplete { consumed },
    }
}

fn frame_value(state: &mut StreamState, input: &[u8], eof: bool) -> Frame {
    let value = match state.value {
        Some(ref mut value) => value,
        None => {
            let start = match skip_whitespace(input, 0, eof) {
                (start, true) => start,
                (_, false) if input.is_empty() && eof => return Frame::End,
                (consumed, false) => return Frame::Incomplete { consumed },
            };
            let (kind, depth, in_string) = match input[start] {
                b'"' => (ValueKind::Structure, 0, true),
                b'\'' => (ValueKind::Structure, 0, true),
                b'{' | b'[' => (ValueKind::Structure, 1, false),
                // a value cannot start with these, the parser reports the
                // error
                b'}' | b']' | b',' | b':' => {
                    return Frame::Value {
                        start,
                        end: start + 1,
                        consumed: start + 1,
                    };
                }
                _ => (ValueKind::Scalar, 0, false),
            };
            state.pos = start + 1;
            state.value.insert(Value {
                start,
                kind,
                depth,
                in_string,
                single: input[start] == b'\'',
            })
        }
    };

    let end = match value.kind {
        ValueKind::Scalar => {
            let end = input[state.pos..].iter().position(|&b| match b {
                b'{' | b'}' | b'[' | b']' | b',' | b':' | b'"' => true,
                // a comment
                b'/' => true,
                b'\'' => true,
                // scalars are ASCII, this is Unicode whitespace
                0x80..=0xff => true,
                _ => is_whitespace(b),
            });
            match end {
                Some(index) => Some(state.pos + index),
                None => {
                    state.pos = input.len();
                    None
                }
            }
        }
        ValueKind::Structure => scan_structure(input, &mut state.pos, value),
    };

    let start = value.start;
    match end {
        Some(end) => {
            state.value = None;
            state.pos = 0;
            Frame::Value {
                start,
                end,
                consumed: end,
            }
        }
        // the value is incomplete, the parser reports the error
        None if eof => {
            state.value = None;
            state.pos = 0;
            Frame::Value {
                start,
                end: input.len(),
                consumed: input.len(),
            }
        }
        None => {
            // discard the whitespace before the value
            value.start = 0;
            state.pos -= start;
            Frame::Incomplete { consumed: start }
        }
    }
}

/// Scans a string, map or sequence from `pos`.
///
/// Returns the end of the value if it's complete.
fn scan_structure(input: &[u8], pos: &mut usize, value: &mut Value) -> Option<usize> {
    let mut index = *pos;
    while index < input.len() {
        if value.in_string {
            index = if value.single {
                skip_to_escape_single(input, index)
            } else {
                skip_to_escape(input, index)
            };
            let byte = input.get(index).copied();
            // the closing single quote is handled like a double quote
            let byte = if value.single && byte == Some(b'\'') {
                Some(b'"')
            } else {
                byte
            };
            match byte {
                Some(b'"') => {
                    value.in_string = false;
                    index += 1;
                    if value.depth == 0 {
                        return Some(index);
                    }
                }
                Some(b'\\') => {
                    // the escaped byte is scanned again with more input
                    if index + 1 >= input.len() {
                        break;
                    }
                    index += 2;
                }
                // control characters are reported by the parser
                Some(_) => index += 1,
                None => break,
            }
        } else {
            match input[index] {
                b'"' => {
                    value.in_string = true;
                    value.single = false;
                }
                b'\'' => {
                    value.in_string = true;
                    value.single = true;
                }
                // comments are skipped, incomplete ones are scanned again
                // with more input
                b'/' => {
                    match skip_whitespace(input, index, false) {
                        // not a comment, the parser reports the error
                        (next, true) if next == index => index += 1,
                        (next, token) => {
                            index = next;
                            if !token && next < input.len() {
                                break;
                            }
                        }
                    }
                    continue;
                }
                b'{' | b'[' => value.depth += 1,
                b'}' | b']' => {
                    value.depth -= 1;
                    if value.depth == 0 {
                        return Some(index + 1);
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
    *pos = index;
    None
}

/// Reads a stream of JSON values (see [`deser::stream`](deser_core::stream)).
///
/// How the stream is split depends on [`DeserializerConfig::trailing`]:
///
/// * [`Trailing::Strict`]: the stream holds a single value which is parsed
///   once the whole stream was read.
/// * [`Trailing::Newline`]: every line holds a value ([JSON
///   Lines](https://jsonlines.org/)).  Blank lines are skipped and errors
///   only discard their line, reading continues with the next one.
/// * [`Trailing::Stop`]: values follow each other (optionally separated by
///   whitespace) and are split where they end.  Numbers at the end of the
///   stream are only complete at the end of the stream, other values are
///   complete once their last byte was read.  Reading continues after
///   values that fail to deserialize, values that are not valid JSON end
///   the stream (unless they are read from their frames).
///
/// Except for JSON Lines, values which do not borrow are deserialized while
/// their input arrives (see
/// [`StreamDeserializer::drive_partial`](de::StreamDeserializer::drive_partial))
/// so only incomplete tokens are buffered.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_json5::{DeserializerConfig, Trailing};
///
/// const LINES: DeserializerConfig =
///     DeserializerConfig::new().trailing(Trailing::Newline);
/// let mut reader = LINES.reader(&b"[1, 2]\n[3]\n"[..]);
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![3]));
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
/// # }
/// ```
///
/// Values which are read from their frames are parsed like with a
/// [`Deserializer`], so they can borrow from the stream's buffer (see
/// [`InputBuffer::deserialize`](deser_core::stream::InputBuffer::deserialize)).  The input ranges (and thus
/// locations) of these values refer to the start of their line (or value),
/// those of values that are deserialized while their input arrives to the
/// stream.
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
        StreamDeserializer::with_config(&DeserializerConfig::new())
    }

    /// Creates a stream deserializer with the given configuration.
    pub fn with_config(config: &DeserializerConfig) -> StreamDeserializer {
        StreamDeserializer {
            config: config.clone(),
            state: StreamState::default(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Skips what precedes the next value.
    ///
    /// This skips the rest of a value that failed in a sink and the
    /// whitespace before the next value.  Returns where the value starts,
    /// or the progress if there is no value (yet).
    fn skip_to_value(
        &mut self,
        input: &[u8],
        offset: usize,
        eof: bool,
    ) -> Result<Result<usize, Progress>, Error> {
        let options = self.options();
        let state = &mut self.state;
        if state.ended {
            return Ok(Err(Progress::End));
        }
        if state.failed {
            return Err(Error::new(
                ErrorKind::InvalidState,
                "cannot continue after an error",
            ));
        }

        // skip the rest of a value that failed in a sink
        let mut pos = 0;
        if let Some(skip) = state.skipping {
            let mut discard = Discard(State::new());
            match state
                .parser
                .parse(input, skip, eof, offset, options, &mut discard)
            {
                Ok(ParseProgress::Done(end)) => {
                    state.skipping = None;
                    state.done = self.config.trailing_mode() == Trailing::Strict;
                    pos = end;
                }
                Ok(ParseProgress::NeedMore(consumed)) => {
                    state.skipping = Some(0);
                    return Ok(Err(Progress::NeedMore { consumed }));
                }
                Err(err) => {
                    state.parser.reset();
                    state.skipping = None;
                    state.failed = true;
                    return Err(err);
                }
            }
        }

        if !state.parser.is_idle() {
            return Ok(Ok(pos));
        }
        if state.done {
            return match trailing_whitespace(&input[pos..], offset + pos, eof)? {
                Progress::NeedMore { consumed } => Ok(Err(Progress::NeedMore {
                    consumed: pos + consumed,
                })),
                progress => Ok(Err(progress)),
            };
        }
        // a new value, skip the whitespace before it
        // (an incomplete comment is scanned again with more input)
        let token;
        (pos, token) = skip_whitespace(input, pos, eof);
        if !token && (pos == input.len() || !eof) {
            return Ok(Err(if eof {
                Progress::End
            } else {
                Progress::NeedMore { consumed: pos }
            }));
        }
        Ok(Ok(pos))
    }

    /// Returns the options of the parser.
    fn options(&self) -> Options {
        Options {
            validate_utf8: true,
            exact_numbers: self.config.exact_numbers_enabled(),
        }
    }
}

impl de::StreamDeserializer for StreamDeserializer {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        let state = &mut self.state;
        match self.config.trailing_mode() {
            Trailing::Strict => frame_all(state, input, eof),
            Trailing::Newline => Ok(frame_line(state, input, eof)),
            Trailing::Stop => Ok(frame_value(state, input, eof)),
        }
    }

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_frame(frame, &self.config).drive(driver)
    }

    fn is_text(&self) -> bool {
        true
    }

    /// JSON Lines are read line by line, the other values while their
    /// input arrives.
    fn supports_partial(&self) -> bool {
        self.config.trailing_mode() != Trailing::Newline
    }

    fn drive_partial(
        &mut self,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        let pos = match self.skip_to_value(input, offset, eof)? {
            Ok(pos) => pos,
            Err(progress) => return Ok(progress),
        };
        let options = self.options();
        let state = &mut self.state;
        match state
            .parser
            .parse(input, pos, eof, offset, options, &mut Copying(driver))
        {
            Ok(ParseProgress::Done(end)) => {
                state.done = self.config.trailing_mode() == Trailing::Strict;
                Ok(Progress::Done { consumed: end })
            }
            Ok(ParseProgress::NeedMore(consumed)) => Ok(Progress::NeedMore { consumed }),
            Err(err) => {
                if let Some(resume) = state.parser.recoverable() {
                    // a sink failed, the next call continues after the
                    // value.  The input is not consumed on errors.
                    state.skipping = Some(resume);
                } else {
                    state.parser.reset();
                    // after an incomplete value at the end there are no
                    // more values
                    state.ended = eof && err.kind() == ErrorKind::EndOfFile;
                    state.failed = true;
                }
                Err(err)
            }
        }
    }

    fn peek(&mut self, input: &[u8], eof: bool) -> Result<Option<Progress>, Error> {
        if !de::StreamDeserializer::supports_partial(self) {
            return Ok(None);
        }
        Ok(Some(match self.skip_to_value(input, 0, eof)? {
            Ok(pos) => Progress::Done { consumed: pos },
            Err(progress) => progress,
        }))
    }
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of a stream of values (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// See [`StreamDeserializer`] for how the stream is split into values.
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self))
    }

    /// Deserializes a value from a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        deser_core::io::from_reader(reader, StreamDeserializer::with_config(self))
    }
}

/// Deserializes a value from a reader.
///
/// The reader is read to the end.  Only whitespace may follow the value.
/// The reader does not need to be buffered.  To read more than one value
/// (for instance JSON Lines) use [`DeserializerConfig::reader`].
///
/// ```
/// let value: Vec<u32> =
///     deser_json5::from_reader(&b"[1, 2, 3]"[..]).unwrap();
/// assert_eq!(value, [1, 2, 3]);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
