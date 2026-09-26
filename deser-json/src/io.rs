//! Reading and writing JSON streams.
use std::io::{Read, Write};

use deser::de::{Decoder, Frame};
use deser::de::{Deserialize, DeserializeDriver, DeserializeOwned, Format};
use deser::ser::Encoder;
use deser::ser::{Serialize, SerializeDriver};
use deser::{Error, ErrorKind};

use crate::de::{Deserializer, DeserializerConfig, Trailing};
use crate::scan::skip_to_escape;
use crate::ser::SerializerConfig;

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\t' | b'\r')
}

/// The state of a JSON stream that is read.
///
/// See [`Decoder::State`].
#[derive(Debug, Default)]
pub struct StreamState {
    // `Trailing::Strict`: the value was read
    done: bool,
    // the position up to which the input was scanned
    pos: usize,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    /// A number or literal.
    Scalar,
    /// A string, map or sequence.
    Structure,
}

fn frame_all(state: &mut StreamState, input: &[u8], eof: bool) -> Frame {
    if !eof {
        return Frame::Incomplete { consumed: 0 };
    }
    match input.iter().position(|&b| !is_whitespace(b)) {
        Some(start) if !state.done => {
            state.done = true;
            Frame::Value {
                start,
                end: input.len(),
                consumed: input.len(),
            }
        }
        _ => Frame::End,
    }
}

fn frame_line(state: &mut StreamState, input: &[u8], eof: bool) -> Frame {
    let end = match input[state.pos..].iter().position(|&b| b == b'\n') {
        Some(index) => state.pos + index,
        None if eof => input.len(),
        None => {
            state.pos = input.len();
            return Frame::Incomplete { consumed: 0 };
        }
    };
    state.pos = 0;
    let consumed = (end + 1).min(input.len());
    match input[..end].iter().position(|&b| !is_whitespace(b)) {
        Some(start) => Frame::Value {
            start,
            end,
            consumed,
        },
        None if consumed == 0 => Frame::End,
        // blank lines are skipped
        None => Frame::Incomplete { consumed },
    }
}

fn frame_value(state: &mut StreamState, input: &[u8], eof: bool) -> Frame {
    let value = match state.value {
        Some(ref mut value) => value,
        None => {
            let start = match input.iter().position(|&b| !is_whitespace(b)) {
                Some(start) => start,
                None if input.is_empty() && eof => return Frame::End,
                None => {
                    return Frame::Incomplete {
                        consumed: input.len(),
                    };
                }
            };
            let (kind, depth, in_string) = match input[start] {
                b'"' => (ValueKind::Structure, 0, true),
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
            })
        }
    };

    let end = match value.kind {
        ValueKind::Scalar => {
            let end = input[state.pos..].iter().position(|&b| {
                is_whitespace(b) || matches!(b, b'{' | b'}' | b'[' | b']' | b',' | b':' | b'"')
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
            index = skip_to_escape(input, index);
            match input.get(index) {
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
                b'"' => value.in_string = true,
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

/// Splits a JSON stream into values (see [`deser::io`]).
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
///   values that fail to deserialize.
///
/// ```
/// use deser::io::Reader;
/// use deser_json::{DeserializerConfig, Trailing};
///
/// const LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
/// let mut reader = Reader::new(&b"[1, 2]\n[3]\n"[..], LINES);
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1, 2]));
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![3]));
/// assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
/// ```
///
/// Values are parsed like with a [`Deserializer`], so they can borrow from
/// the stream's buffer (see [`deser::io::Reader::read_borrowed`]).  The
/// input ranges (and thus locations) of values refer to the start of their
/// line (or value).
impl Decoder for DeserializerConfig {
    type State = StreamState;

    fn frame(&self, state: &mut StreamState, input: &[u8], eof: bool) -> Result<Frame, Error> {
        Ok(match self.trailing_mode() {
            Trailing::Strict => frame_all(state, input, eof),
            Trailing::Newline => frame_line(state, input, eof),
            Trailing::Stop => frame_value(state, input, eof),
        })
    }

    fn drive<'de>(
        &self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        Deserializer::from_frame(frame, self).drive(driver)
    }

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        Deserializer::from_slice_with_config(input, self).deserialize_with(setup)
    }
}

/// Writes JSON values to a stream (see [`deser::io`]).
///
/// What follows the values depends on [`SerializerConfig::trailing`].
impl Encoder for SerializerConfig {
    fn encode(
        &self,
        driver: &mut SerializeDriver<'_>,
        index: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let trailing = self.trailing_mode();
        match trailing {
            Trailing::Strict if index > 0 => {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "a stream with Trailing::Strict holds a single value",
                ));
            }
            Trailing::Stop if index > 0 => out.push(b'\n'),
            _ => {}
        }
        let json = self.serialize_driver(driver)?;
        out.extend_from_slice(json.as_bytes());
        if trailing == Trailing::Newline {
            out.push(b'\n');
        }
        Ok(())
    }
}

impl DeserializerConfig {
    /// Deserializes a value from a reader.
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

/// Deserializes a value from a reader.
///
/// The reader is read to the end.  Only whitespace may follow the value.
/// The reader does not need to be buffered.  To read more than one value
/// (for instance JSON Lines) use a [`deser::io::Reader`] with a
/// [`DeserializerConfig`].
///
/// ```
/// let value: Vec<u32> = deser_json::from_reader(&b"[1, 2, 3]"[..]).unwrap();
/// assert_eq!(value, [1, 2, 3]);
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
/// deser_json::to_writer(&mut out, &vec![1, 2, 3]).unwrap();
/// assert_eq!(out, b"[1,2,3]");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
