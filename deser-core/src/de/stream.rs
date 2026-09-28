use crate::de::DeserializeDriver;
use crate::error::{Error, ErrorKind};

/// The result of [`StreamDeserializer::frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frame {
    /// The next value is complete.
    ///
    /// Its bytes are `input[start..end]`.  Afterwards the first `consumed`
    /// bytes of the input (at least up to `end`) are discarded, the bytes
    /// before `start` are skipped.
    Value {
        start: usize,
        end: usize,
        consumed: usize,
    },
    /// The input does not contain a complete value.
    ///
    /// The first `consumed` bytes of the input are discarded, for instance
    /// whitespace before the next value.  If bytes were consumed, the
    /// deserializer is invoked again right away (as a value might follow
    /// them), otherwise once more input was read.  At the end of the input
    /// the deserializer must not return this without consuming bytes.
    Incomplete { consumed: usize },
    /// There are no more values.
    ///
    /// This must only be returned at the end of the input.
    End,
}

/// The result of [`StreamDeserializer::feed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Progress {
    /// The value is complete, it used the first `consumed` bytes of the
    /// input.
    Done { consumed: usize },
    /// The value needs more input.  The first `consumed` bytes of the input
    /// were used and are discarded, the next call continues with the input
    /// after them (followed by the new data).
    NeedMore { consumed: usize },
    /// There are no more values.
    ///
    /// This must only be returned at the end of the input.
    End,
}

/// Deserializes a stream of values from input that arrives in chunks.
///
/// This is implemented by the stream deserializers of the data formats
/// (for instance `deser_json::StreamDeserializer`).  Unlike a
/// [`Deserializer`](crate::de::Deserializer), which pulls values from an
/// input it holds (and can lend data from it), a stream deserializer is
/// given the input as it arrives.  It holds everything a stream needs to
/// remember: the progress of the scan and what earlier parts of the stream
/// established for the values that follow, for instance the names of the
/// columns of a CSV file.
///
/// Stream deserializers do not do IO: a
/// [`stream::InputBuffer`](crate::stream::InputBuffer) holds the input and
/// invokes them, the readers of `deser::io` and of other IO adapters
/// (like `deser-tokio`) fill the buffer.
///
/// # Frames
///
/// A stream deserializer splits the input into frames: it finds the bytes
/// of the next value in the input that was read so far (see
/// [`frame`](Self::frame)), for instance a line with JSON Lines.  Once a
/// value is complete it's deserialized from its frame (see
/// [`drive_frame`](Self::drive_frame)), typically with the format's
/// regular parser.  Values read from their frames can borrow from the
/// buffer.
///
/// # Feeding
///
/// Formats which can be parsed while the input arrives (like JSON and
/// CBOR) can also deserialize values while their input is fed to them
/// (see [`feed`](Self::feed)).  Only incomplete tokens are buffered, so
/// the memory used does not depend on the size of the values.  Values
/// read this way cannot borrow from the input.
///
/// ```
/// use deser::de::{DeserializeDriver, Frame, StreamDeserializer};
/// use deser::stream::{InputBuffer, Status};
/// use deser::Error;
///
/// /// A format with a number per line.
/// struct Lines;
///
/// impl StreamDeserializer for Lines {
///     fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
///         Ok(match input.iter().position(|&b| b == b'\n') {
///             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
///             None if eof && input.is_empty() => Frame::End,
///             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
///             None => Frame::Incomplete { consumed: 0 },
///         })
///     }
///
///     fn drive_frame<'de>(
///         &mut self,
///         frame: &'de [u8],
///         driver: &mut DeserializeDriver<'_, 'de>,
///     ) -> Result<(), Error> {
///         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
///         driver.emit(value)
///     }
/// }
///
/// let mut buffer = InputBuffer::new(Lines);
/// buffer.extend_from_slice(b"1\n2");
/// assert_eq!(buffer.poll().unwrap(), Status::Ready);
/// assert_eq!(buffer.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(buffer.poll().unwrap(), Status::NeedInput);
/// buffer.set_eof();
/// assert_eq!(buffer.poll().unwrap(), Status::Ready);
/// assert_eq!(buffer.deserialize::<u32>().unwrap(), 2);
/// assert_eq!(buffer.poll().unwrap(), Status::End);
/// ```
pub trait StreamDeserializer {
    /// Finds the next value in the input.
    ///
    /// The input holds the data that was read so far (minus the data that
    /// was discarded).  If it does not contain a complete value yet,
    /// [`Frame::Incomplete`] is returned and the method is invoked again
    /// once more data was read: the input then starts after the bytes that
    /// were consumed and continues with the new data.  This allows
    /// deserializers to keep the progress of their scan so they do not have
    /// to scan the input again.  `eof` is `true` if no more data follows
    /// the input.
    ///
    /// Once a value is complete, [`Frame::Value`] is returned and the value
    /// is deserialized with [`drive_frame`](Self::drive_frame).  The next
    /// call starts a new value, again after the consumed bytes.  Offsets
    /// of errors refer to the input.
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error>;

