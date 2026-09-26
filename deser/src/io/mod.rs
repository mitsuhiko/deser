//! Reading and writing values from and to streams.
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
//! # use deser::de::{Decoder, Frame};
//! # use deser::ser::Encoder;
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
//! #     fn drive<'de>(&self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
//! #         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
//! #         driver.emit(value)
//! #     }
//! # }
//! # impl Encoder for LinesConfig {
//! #     fn encode(&self, driver: &mut SerializeDriver<'_>, _: usize, out: &mut Vec<u8>) -> Result<(), Error> {
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
//! Decoders of formats which can be parsed while the input arrives (like
//! JSON and CBOR) can also deserialize values incrementally (see
//! [`Decoder::feed`]).  [`Reader::read`] uses this if possible: the parts of
//! a value are deserialized as they are read and only incomplete tokens are
//! buffered, which means that the memory used does not depend on the size
//! of the values.  Otherwise the complete value is buffered first.
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
use std::io::{Read, Write};
use std::marker::PhantomData;

use crate::de::{Decoder, Deserialize, DeserializeDriver, DeserializeOwned, Frame};
use crate::error::{Error, ErrorKind};
use crate::ser::{Encoder, Serialize, SerializeDriver};

mod buffer;

use self::buffer::Position;
pub use self::buffer::{DecodeBuffer, Status};

/// Serializes a value into a buffer with an encoder.
///
/// The buffer is cleared first.  This is used by the writers of this module
/// and of adapters for other kinds of IO.
///
/// ```
/// # use deser::ser::Encoder;
/// # use deser::ser::SerializeDriver;
/// # use deser::Error;
/// # struct Debug;
/// # impl Encoder for Debug {
/// #     fn encode(&self, driver: &mut SerializeDriver<'_>, _: usize, out: &mut Vec<u8>) -> Result<(), Error> {
/// #         driver.drive(|event, _| Ok(out.extend_from_slice(format!("{event:?};").as_bytes())))
/// #     }
/// # }
/// let mut buffer = Vec::new();
/// deser::io::encode(&Debug, &true, |_| {}, 0, &mut buffer).unwrap();
/// assert_eq!(buffer, b"Atom(Bool(true));");
/// ```
pub fn encode<E, F>(
    encoder: &E,
    value: &dyn Serialize,
    setup: F,
    index: usize,
    buffer: &mut Vec<u8>,
) -> Result<(), Error>
where
    E: Encoder + ?Sized,
    F: FnOnce(&mut SerializeDriver<'_>),
{
    buffer.clear();
    let mut driver = SerializeDriver::new(value);
    setup(&mut driver);
    encoder.encode(&mut driver, index, buffer)
}

/// Reads values from a [`Read`].
///
/// The values are split and deserialized with a [`Decoder`] (the
/// deserializer configuration of a format).  The reader buffers the input so
/// it does not need to be buffered.
pub struct Reader<R, D: Decoder> {
    reader: R,
    buffer: DecodeBuffer<D>,
}

impl<R: Read, D: Decoder> Reader<R, D> {
    /// Creates a reader.
    pub fn new(reader: R, decoder: D) -> Reader<R, D> {
        Reader {
            reader,
            buffer: DecodeBuffer::new(decoder),
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
    /// # use deser::de::{Decoder, Frame};
    /// # use deser::io::Reader;
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
    /// #     fn drive<'de>(&self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
    /// #         driver.emit_borrowed(std::str::from_utf8(frame).unwrap())
    /// #     }
    /// # }
    /// // `LinesConfig` is the configuration of a format with a string per line
    /// let mut reader = Reader::new(&b"hello\nworld\n"[..], LinesConfig);
    /// let value: &str = reader.read_borrowed().unwrap().unwrap();
    /// assert_eq!(value, "hello");
    /// ```
    pub fn read_borrowed<'a, T: Deserialize<'a>>(&'a mut self) -> Result<Option<T>, Error> {
        if !self.fill()? {
            return Ok(None);
        }
        self.buffer.deserialize().map(Some)
    }

    /// Checks that there are no more values.
    ///
    /// Fails if another value follows (or if the data that follows is not
    /// valid).
    pub fn end(&mut self) -> Result<(), Error> {
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
/// configuration of a format).  Every value is written with a single
/// [`write_all`](Write::write_all), wrap the writer in a
/// [`BufWriter`](std::io::BufWriter) when writing many small values.
pub struct Writer<W, E> {
    writer: W,
    encoder: E,
    buffer: Vec<u8>,
    written: usize,
}

impl<W: Write, E: Encoder> Writer<W, E> {
    /// Creates a writer.
    pub fn new(writer: W, encoder: E) -> Writer<W, E> {
        Writer {
            writer,
            encoder,
            buffer: Vec::new(),
            written: 0,
        }
    }

    /// Serializes a value and writes it.
    ///
    /// If the value fails to serialize nothing is written.
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
        encode(&self.encoder, value, setup, self.written, &mut self.buffer)?;
        self.writer.write_all(&self.buffer)?;
        self.written += 1;
        Ok(())
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
    let locate = |err: Error, offset: usize| {
        let mut position = Position::start();
        position.advance(&input[..offset]);
        err.shift_position(position.offset, position.line, position.column)
    };

    let mut state = D::State::default();

    // finds the next value from `pos` and returns its range
    let mut next = |pos: &mut usize| -> Result<Option<(usize, usize)>, Error> {
        loop {
            match decoder
                .frame(&mut state, &input[*pos..], true)
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
    let (start, end) =
        next(&mut pos)?.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        setup(&mut driver);
        decoder
            .drive(&input[start..end], &mut driver)
            .map_err(|err| locate(err, start))?;
    }
    let value = out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    if let Some((start, _)) = next(&mut pos)? {
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
