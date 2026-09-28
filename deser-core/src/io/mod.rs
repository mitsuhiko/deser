//! Reading and writing values from and to streams.
//!
//! This module requires the `io` feature (which is enabled by default).
//! For values in memory, the formats have deserializers (which read values
//! from slices) and serializers (which write values into buffers) that do
//! not need this module.
//!
//! Data formats parse complete inputs (slices) and serialize into complete
//! outputs.  This module connects them to streams, such as files, sockets
//! or pipes, without the formats having to know about IO.  The
//! configurations of the formats implement [`Decoder`] (for reading) and
//! [`Encoder`] (for writing) and this module (or an adapter for an async
//! runtime such as `deser-tokio`) does the IO.  Single values are read and
//! written with [`Decoder::from_reader`] and [`Encoder::to_writer`].  The configuration that is
//! used to deserialize from or serialize into a string is also used for
//! streams:
//!
//! ```
//! # fn example() -> Result<(), deser::Error> {
//! use deser::io::{Reader, Writer};
//! # use deser::io::{Decoder, Encoder, Frame};
//! # use deser::de::DeserializeDriver;
//! # use deser::ser::SerializeDriver;
//! # use deser::Error;
//! # /// A format with a number per line.
//! # struct LinesConfig;
//! # impl Decoder for LinesConfig {
//! #     type State = ();
//! #     fn frame(&self, _: &mut (), input: &[u8], eof: bool) -> Result<Frame, Error> {
//! #         Ok(match input.iter().position(|&b| b == b'\n') {
//! #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
//! #             None if eof && input.is_empty() => Frame::End,
//! #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
//! #             None => Frame::Incomplete { consumed: 0 },
//! #         })
//! #     }
//! #     fn drive<'de>(&self, _: &mut (), frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
//! #         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
//! #         driver.emit(value)
//! #     }
//! # }
//! # impl Encoder for LinesConfig {
//! #     type State = ();
//! #     fn encode(&self, _: &mut (), driver: &mut SerializeDriver<'_>, out: &mut Vec<u8>) -> Result<(), Error> {
//! #         driver.drive(|event, _| {
//! #             if let deser::Event::Atom(deser::Atom::U64(v)) = event {
//! #                 out.extend_from_slice(format!("{v}\n").as_bytes());
//! #             }
//! #             Ok(())
//! #         })
//! #     }
//! # }
//! // `LinesConfig` is the configuration of a format with a number per line
//! let mut reader = Reader::new(&b"1\n2\n3\n"[..], LinesConfig);
//! let mut writer = Writer::new(Vec::new(), LinesConfig);
//! while let Some(value) = reader.read::<u64>()? {
//!     writer.write(&(value * 2))?;
//! }
//! assert_eq!(writer.into_inner(), b"2\n4\n6\n");
//! # Ok(()) } example().unwrap();
//! ```
//!
//! # Framing
//!
//! A [`Decoder`] splits the input into frames: it finds the bytes of the
//! next value in the input that was read so far (see [`Decoder::frame`]),
//! for instance a line with JSON Lines.  Once a value is complete it's
//! deserialized from its frame with the format's regular parser (see
//! [`Decoder::drive`]).  Types can borrow from the frame (see
//! [`Reader::read_borrowed`]).
//!
//! Everything a stream needs to remember is kept in the state of the stream
//! ([`Decoder::State`] and [`Encoder::State`]), not in the decoder or
//! encoder (which are the configurations of the format).  This includes the
//! progress of the scan and what earlier parts of the stream established
//! for the values that follow, for instance the names of the columns of a
//! CSV file.  Readers and writers expose the state (see [`Reader::state`])
//! and can start with a given state to continue a stream (see
//! [`Reader::with_state`]).
//!
//! Decoders of formats which can be parsed while the input arrives (like
//! JSON and CBOR) can also deserialize values incrementally (see
//! [`Decoder::feed`]).  [`Reader::read`] uses this if possible: the parts of
//! a value are deserialized as they are read and only incomplete tokens are
//! buffered, which means that the memory used does not depend on the size
//! of the values.  Otherwise the complete value is buffered first.
//!
//! # Large Sequences
//!
//! Values which contain a large (or unbounded) sequence can be processed
//! while they are read: a [`Streamed`](crate::Streamed) sequence hands out its elements as
//! they are read with [`Reader::read_next`] (and behaves like a `Vec`
//! otherwise).
//!
//! # Writing Large Values
//!
//! Encoders of formats whose output can be written before the value is
//! complete (like JSON and CBOR) also serialize values incrementally (see
//! [`Encoder::encode_incremental`]).  [`Writer::write`] uses this if
//! possible: once the output of a value exceeds the
//! [buffer limit](Writer::set_buffer_limit), what was serialized so far is
//! written and the serialization continues, which means that the memory
//! used does not depend on the size of the values.  Values below the limit
//! are written at once.  Output that can still change (for instance the
//! header of a container whose length is not known upfront) is held back
//! until it's final.
//!
//! # Other IO
//!
//! The [`DecodeBuffer`] implements the framing without doing IO itself.  The
//! [`Reader`] fills it from a [`std::io::Read`], adapters for other IO
//! (for instance async runtimes) do the same with their IO.
//!
//! # Errors
//!
//! Errors refer to positions in the stream: the offsets, lines and columns
//! of errors are relative to the start of the stream, not to the start of
//! the frame.  Failed reads and writes are errors of the kind
//! [`ErrorKind::Io`] with the IO error as source.
//!
//! The input ranges formats publish into the [`State`](crate::State) (and
//! the locations derived from them, for instance by `deser-location`)
//! refer to the frame of the value.
use core::any::Any;
use core::marker::PhantomData;
use std::io::{Read, Write};