    /// Deserializes a value from its frame.
    ///
    /// The frame holds the bytes of a value found by
    /// [`frame`](Self::frame), it's deserialized right after it was found.
    /// This allows deserializers to keep what they learned while scanning
    /// the frame (like the positions of fields) so they do not have to scan
    /// it again.  Events can borrow from the frame.  Offsets of errors
    /// refer to the frame.
    ///
    /// Data that is only valid for the call (for instance names kept by
    /// the deserializer) can be emitted without copying it with
    /// [`DeserializeDriver::emit`].
    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error>;

    /// Returns `true` if the format is text.
    ///
    /// For text formats the positions of errors are resolved into lines and
    /// columns.  This is `false` by default.
    fn is_text(&self) -> bool {
        false
    }

    /// Returns `true` if the deserializer implements [`feed`](Self::feed).
    ///
    /// This can depend on the configuration, for instance JSON Lines are
    /// read line by line.
    fn supports_feed(&self) -> bool {
        false
    }

    /// Deserializes a value while its input arrives.
    ///
    /// This is only invoked if [`supports_feed`](Self::supports_feed)
    /// returns `true`.  It's used instead of [`frame`](Self::frame) and
    /// [`drive_frame`](Self::drive_frame) for values which do not borrow
    /// from the input.  The deserializer emits the events of the parts of
    /// the value in the input into the driver and returns how much of the
    /// input it used ([`Progress::NeedMore`]) until the value is complete
    /// ([`Progress::Done`]).  Only incomplete tokens need to be kept.  The
    /// driver is the same for all calls for a value, the first call for a
    /// value starts where the previous value ended.  At the end of the
    /// input (`eof`) the value has to be completed (or fail).
    ///
    /// As the input does not live beyond the call, the events cannot
    /// borrow from it.  `offset` is the offset of the input in the stream:
    /// the input ranges of the events and the offsets of errors refer to
    /// positions in the stream.  After an error the value is abandoned, the
    /// deserializer decides if the stream can continue with the next value
    /// (for instance by skipping the rest of the value if a sink failed) or
    /// if further calls fail.
    fn feed(
        &mut self,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        let _ = (input, offset, eof, driver);
        Err(Error::new(
            ErrorKind::Unexpected,
            "the deserializer cannot deserialize while the input arrives",
        ))
    }

    /// Finds the start of the next value without deserializing it.
    ///
    /// This is used to check if another value follows (see
    /// [`InputBuffer::peek`](crate::stream::InputBuffer::peek)) without
    /// reading the value.  It skips what precedes the next value (for
    /// instance whitespace) and returns:
    ///
    /// * `Some(Progress::Done { consumed })` if a value starts after the
    ///   first `consumed` bytes of the input (which are discarded).
    /// * `Some(Progress::NeedMore { consumed })` if more input is needed
    ///   to know, the first `consumed` bytes are discarded.
    /// * `Some(Progress::End)` if there are no more values.  This must only
    ///   be returned at the end of the input.
    ///
    /// Offsets of errors refer to the input.  The provided implementation
    /// returns `None`, then the next value is found by framing it (which
    /// buffers it completely).  Deserializers that support
    /// [`feed`](Self::feed) should implement this so values are not
    /// buffered to find out if they exist.
    fn peek(&mut self, input: &[u8], eof: bool) -> Result<Option<Progress>, Error> {
        let _ = (input, eof);
        Ok(None)
    }
}

impl<D: StreamDeserializer + ?Sized> StreamDeserializer for &mut D {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        (**self).frame(input, eof)
    }

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        (**self).drive_frame(frame, driver)
    }

    fn is_text(&self) -> bool {
        (**self).is_text()
    }

    fn supports_feed(&self) -> bool {
        (**self).supports_feed()
    }

    fn feed(
        &mut self,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        (**self).feed(input, offset, eof, driver)
    }

    fn peek(&mut self, input: &[u8], eof: bool) -> Result<Option<Progress>, Error> {
        (**self).peek(input, eof)
    }
}
