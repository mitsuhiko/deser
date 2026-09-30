use alloc::vec;
use alloc::vec::Vec;

use crate::Position;
use crate::de::{Deserialize, DeserializeDriver, Frame, Progress, StreamDeserializer};
use crate::error::{Error, ErrorKind};

/// The minimum number of bytes offered to read into.
const READ_SIZE: usize = 8 * 1024;

/// The state of an [`InputBuffer`], see [`InputBuffer::poll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// A value is complete and can be deserialized.
    Ready,
    /// More input is needed.
    NeedInput,
    /// There are no more values.
    End,
}

/// Splits a stream into values without doing IO.
///
/// The buffer holds the data of a stream that was read so far and splits it
/// into values with a [`StreamDeserializer`].  It does not do IO itself
/// which makes it usable with any kind of IO: [`poll`](Self::poll) reports
/// if a value is ready or if more input is needed.  Input is read into
/// [`read_buf`](Self::read_buf) and committed with
/// [`filled`](Self::filled) (or [`set_eof`](Self::set_eof) at the end of
/// the stream).  Once a value is ready it's deserialized with
/// [`deserialize`](Self::deserialize):
///
/// ```
/// # use deser::de::{DeserializeDriver, Frame, StreamDeserializer};
/// # use deser::Error;
/// # struct Lines;
/// # impl StreamDeserializer for Lines {
/// #     fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
/// #         Ok(match input.iter().position(|&b| b == b'\n') {
/// #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
/// #             None if eof && input.is_empty() => Frame::End,
/// #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
/// #             None => Frame::Incomplete { consumed: 0 },
/// #         })
/// #     }
/// #     fn drive_frame<'de>(&mut self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
/// #         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
/// #         driver.emit(value)
/// #     }
/// # }
/// use std::io::Read;
/// use deser::stream::{InputBuffer, Status};
///
/// fn read_all(mut input: impl Read) -> Result<Vec<u64>, deser::Error> {
///     // `Lines` is the stream deserializer of a format with a number
///     // per line
///     let mut buffer = InputBuffer::new(Lines);
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
///
/// Stream deserializers which support it can also deserialize values while
/// their input arrives, see [`feed`](Self::feed).
pub struct InputBuffer<D: StreamDeserializer> {
    deserializer: D,
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
    // `true` once the deserializer reported the end or failed
    done: bool,
    failed: bool,
    // a value is being fed into a driver
    feeding: bool,
}

impl<D: StreamDeserializer> InputBuffer<D> {
    /// Creates an empty buffer.
    ///
    /// To continue a stream whose context is known (for instance the
    /// names of the columns of a CSV file), create the stream deserializer
    /// with that context.
    pub fn new(deserializer: D) -> InputBuffer<D> {
        InputBuffer {
            deserializer,
            data: Vec::new(),
            start: 0,
            end: 0,
            eof: false,
            position: Position::start(),
            ready: None,
            done: false,
            failed: false,
            feeding: false,
        }
    }

    /// Returns the stream deserializer.
    pub fn deserializer(&self) -> &D {
        &self.deserializer
    }

    /// Returns the stream deserializer and the input that was read but
    /// not consumed.
    pub fn into_parts(mut self) -> (D, Vec<u8>) {
        self.data.truncate(self.end);
        self.data.drain(..self.start);
        (self.deserializer, self.data)
    }

    /// Returns the number of bytes of the stream that were consumed.
    ///
    /// This is the offset of the unconsumed input in the stream.
    pub fn offset(&self) -> usize {
        self.position.offset
    }