use crate::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use crate::error::{Error, ErrorKind};
use crate::ser::{Serialize, SerializeDriver};

mod buffer;
mod decoder;
pub(crate) mod elements;
mod encoder;

pub use self::buffer::{DecodeBuffer, Status};
pub use self::decoder::{Decoder, Frame, Progress};
pub use self::elements::{ElementReader, ElementStatus, Next};
pub use self::encoder::{Encoded, Encoder};
use crate::Position;

/// Serializes a value into a buffer with an encoder.
///
/// The buffer is cleared first.  This is used by the writers of this module
/// and of adapters for other kinds of IO, the state is the state of the
/// stream that is written (see [`Encoder::State`]).
///
/// ```
/// # use deser::io::Encoder;
/// # use deser::ser::SerializeDriver;
/// # use deser::Error;
/// # struct Debug;
/// # impl Encoder for Debug {
/// #     type State = ();
/// #     fn encode(&self, _: &mut (), driver: &mut SerializeDriver<'_>, out: &mut Vec<u8>) -> Result<(), Error> {
/// #         driver.drive(|event, _| Ok(out.extend_from_slice(format!("{event:?};").as_bytes())))
/// #     }
/// # }
/// let mut buffer = Vec::new();
/// deser::io::encode(&Debug, &mut (), &true, |_| {}, &mut buffer).unwrap();
/// assert_eq!(buffer, b"Atom(Bool(true));");
/// ```
pub fn encode<E, F>(
    encoder: &E,
    state: &mut E::State,
    value: &dyn Serialize,
    setup: F,
    buffer: &mut Vec<u8>,
) -> Result<(), Error>
where
    E: Encoder + ?Sized,
    F: FnOnce(&mut SerializeDriver<'_>),
{
    buffer.clear();
    let mut driver = SerializeDriver::new(value);
    setup(&mut driver);
    encoder.encode(state, &mut driver, buffer)
}

/// The buffer limit of writers (see [`Writer::set_buffer_limit`]).
pub const DEFAULT_BUFFER_LIMIT: usize = 8 * 1024;

/// Serializes the next piece of a value into a buffer with an encoder.
///
/// The buffer is cleared first.  This is used by the writers of this module
/// and of adapters for other kinds of IO to write values in pieces: the
/// caller writes the buffer after every call and calls again with the same
/// driver until the value is complete ([`Encoded::Done`]).  If the encoder
/// does not support incremental encoding (see
/// [`Encoder::supports_incremental`]) or the limit is `usize::MAX`, the
/// whole value is serialized at once.
///
/// Once a part of a value was written, the value has to be completed:
/// if serializing or writing it fails (or the caller gives up on the value
/// for another reason), the stream cannot continue, the state holds the
/// progress of the abandoned value.  See [`Writer::write`].
///
/// ```
/// # use deser::io::{Encoded, Encoder};
/// # use deser::ser::{PausableSink, SerializeDriver};
/// # use deser::{Error, Event, Serialize, State};
/// # /// Writes `x` for every event, can stop between values.
/// # struct Xs;
/// # struct Sink<'a>(&'a mut Vec<u8>, usize);
/// # impl PausableSink for Sink<'_> {
/// #     fn event(&mut self, _: Event<'_>, _: &dyn Serialize, _: &mut State) -> Result<(), Error> {
/// #         self.0.push(b'x');
/// #         Ok(())
/// #     }
/// #     fn pause(&mut self) -> bool {
/// #         self.0.len() >= self.1
/// #     }
/// # }
/// # impl Encoder for Xs {
/// #     type State = ();
/// #     fn encode(&self, _: &mut (), driver: &mut SerializeDriver<'_>, out: &mut Vec<u8>) -> Result<(), Error> {
/// #         driver.drive(|_, _| Ok(out.push(b'x')))
/// #     }
/// #     fn supports_incremental(&self) -> bool {
/// #         true
/// #     }
/// #     fn encode_incremental(&self, _: &mut (), driver: &mut SerializeDriver<'_>, out: &mut Vec<u8>, limit: usize) -> Result<Encoded, Error> {
/// #         Ok(if driver.drive_until(&mut Sink(out, limit))? { Encoded::Done } else { Encoded::Partial })
/// #     }
/// # }
/// // `Xs` is a format which writes an `x` for every event
/// let value: Vec<u64> = (0..10_000).collect();
/// let mut driver = SerializeDriver::new(&value);
/// let mut buffer = Vec::new();
/// let mut written = 0;
/// loop {
///     let encoded =
///         deser::io::encode_part(&Xs, &mut (), &mut driver, 100, &mut buffer).unwrap();
///     // the buffer is written here
///     assert!(buffer.len() < 1000);
///     written += buffer.len();
///     if encoded == Encoded::Done {
///         break;
///     }
/// }
/// assert_eq!(written, 10_002);
/// ```
pub fn encode_part<E>(
    encoder: &E,
    state: &mut E::State,
    driver: &mut SerializeDriver<'_>,
    limit: usize,
    buffer: &mut Vec<u8>,
) -> Result<Encoded, Error>
where
    E: Encoder + ?Sized,
{
    buffer.clear();
    if limit == usize::MAX || !encoder.supports_incremental() {
        encoder.encode(state, driver, buffer)?;
        return Ok(Encoded::Done);
    }
    encoder.encode_incremental(state, driver, buffer, limit.max(1))
}

/// The error for writes after a value was abandoned while it was written.
pub(crate) fn broken_stream() -> Error {
    Error::new(
        ErrorKind::Unexpected,
        "a value was only partially written, the stream cannot continue",
    )
}

/// Reads values from a [`Read`].
///
/// The values are split and deserialized with a [`Decoder`] (the
/// deserializer configuration of a format).  The reader buffers the input so
/// it does not need to be buffered.
pub struct Reader<R, D: Decoder> {
    reader: R,
    buffer: DecodeBuffer<D>,
    // the value that is read with `read_next` (an `ElementReader`)
    pending: Option<Box<dyn Any + Send>>,
}

impl<R: Read, D: Decoder> Reader<R, D> {
    /// Creates a reader.
    pub fn new(reader: R, decoder: D) -> Reader<R, D> {
        Reader::with_state(reader, decoder, D::State::default())
    }

    /// Creates a reader for a stream that continues with the given state.
    ///
    /// This is useful to continue a stream whose context is known, for
    /// instance to read a part of a CSV file with the columns of the file.
    pub fn with_state(reader: R, decoder: D, state: D::State) -> Reader<R, D> {
        Reader {
            reader,
            buffer: DecodeBuffer::with_state(decoder, state),
            pending: None,
        }
    }

    /// Fails if a value is being read with [`read_next`](Self::read_next).
    fn ensure_idle(&self) -> Result<(), Error> {
        match self.pending {
            Some(_) => Err(Error::new(
                ErrorKind::Unexpected,
                "a value is being read with read_next",
            )),
            None => Ok(()),
        }
    }

    /// Reads more input into the buffer.
    fn read_more(&mut self) -> Result<(), Error> {
        let buf = self.buffer.read_buf();
        let read = loop {
            match self.reader.read(buf) {
                Ok(read) => break read,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err.into()),
            }
        };
        if read == 0 {
            self.buffer.set_eof();
        } else {
            self.buffer.filled(read);
        }
        Ok(())
    }

    /// Reads until the frame of the next value is complete.
    ///
    /// Returns `false` if there are no more values.
    fn fill(&mut self) -> Result<bool, Error> {
        loop {
            match self.buffer.poll()? {
                Status::Ready => return Ok(true),
                Status::End => return Ok(false),
                Status::NeedInput => self.read_more()?,
            }
        }
    }

    /// Reads the next value.
    ///
    /// Returns `None` if there are no more values.  If the decoder supports
    /// it (see [`Decoder::supports_feed`]), the value is deserialized while
    /// the input is read which means that only incomplete tokens are
    /// buffered.  Otherwise the complete value is buffered first.  Whether
    /// reading can continue after an error depends on the decoder (for
    /// instance with JSON Lines it continues with the next line).
    pub fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        self.read_with(|_| {})
    }

    /// Reads the next value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](crate::de::Layer)s.
    pub fn read_with<T, F>(&mut self, setup: F) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
        F: FnOnce(&mut DeserializeDriver<'_, '_>),
    {
        self.ensure_idle()?;
        if !self.buffer.supports_feed() {
            if !self.fill()? {
                return Ok(None);
            }
            return self
                .buffer
                .deserialize_with(|driver| setup(driver))
                .map(Some);
        }

        let mut out = None::<T>;
        {
            let mut driver = DeserializeDriver::<'_, 'static>::new(&mut out);
            setup(&mut driver);
            loop {
                match self.buffer.feed(&mut driver)? {
                    Status::Ready => break,
                    Status::End => return Ok(None),
                    Status::NeedInput => self.read_more()?,
                }
            }
        }
        out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
            .map(Some)
    }

    /// Reads the next value which can borrow from the reader's buffer.
    ///
    /// The complete value is buffered first.
    ///
    /// ```
    /// # use deser::io::{Decoder, Frame, Reader};
    /// # use deser::de::DeserializeDriver;
    /// # use deser::Error;
    /// # struct LinesConfig;
    /// # impl Decoder for LinesConfig {
    /// #     type State = ();
    /// #     fn frame(&self, _: &mut (), input: &[u8], eof: bool) -> Result<Frame, Error> {
    /// #         Ok(match input.iter().position(|&b| b == b'\n') {
    /// #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
    /// #             None if eof && input.is_empty() => Frame::End,
    /// #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
    /// #             None => Frame::Incomplete { consumed: 0 },
    /// #         })
    /// #     }
    /// #     fn drive<'de>(&self, _: &mut (), frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
    /// #         driver.emit_borrowed(std::str::from_utf8(frame).unwrap())
    /// #     }
    /// # }
    /// // `LinesConfig` is the configuration of a format with a string per line
    /// let mut reader = Reader::new(&b"hello\nworld\n"[..], LinesConfig);
    /// let value: &str = reader.read_borrowed().unwrap().unwrap();
    /// assert_eq!(value, "hello");
    /// ```
    pub fn read_borrowed<'a, T: Deserialize<'a>>(&'a mut self) -> Result<Option<T>, Error> {
        self.ensure_idle()?;
        if !self.fill()? {
            return Ok(None);
        }
        self.buffer.deserialize().map(Some)
    }

    /// Reads the next element of the [`Streamed`](crate::Streamed) sequence of a value or
    /// the value.
    ///
    /// `T` is the type of the value and `E` the type of the elements of a
    /// [`Streamed<E>`](crate::Streamed) sequence within it.  The elements are
    /// handed out as they are read ([`Next::Element`]), the value once it's
    /// complete ([`Next::Done`]).  The next call continues with the next
    /// value.  Returns `None` if there are no more values.  See [`Streamed`](crate::Streamed)
    /// for an example.
    ///
    /// Until the value is complete, the reader can only be used to read the
    /// value with the same types.
    pub fn read_next<T, E>(&mut self) -> Result<Option<Next<E, T>>, Error>
    where
        T: DeserializeOwned + 'static,
        E: Send + 'static,
    {
        let mut reader = match self.pending.take() {
            Some(pending) => match pending.downcast::<ElementReader<T, E>>() {
                Ok(reader) => reader,
                Err(pending) => {
                    self.pending = Some(pending);
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "a value of another type is being read",
                    ));
                }
            },
            None => Box::new(ElementReader::<T, E>::new()),
        };
        loop {
            match reader.poll(&mut self.buffer)? {
                ElementStatus::Ready(next) => {
                    if reader.is_reading() {
                        self.pending = Some(reader);
                    }
                    return Ok(Some(next));
                }
                ElementStatus::End => return Ok(None),
                ElementStatus::NeedInput => {
                    if let Err(err) = self.read_more() {
                        self.pending = Some(reader);
                        return Err(err);
                    }
                }
            }
        }
    }

    /// Checks that there are no more values.
    ///
    /// Fails if another value follows (or if the data that follows is not
    /// valid).
    pub fn end(&mut self) -> Result<(), Error> {
        self.ensure_idle()?;
        if self.fill()? {
            return Err(self.buffer.trailing_error());
        }
        Ok(())
    }

    /// Returns an iterator over the remaining values.
    ///
    /// The iterator stops after the first error.
    pub fn iter<T: DeserializeOwned>(&mut self) -> Iter<'_, R, D, T> {
        Iter {
            reader: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Returns the decoder.
    pub fn decoder(&self) -> &D {
        self.buffer.decoder()
    }

    /// Returns the state of the stream (see [`Decoder::State`]).
    pub fn state(&self) -> &D::State {
        self.buffer.state()
    }

    /// Returns a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.reader
    }

    /// Returns a mutable reference to the underlying reader.
    ///
    /// Reading from it directly is likely to corrupt the stream as the
    /// reader buffers data.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    /// Returns the underlying reader.
    ///
    /// Data that was read into the buffer but not deserialized yet is lost.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

