//! Writing JSON streams.
use std::io::Write;

use deser_core::io::{Encoded, Encoder};
use deser_core::ser::{Serialize, SerializeDriver};
use deser_core::{Error, ErrorKind};

use crate::Trailing;
use crate::buf::Buffer;
use crate::ser::{SerializerConfig, ValueWriter};

/// The state of a stream of JSON values that is written.
///
/// This holds the number of values that were written (see
/// [`SerializerConfig::trailing`]) and the progress of the value that is
/// being written.  See [`Encoder::State`].
#[derive(Default)]
pub struct WriterState {
    written: usize,
    value: Option<Box<ValueWriter>>,
}

impl WriterState {
    /// Creates the state of a stream that continues after the given number
    /// of values.
    pub fn with_written(written: usize) -> WriterState {
        WriterState {
            written,
            value: None,
        }
    }

    /// Returns the number of values that were written.
    pub fn written(&self) -> usize {
        self.written
    }
}

impl std::fmt::Debug for WriterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterState")
            .field("written", &self.written)
            .field("in_progress", &self.value.is_some())
            .finish()
    }
}

/// Writes JSON values to a stream (see [`deser::io`](deser_core::io)).
///
/// What follows the values depends on [`SerializerConfig::trailing`].
/// Values are written incrementally (see
/// [`Encoder::encode_incremental`]): the output of large values is written
/// in pieces while they are serialized.
///
/// ```
/// use deser::io::Writer;
/// use deser_json::SerializerConfig;
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
/// writer.set_buffer_limit(4);
/// writer.write(&vec!["a", "b", "c"]).unwrap();
/// assert_eq!(writer.into_inner(), br#"["a","b","c"]"#);
/// ```
impl Encoder for SerializerConfig {
    type State = WriterState;

    fn encode(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.encode_incremental(state, driver, out, usize::MAX)
            .map(|_| ())
    }

    fn supports_incremental(&self) -> bool {
        true
    }

    fn encode_incremental(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<Encoded, Error> {
        let trailing = self.trailing_mode();
        let mut writer = match state.value.take() {
            Some(writer) => writer,
            None => {
                match trailing {
                    Trailing::Strict if state.written > 0 => {
                        return Err(Error::new(
                            ErrorKind::Unexpected,
                            "with Trailing::Strict only a single value can be written",
                        ));
                    }
                    Trailing::Stop if state.written > 0 => out.push(b'\n'),
                    _ => {}
                }
                Box::new(self.value_writer(Buffer::from_vec(Vec::new())))
            }
        };
        // the writer writes into an empty output directly, otherwise its
        // output is appended
        let adopt = out.is_empty();
        if adopt {
            *writer.output() = Buffer::from_vec(std::mem::take(out));
        }
        // after an error the value is abandoned, its writer is dropped
        let done = writer.drive(driver, limit)?;
        let output = writer.take_output();
        if adopt {
            *out = output;
        } else {
            out.extend_from_slice(&output);
        }
        if !done {
            state.value = Some(writer);
            return Ok(Encoded::Partial);
        }
        if trailing == Trailing::Newline {
            out.push(b'\n');
        }
        state.written += 1;
        Ok(Encoded::Done)
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

/// Serializes a value to a writer.
///
/// The output of large values is written in pieces while they are
/// serialized (see [`deser::io`](deser_core::io)), the writer does not need to be
/// buffered.
///
/// ```
/// let mut out = Vec::new();
/// deser_json::to_writer(&mut out, &vec![1, 2, 3]).unwrap();
/// assert_eq!(out, b"[1,2,3]");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