    /// Returns the number of bytes which were read but not consumed.
    pub fn buffered(&self) -> usize {
        self.end - self.start
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
    /// This invokes the stream deserializer to find the next value if
    /// needed.  Once the
    /// status is [`Status::Ready`], the value has to be deserialized with
    /// [`deserialize`](Self::deserialize) before the next one can be found.
    /// If the stream deserializer fails, all further calls fail.
    pub fn poll(&mut self) -> Result<Status, Error> {
        if self.ready.is_some() {
            return Ok(Status::Ready);
        }
        if self.failed {
            return Err(failed_error());
        }
        if self.feeding {
            return Err(Error::new(ErrorKind::Unexpected, "a value is being fed"));
        }
        if self.done {
            return Ok(Status::End);
        }
        loop {
            let input = &self.data[self.start..self.end];
            let frame = match self.deserializer.frame(input, self.eof) {
                Ok(frame) => frame,
                Err(err) => {
                    self.failed = true;
                    let base = self.position;
                    let err = if self.deserializer.is_text() {
                        err.resolve_position(input)
                    } else {
                        err
                    };
                    return Err(err.shift_position(base));
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
                        // the deserializer cannot get more input
                        self.failed = true;
                        return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of input")
                            .shift_position(self.position));
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

    /// Checks if another value follows.
    ///
    /// Returns [`Status::Ready`] if a value follows (it does not need to be
    /// complete), [`Status::End`] if there are no more values and
    /// [`Status::NeedInput`] if more input is needed to know.  The value is
    /// then read with [`feed`](Self::feed) or, once
    /// [`poll`](Self::poll) reports it's complete, with
    /// [`deserialize`](Self::deserialize).  If the stream deserializer
    /// cannot find the start of a value on its own (see
    /// [`StreamDeserializer::peek`]), the value is framed which means that
    /// it's buffered completely.
    pub fn peek(&mut self) -> Result<Status, Error> {
        if self.ready.is_some() || self.feeding {
            return Ok(Status::Ready);
        }
        if self.failed {
            return Err(failed_error());
        }
        if self.done {
            return Ok(Status::End);
        }
        loop {
            let input = &self.data[self.start..self.end];
            let progress = match self.deserializer.peek(input, self.eof) {
                Ok(Some(progress)) => progress,
                Ok(None) => return self.poll(),
                Err(err) => {
                    self.failed = true;
                    let err = if self.deserializer.is_text() {
                        err.resolve_position(input)
                    } else {
                        err
                    };
                    return Err(err.shift_position(self.position));
                }
            };
            match progress {
                Progress::Done { consumed } => {
                    assert!(consumed <= input.len(), "invalid progress");
                    self.consume(consumed);
                    return Ok(Status::Ready);
                }
                Progress::NeedMore { consumed } => {
                    assert!(consumed <= input.len(), "invalid progress");
                    if consumed > 0 {
                        self.consume(consumed);
                        continue;
                    }
                    if self.eof {
                        self.failed = true;
                        return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of input")
                            .shift_position(self.position));
                    }
                    return Ok(Status::NeedInput);
                }
                Progress::End => {
                    assert!(self.eof, "end of values before the end of the input");
                    self.done = true;
                    return Ok(Status::End);
                }
            }
        }
    }

    /// Returns `true` if the stream deserializer can deserialize values
    /// while their input arrives.
    ///
    /// See [`StreamDeserializer::supports_feed`] and [`feed`](Self::feed).
    pub fn supports_feed(&self) -> bool {
        self.deserializer.supports_feed()
    }

    /// Feeds the input into the driver of the next value.
    ///
    /// This is the alternative to [`poll`](Self::poll) and
    /// [`deserialize`](Self::deserialize) for stream deserializers which
    /// support it (see [`supports_feed`](Self::supports_feed)) and values
    /// which do not borrow from the input.  If the value was framed already
    /// (by [`peek`](Self::peek) of a format that cannot find the start of a
    /// value otherwise), it's deserialized from its frame.  The input is fed into the driver until the
    /// value is complete ([`Status::Ready`]), the input is consumed as it's
    /// used.  If more input is needed ([`Status::NeedInput`]) the method has
    /// to be invoked again with the same driver once more input was read.
    /// In the meantime the buffer cannot be used otherwise.  After an error
    /// the value is abandoned, whether the stream can continue with the next
    /// value depends on the stream deserializer.
    ///
    /// ```
    /// # use deser::de::{Frame, Progress, StreamDeserializer};
    /// # use deser::Error;
    /// # /// A format with sequences of digits (without separators).
    /// # #[derive(Default)]
    /// # struct Digits { started: bool }
    /// # impl StreamDeserializer for Digits {
    /// #     fn frame(&mut self, _: &[u8], _: bool) -> Result<Frame, Error> { unimplemented!() }
    /// #     fn drive_frame<'de>(&mut self, _: &'de [u8], _: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> { unimplemented!() }
    /// #     fn supports_feed(&self) -> bool { true }
    /// #     fn feed(&mut self, input: &[u8], _: usize, eof: bool, driver: &mut DeserializeDriver<'_, '_>) -> Result<Progress, Error> {
    /// #         if !self.started {
    /// #             if input.is_empty() && eof { return Ok(Progress::End); }
    /// #             driver.emit(deser::Event::seq_start())?;
    /// #             self.started = true;
    /// #         }
    /// #         for digit in input { driver.emit(u64::from(digit - b'0'))?; }
    /// #         if eof {
    /// #             driver.emit(deser::Event::SeqEnd)?;
    /// #             self.started = false;
    /// #             return Ok(Progress::Done { consumed: input.len() });
    /// #         }
    /// #         Ok(Progress::NeedMore { consumed: input.len() })
    /// #     }
    /// # }
    /// use deser::de::DeserializeDriver;
    /// use deser::stream::{InputBuffer, Status};
    ///
    /// // `Digits` is the stream deserializer of a format with a sequence
    /// // of digits
    /// let mut buffer = InputBuffer::new(Digits::default());
    /// let mut out = None::<Vec<u32>>;
    /// {
    ///     let mut driver = DeserializeDriver::new(&mut out);
    ///     for chunk in [&b"12"[..], b"3"] {
    ///         buffer.extend_from_slice(chunk);
    ///         assert_eq!(buffer.feed(&mut driver).unwrap(), Status::NeedInput);
    ///     }
    ///     buffer.set_eof();
    ///     assert_eq!(buffer.feed(&mut driver).unwrap(), Status::Ready);
    /// }
    /// assert_eq!(out.unwrap(), [1, 2, 3]);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the stream deserializer does not support feeding.
    pub fn feed(&mut self, driver: &mut DeserializeDriver<'_, '_>) -> Result<Status, Error> {
        assert!(
            self.deserializer.supports_feed(),
            "the stream deserializer does not support feeding"
        );
        // a value that was framed already (see `peek`)
        if self.ready.is_some() {
            return self.drive_transient(driver).map(|()| Status::Ready);
        }
        if self.failed {
            return Err(failed_error());
        }
        if self.done {
            return Ok(Status::End);
        }
        let input = &self.data[self.start..self.end];
        let rv = self
            .deserializer
            .feed(input, self.position.offset, self.eof, driver);
        match rv {
            Ok(Progress::Done { consumed }) => {
                assert!(consumed <= input.len(), "invalid progress");
                self.consume(consumed);
                self.feeding = false;
                Ok(Status::Ready)
            }
            Ok(Progress::NeedMore { consumed }) => {
                assert!(consumed <= input.len(), "invalid progress");
                self.consume(consumed);
                self.feeding = true;
                if self.eof {
                    self.failed = true;
                    return Err(self.locate(
                        Error::new(ErrorKind::EndOfFile, "unexpected end of input")
                            .with_offset(self.position.offset),
                    ));
                }
                Ok(Status::NeedInput)
            }
            Ok(Progress::End) => {
                assert!(self.eof, "end of values before the end of the input");
                self.done = true;
                self.feeding = false;
                Ok(Status::End)
            }
            // whether the stream can continue is up to the deserializer
            Err(err) => {
                self.feeding = false;
                Err(self.locate(err))
            }
        }
    }

    /// Resolves the line and column of an error with an offset in the
    /// stream.
    ///
    /// This is only possible for offsets in the buffered data.
    fn locate(&self, err: Error) -> Error {
        err.map_each(|err| match err.offset() {
            Some(offset)
                if self.deserializer.is_text()
                    && err.line().is_none()
                    && offset >= self.position.offset
                    && offset - self.position.offset <= self.end - self.start =>
            {
                let mut position = self.position;
                position
                    .advance(&self.data[self.start..self.start + offset - self.position.offset]);
                err.with_position(offset, position.line, position.column)
            }
            _ => err,
        })
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
                // a zeroed allocation instead of resizing, which writes the
                // zeroes one by one without optimizations (and in miri)
                let mut data = vec![0; len];
                data[..self.end].copy_from_slice(&self.data[..self.end]);
                self.data = data;
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
    fn take_ready(&mut self) -> (core::ops::Range<usize>, Position) {
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
        crate::de::deserialize_value(|driver| {
            setup(driver);
            self.drive(driver)
        })
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
        self.deserializer
            .drive_frame(frame, driver)
            .map_err(|err| err.shift_position(position))
    }

    /// Feeds the events of the ready value into a driver for any lifetime.
    ///
    /// This is like [`drive`](Self::drive) but the value cannot borrow
    /// from the buffer: the driver is lent out with
    /// [`DeserializeDriver::transient`], borrowed data is passed on like
    /// data that is only valid for the call.  This allows driving a value
    /// into a driver which outlives the buffer's data, for instance to
    /// implement [`Deserializer`](crate::de::Deserializer) for a reader.
    ///
    /// # Panics
    ///
    /// Panics if no value is ready (see [`poll`](Self::poll)).
    pub fn drive_transient(&mut self, driver: &mut DeserializeDriver<'_, '_>) -> Result<(), Error> {
        let (range, position) = self.take_ready();
        let frame = &self.data[range];
        let deserializer = &mut self.deserializer;
        driver
            .transient(|driver| deserializer.drive_frame(frame, driver))
            .map_err(|err| err.shift_position(position))
    }

    /// Creates the error for a value where none is expected.
    ///
    /// The error refers to the start of the ready value.  Adapters use
    /// this to check that a stream ends after a value (see
    /// `Reader::end` of `deser::io`).
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
            .shift_position(position)
    }
}

#[cold]
fn failed_error() -> Error {
    Error::new(ErrorKind::Unexpected, "cannot continue after an error")
}