/// An iterator over the values of a [`Reader`].
pub struct Iter<'r, R, D: Decoder, T> {
    reader: &'r mut Reader<R, D>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<R: Read, D: Decoder, T: DeserializeOwned> Iterator for Iter<'_, R, D, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.reader.read() {
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => None,
            Err(err) => {
                self.failed = true;
                Some(Err(err))
            }
        }
    }
}

/// Writes values to a [`Write`].
///
/// The values are serialized with an [`Encoder`] (the serializer
/// configuration of a format).  Values are serialized into a buffer and
/// written with [`write_all`](Write::write_all), wrap the writer in a
/// [`BufWriter`](std::io::BufWriter) when writing many small values.  If
/// the encoder supports it, the output of large values is written in
/// pieces while they are serialized (see
/// [`set_buffer_limit`](Self::set_buffer_limit)).
pub struct Writer<W, E: Encoder> {
    writer: W,
    encoder: E,
    state: E::State,
    buffer: Vec<u8>,
    limit: usize,
    // a value was abandoned after a part of it was written
    broken: bool,
}

impl<W: Write, E: Encoder> Writer<W, E> {
    /// Creates a writer.
    pub fn new(writer: W, encoder: E) -> Writer<W, E> {
        Writer::with_state(writer, encoder, E::State::default())
    }

