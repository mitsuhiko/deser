use crate::de::{Deserialize, DeserializeDriver};
use crate::error::{Error, ErrorKind};
use crate::io::{Decoder, Frame};

/// The minimum number of bytes offered to read into.
const READ_SIZE: usize = 8 * 1024;

/// The state of a [`DecodeBuffer`], see [`DecodeBuffer::poll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// A value is complete and can be deserialized.
    Ready,
    /// More input is needed.
    NeedInput,
    /// There are no more values.
    End,
}

/// A position in the stream.
#[derive(Debug, Clone, Copy)]
struct Position {
    offset: usize,
    // 1-based
    line: usize,
    column: usize,
}

impl Position {
    /// Advances the position over the given bytes.
    fn advance(&mut self, bytes: &[u8]) {
        self.offset += bytes.len();
        let line_start = match bytes.iter().rposition(|&b| b == b'\n') {
            Some(index) => {
                self.line += bytes.iter().filter(|&&b| b == b'\n').count();
                self.column = 1;
                index + 1
            }
            None => 0,
        };
        // columns are counted in characters
        self.column += bytes[line_start..]
            .iter()
            .filter(|&&b| b & 0xc0 != 0x80)
            .count();
    }
}

/// Splits a stream into values without doing IO.
///
/// The buffer holds the data of a stream that was read so far and splits it
/// into values with a [`Decoder`].  It does not do IO itself which makes it
/// usable with any kind of IO: [`poll`](Self::poll) reports if a value is
/// ready or if more input is needed.  Input is read into
/// [`read_buf`](Self::read_buf) and committed with
/// [`filled`](Self::filled) (or [`set_eof`](Self::set_eof) at the end of the
/// stream).  Once a value is ready it's deserialized with
/// [`deserialize`](Self::deserialize):
///
/// ```
/// # use deser::io::{Decoder, Frame};
/// # use deser::de::DeserializeDriver;
/// # use deser::Error;
/// # struct Lines;
/// # impl Decoder for Lines {
/// #     fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
/// #         Ok(match input.iter().position(|&b| b == b'\n') {
/// #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
/// #             None if eof && input.is_empty() => Frame::End,
/// #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
/// #             None => Frame::Incomplete { consumed: 0 },
/// #         })
/// #     }
/// #     fn drive<'de>(&mut self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
/// #         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
/// #         driver.emit(value)
/// #     }
/// # }
/// use std::io::Read;
/// use deser::io::{DecodeBuffer, Status};
///
/// fn read_all(mut input: impl Read) -> Result<Vec<u64>, deser::Error> {
///     // `Lines` is a format with one number per line
///     let mut buffer = DecodeBuffer::new(Lines);
///     let mut values = Vec::new();
///     loop {
///         match buffer.poll()? {
///             Status::Ready => values.push(buffer.deserialize()?),
///             Status::End => return Ok(values),
///             Status::NeedInput => match input.read(buffer.read_buf())? {
///                 0 => buffer.set_eof(),
///                 read => buffer.filled(read),
///             },
///         }
///     }
/// }
///
/// assert_eq!(read_all(&b"1\n2\n3"[..]).unwrap(), [1, 2, 3]);
/// ```
///
/// The offsets, lines and columns of errors refer to the stream.
pub struct DecodeBuffer<D> {
    decoder: D,
    // `data[start..end]` holds the input that was not consumed yet, the
    // data after `end` is space to read into.
    data: Vec<u8>,
    start: usize,
    end: usize,
    eof: bool,
    // the position of `data[start]` in the stream
    position: Position,
    // the frame of the value which is ready (relative to `start`)
    ready: Option<(usize, usize, usize)>,
    // `true` once the decoder reported the end or failed
    done: bool,
    failed: bool,
}

impl<D: Decoder> DecodeBuffer<D> {
    /// Creates an empty buffer.
    pub fn new(decoder: D) -> DecodeBuffer<D> {
        DecodeBuffer {
            decoder,
            data: Vec::new(),
            start: 0,
            end: 0,
            eof: false,
            position: Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            ready: None,
            done: false,
            failed: false,
        }
    }

    /// Returns the decoder.
    pub fn decoder(&self) -> &D {
        &self.decoder
    }

    /// Returns the number of bytes of the stream that were consumed.
    ///
    /// This is the offset of the unconsumed input in the stream.
    pub fn offset(&self) -> usize {
        self.position.offset
    }

    /// Returns `true` if the end of the stream was reached.
    pub fn is_eof(&self) -> bool {
        self.eof
    }

    /// Discards the first bytes of the unconsumed input.
    fn consume(&mut self, len: usize) {
        self.position
            .advance(&self.data[self.start..self.start + len]);
        self.start += len;
    }