    /// Creates a writer for a stream that continues with the given state.
    ///
    /// This is useful to append to a stream that was written before.
    pub fn with_state(writer: W, encoder: E, state: E::State) -> Writer<W, E> {
        Writer {
            writer,
            encoder,
            state,
            buffer: Vec::new(),
            limit: DEFAULT_BUFFER_LIMIT,
            broken: false,
        }
    }

    /// Sets how much output of a value is buffered before it's written.
    ///
    /// If the encoder supports it (see [`Encoder::supports_incremental`]),
    /// the output of a value is written once it exceeds the limit, the
    /// serialization continues after that.  This way the memory used does
    /// not depend on the size of the values.  The default is
    /// [`DEFAULT_BUFFER_LIMIT`] (8 KiB).  With `usize::MAX` every value is
    /// serialized completely before it's written.
    ///
    /// ```
    /// use deser::io::Writer;
    /// # use deser::io::{Encoded, Encoder};
    /// # use deser::ser::{PausableSink, SerializeDriver};
    /// # use deser::{Error, Event, Serialize, State};
    /// # /// Writes `x` for every event, can stop between values.
    /// # struct Xs;
    /// # struct Sink<'a>(&'a mut Vec<u8>, usize);
    /// # impl PausableSink for Sink<'_> {
    /// #     fn event(&mut self, _: Event<'_>, _: &dyn Serialize, _: &mut State) -> Result<(), Error> {
    /// #         self.0.push(b'x');
    /// #         Ok(())
    /// #     }
    /// #     fn pause(&mut self) -> bool {
    /// #         self.0.len() >= self.1
    /// #     }
    /// # }
    /// # impl Encoder for Xs {
    /// #     type State = ();
    /// #     fn encode(&self, _: &mut (), driver: &mut SerializeDriver<'_>, out: &mut Vec<u8>) -> Result<(), Error> {
    /// #         driver.drive(|_, _| Ok(out.push(b'x')))
    /// #     }
    /// #     fn supports_incremental(&self) -> bool {
    /// #         true
    /// #     }
    /// #     fn encode_incremental(&self, _: &mut (), driver: &mut SerializeDriver<'_>, out: &mut Vec<u8>, limit: usize) -> Result<Encoded, Error> {
    /// #         Ok(if driver.drive_until(&mut Sink(out, limit))? { Encoded::Done } else { Encoded::Partial })
    /// #     }
    /// # }
    /// /// Counts the writes.
    /// struct Counter(usize);
    ///
    /// impl std::io::Write for Counter {
    ///     fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    ///         self.0 += 1;
    ///         Ok(buf.len())
    ///     }
    ///
    ///     fn flush(&mut self) -> std::io::Result<()> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// // `Xs` is a format which writes an `x` for every event
    /// let mut writer = Writer::new(Counter(0), Xs);
    /// writer.set_buffer_limit(1000);
    /// writer.write(&vec![0; 100_000]).unwrap();
    /// assert!(writer.get_ref().0 > 50);
    /// ```
    pub fn set_buffer_limit(&mut self, limit: usize) {
        self.limit = limit;
    }

    /// Returns how much output of a value is buffered before it's written.
    pub fn buffer_limit(&self) -> usize {
        self.limit
    }

    /// Serializes a value and writes it.
    ///
    /// If the value fails to serialize before any of it was written (which
    /// is always the case for values whose output is below the
    /// [buffer limit](Self::set_buffer_limit)), nothing is written and the
    /// next value can be written.  If a value is abandoned after a part of
    /// it was written, because it fails to serialize or a write fails, the
    /// stream holds an incomplete value and the writer refuses to write
    /// more values.
    pub fn write(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        self.write_with(value, |_| {})
    }

    /// Serializes a value with a configured driver and writes it.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](crate::ser::Layer)s.
    pub fn write_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        if self.broken {
            return Err(broken_stream());
        }
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        loop {
            // the state is only updated if the value was serialized, if the
            // write fails the stream is broken anyways
            let encoded = encode_part(
                &self.encoder,
                &mut self.state,
                &mut driver,
                self.limit,
                &mut self.buffer,
            )?;
            if encoded == Encoded::Partial {
                // cleared once the rest of the value was written
                self.broken = true;
            }
            if !self.buffer.is_empty() {
                self.writer.write_all(&self.buffer)?;
            }
            if encoded == Encoded::Done {
                self.broken = false;
                return Ok(());
            }
        }
    }

    /// Flushes the underlying writer.
    pub fn flush(&mut self) -> Result<(), Error> {
        self.writer.flush()?;
        Ok(())
    }

    /// Returns the encoder.
    pub fn encoder(&self) -> &E {
        &self.encoder
    }

    /// Returns the state of the stream (see [`Encoder::State`]).
    pub fn state(&self) -> &E::State {
        &self.state
    }

    /// Returns a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        &self.writer
    }

    /// Returns a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    /// Returns the underlying writer.
    pub fn into_inner(self) -> W {
        self.writer
    }
}