    /// Checks if the next value is ready.
    ///
    /// This invokes the decoder to find the next value if needed.  Once the
    /// status is [`Status::Ready`], the value has to be deserialized with
    /// [`deserialize`](Self::deserialize) before the next one can be found.
    /// If the decoder fails, all further calls fail.
    pub fn poll(&mut self) -> Result<Status, Error> {
        if self.ready.is_some() {
            return Ok(Status::Ready);
        }
        if self.failed {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "cannot continue after an error",
            ));
        }
        if self.done {
            return Ok(Status::End);
        }
        loop {
            let input = &self.data[self.start..self.end];
            let frame = match self.decoder.frame(input, self.eof) {
                Ok(frame) => frame,
                Err(err) => {
                    self.failed = true;
                    let base = self.position;
                    return Err(err.shift_position(base.offset, base.line, base.column));
                }
            };
            match frame {
                Frame::Value {
                    start,
                    end,
                    consumed,
                } => {
                    assert!(
                        start <= end && end <= consumed && consumed <= input.len(),
                        "invalid frame"
                    );
                    self.ready = Some((start, end, consumed));
                    return Ok(Status::Ready);
                }
                Frame::Incomplete { consumed } => {
                    assert!(consumed <= input.len(), "invalid frame");
                    // a value might follow the discarded data
                    if consumed > 0 {
                        self.consume(consumed);
                        continue;
                    }
                    if self.eof {
                        // the decoder cannot get more input
                        self.failed = true;
                        return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of input")
                            .shift_position(
                                self.position.offset,
                                self.position.line,
                                self.position.column,
                            ));
                    }
                    return Ok(Status::NeedInput);
                }
                Frame::End => {
                    assert!(self.eof, "end of values before the end of the input");
                    self.done = true;
                    return Ok(Status::End);
                }
            }
        }
    }

    /// Returns the buffer to read the next input into.
    ///
    /// After data was placed in the buffer, [`filled`](Self::filled) has
    /// to be called with its length.  The buffer is never empty.
    pub fn read_buf(&mut self) -> &mut [u8] {
        if self.data.len() - self.end < READ_SIZE {
            // move the unconsumed input to the front before growing
            if self.start > 0 {
                self.data.copy_within(self.start..self.end, 0);
                self.end -= self.start;
                self.start = 0;
            }
            if self.data.len() - self.end < READ_SIZE {
                let len = (self.end + READ_SIZE).max(self.data.len() * 2);
                self.data.resize(len, 0);
            }
        }
        &mut self.data[self.end..]
    }

    /// Adds data that was read into [`read_buf`](Self::read_buf).
    ///
    /// # Panics
    ///
    /// Panics if the length exceeds the buffer or if the end of the stream
    /// was reached.
    pub fn filled(&mut self, len: usize) {
        assert!(!self.eof, "data after the end of the stream");
        assert!(
            len <= self.data.len() - self.end,
            "more data than read into"
        );
        self.end += len;
    }

    /// Marks the end of the stream.
    pub fn set_eof(&mut self) {
        self.eof = true;
    }

    /// Adds input by copying it into the buffer.
    ///
    /// This is an alternative to [`read_buf`](Self::read_buf) and
    /// [`filled`](Self::filled) for input that is already in memory.
    pub fn extend_from_slice(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            let buf = self.read_buf();
            let len = buf.len().min(input.len());
            buf[..len].copy_from_slice(&input[..len]);
            self.filled(len);
            input = &input[len..];
        }
    }

    /// Takes the frame of the ready value.
    ///
    /// Returns the range of the frame in the data and its position.
    fn take_ready(&mut self) -> (std::ops::Range<usize>, Position) {
        let (start, end, consumed) = self
            .ready
            .take()
            .expect("no value is ready, poll the buffer first");
        let mut position = self.position;
        position.advance(&self.data[self.start..self.start + start]);
        let range = self.start + start..self.start + end;
        self.consume(consumed);
        (range, position)
    }

    /// Deserializes the ready value.
    ///
    /// The value can borrow from the buffer.
    ///
    /// # Panics
    ///
    /// Panics if no value is ready (see [`poll`](Self::poll)).
    pub fn deserialize<'a, T: Deserialize<'a>>(&'a mut self) -> Result<T, Error> {
        self.deserialize_with(|_| {})
    }

    /// Deserializes the ready value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](crate::de::Layer)s.
    ///
    /// # Panics
    ///
    /// Panics if no value is ready (see [`poll`](Self::poll)).
    pub fn deserialize_with<'a, T, F>(&'a mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            setup(&mut driver);
            self.drive(&mut driver)?;
        }
        out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
    }

    /// Feeds the events of the ready value into a driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](crate::de::Sink).
    ///
    /// # Panics
    ///
    /// Panics if no value is ready (see [`poll`](Self::poll)).
    pub fn drive<'a>(&'a mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        let (range, position) = self.take_ready();
        let frame = &self.data[range];
        self.decoder
            .drive(frame, driver)
            .map_err(|err| err.shift_position(position.offset, position.line, position.column))
    }

    /// Creates the error for a value where none is expected.
    ///
    /// The error refers to the start of the ready value.  Adapters use
    /// this to check that a stream ends after a value (see
    /// [`Reader::end`](crate::io::Reader::end)).
    ///
    /// # Panics
    ///
    /// Panics if no value is ready (see [`poll`](Self::poll)).
    pub fn trailing_error(&self) -> Error {
        let (start, _, _) = self.ready.expect("no value is ready");
        let mut position = self.position;
        position.advance(&self.data[self.start..self.start + start]);
        Error::new(ErrorKind::Unexpected, "unexpected value after the end")
            .with_position(0, 1, 1)
            .shift_position(position.offset, position.line, position.column)
    }
}