/// Deserializes the single value of a slice with the frames of a decoder.
///
/// This is the provided implementation of [`Decoder::from_slice_with`].
pub(crate) fn decode_slice<'de, D, T, F>(
    decoder: &D,
    input: &'de [u8],
    setup: F,
) -> Result<T, Error>
where
    D: Decoder + ?Sized,
    T: Deserialize<'de>,
    F: FnOnce(&mut DeserializeDriver<'_, 'de>),
{
    // moves the position of an error by the position of the part of the
    // input it refers to
    let locate = |err: Error, offset: usize| err.shift_position(Position::of(input, offset));

    let mut state = D::State::default();

    // finds the next value from `pos` and returns its range
    let next = |state: &mut D::State, pos: &mut usize| -> Result<Option<(usize, usize)>, Error> {
        loop {
            match decoder
                .frame(state, &input[*pos..], true)
                .map_err(|err| locate(err, *pos))?
            {
                Frame::Value {
                    start,
                    end,
                    consumed,
                } => {
                    let range = (*pos + start, *pos + end);
                    *pos += consumed;
                    return Ok(Some(range));
                }
                Frame::Incomplete { consumed } if consumed > 0 => *pos += consumed,
                Frame::Incomplete { .. } => {
                    return Err(locate(
                        Error::new(ErrorKind::EndOfFile, "unexpected end of input")
                            .with_position(0, 1, 1),
                        *pos,
                    ));
                }
                Frame::End => return Ok(None),
            }
        }
    };

    let mut pos = 0;
    let (start, end) = next(&mut state, &mut pos)?
        .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        setup(&mut driver);
        decoder
            .drive(&mut state, &input[start..end], &mut driver)
            .map_err(|err| locate(err, start))?;
    }
    let value = out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    if let Some((start, _)) = next(&mut state, &mut pos)? {
        return Err(locate(
            Error::new(ErrorKind::Unexpected, "unexpected value after the end")
                .with_position(0, 1, 1),
            start,
        ));
    }
    Ok(value)
}

/// Reads a single value from a [`Read`].
///
/// Fails if there is no value or if another value follows it.
pub fn from_reader<T, R, D>(reader: R, decoder: D) -> Result<T, Error>
where
    T: DeserializeOwned,
    R: Read,
    D: Decoder,
{
    let mut reader = Reader::new(reader, decoder);
    let value = reader
        .read()?
        .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    reader.end()?;
    Ok(value)
}

/// Writes a single value to a [`Write`].
pub fn to_writer<W, E>(writer: W, encoder: E, value: &dyn Serialize) -> Result<(), Error>
where
    W: Write,
    E: Encoder,
{
    Writer::new(writer, encoder).write(value)
}
