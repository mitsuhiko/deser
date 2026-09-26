//! Reading and writing values from and to streams.
//!
//! Data formats parse complete inputs (slices) and serialize into complete
//! outputs.  This module connects them to streams, such as files, sockets
//! or pipes, without the formats having to know about IO.  The formats
//! provide a [`Decoder`] and an [`Encoder`] and this module (or an adapter
//! for an async runtime such as `deser-tokio`) does the IO.
//!
//! ```
//! # fn example() -> Result<(), deser::Error> {
//! use deser::io::{Reader, Writer};
//! # use deser::io::{Decoder, Encoder, Frame};
//! # use deser::de::DeserializeDriver;
//! # use deser::{Error, Serialize};
//! # /// A format of numbers on lines of their own.
//! # struct Lines;
//! # impl Decoder for Lines {
//! #     fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
//! #         Ok(match input.iter().position(|&b| b == b'\n') {
//! #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
//! #             None if eof && input.is_empty() => Frame::End,
//! #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
//! #             None => Frame::Incomplete { consumed: 0 },
//! #         })
//! #     }
//! #     fn drive<'de>(&mut self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
//! #         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
//! #         driver.emit(value)
//! #     }
//! # }
//! # impl Encoder for Lines {
//! #     fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error> {
//! #         let mut driver = deser::ser::SerializeDriver::new(value);
//! #         driver.drive(|event, _| { if let deser::Event::Atom(deser::Atom::U64(v)) = event { out.extend_from_slice(format!("{v}\n").as_bytes()); } Ok(()) })
//! #     }
//! # }
//! // `Lines` is a format with one number per line
//! let mut reader = Reader::new(&b"1\n2\n3\n"[..], Lines);
//! let mut writer = Writer::new(Vec::new(), Lines);
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
//! [`Decoder::drive`]).  This means that a value has to fit into memory as
//! a whole, but streams of values (like JSON Lines, CBOR sequences or YAML
//! documents) can be read with bounded memory and parsing is as fast as for
//! complete inputs.  Types can borrow from the frame (see
//! [`Reader::read_borrowed`]).
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
use std::io::{Read, Write};
use std::marker::PhantomData;

use crate::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use crate::error::{Error, ErrorKind};
use crate::ser::Serialize;

mod buffer;

pub use self::buffer::{DecodeBuffer, Status};

/// The result of [`Decoder::frame`].
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
    /// More input is needed for the next value.
    ///
    /// The first `consumed` bytes of the input are discarded, for instance
    /// whitespace before the next value.
    Incomplete { consumed: usize },
    /// There are no more values.
    ///
    /// This must only be returned at the end of the input.
    End,
}

/// Splits a stream into values and deserializes them.
///
/// Decoders are provided by data formats.  They are used with a
/// [`DecodeBuffer`] (for instance through a [`Reader`]), see the [module
/// documentation](self) for more information.
pub trait Decoder {
    /// Finds the next value in the input.
    ///
    /// The input holds the data that was read so far (minus the data that
    /// was discarded).  If it does not contain a complete value yet,
    /// [`Frame::Incomplete`] is returned and the method is invoked again
    /// once more data was read: the input then starts after the bytes that
    /// were consumed and continues with the new data.  This allows decoders
    /// to keep the state of their scan so they do not have to scan the input
    /// again.  `eof` is `true` if no more data follows the input.
    ///
    /// Once a value is complete, [`Frame::Value`] is returned and the value
    /// is deserialized with [`drive`](Self::drive).  The next call starts a
    /// new value, again after the consumed bytes.  Offsets of errors refer
    /// to the input.
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error>;

    /// Deserializes a value from its frame.
    ///
    /// The frame holds the bytes of a value found by
    /// [`frame`](Self::frame).  Offsets of errors refer to the frame.
    fn drive<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error>;
}

impl<D: Decoder + ?Sized> Decoder for &mut D {
    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        (**self).frame(input, eof)
    }

    fn drive<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        (**self).drive(frame, driver)
    }
}

/// Serializes values into the bytes of a stream.
///
/// Encoders are provided by data formats.  They are used by a [`Writer`]
/// (or the equivalent of an async runtime) to write a value or a stream of
/// values.  Encoders write everything that separates values in the
/// stream, for instance the line breaks of JSON Lines or the markers
/// between YAML documents.
pub trait Encoder {
    /// Serializes a value and appends its bytes to the output.
    ///
    /// If this fails, the output is left unchanged.
    fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error>;
}

impl<E: Encoder + ?Sized> Encoder for &mut E {
    fn encode(&mut self, value: &dyn Serialize, out: &mut Vec<u8>) -> Result<(), Error> {
        (**self).encode(value, out)
    }
}

/// Reads values from a [`Read`].
///
/// The values are split and deserialized with a [`Decoder`].  The reader
/// buffers the input so it does not need to be buffered.
pub struct Reader<R, D> {
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

    /// Reads until the next value is complete.
    ///
    /// Returns `false` if there are no more values.
    fn fill(&mut self) -> Result<bool, Error> {
        loop {
            match self.buffer.poll()? {
                Status::Ready => return Ok(true),
                Status::End => return Ok(false),
                Status::NeedInput => {}
            }
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
        }
    }

    /// Reads the next value.
    ///
    /// Returns `None` if there are no more values.  Whether reading can
    /// continue after an error depends on the decoder (for instance with
    /// JSON Lines it continues with the next line).
    pub fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        self.read_borrowed()
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
        if !self.fill()? {
            return Ok(None);
        }
        self.buffer
            .deserialize_with(|driver| setup(driver))
            .map(Some)
    }

    /// Reads the next value which can borrow from the reader's buffer.
    ///
    /// ```
    /// # use deser::io::{Decoder, Frame, Reader};
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
    /// #         driver.emit_borrowed(std::str::from_utf8(frame).unwrap())
    /// #     }
    /// # }
    /// // `Lines` is a format with one string per line
    /// let mut reader = Reader::new(&b"hello\nworld\n"[..], Lines);
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
pub struct Iter<'r, R, D, T> {
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
/// The values are serialized with an [`Encoder`].  Every value is written
/// with a single [`write_all`](Write::write_all), wrap the writer in a
/// [`BufWriter`](std::io::BufWriter) when writing many small values.
pub struct Writer<W, E> {
    writer: W,
    encoder: E,
    buffer: Vec<u8>,
}

impl<W: Write, E: Encoder> Writer<W, E> {
    /// Creates a writer.
    pub fn new(writer: W, encoder: E) -> Writer<W, E> {
        Writer {
            writer,
            encoder,
            buffer: Vec::new(),
        }
    }

    /// Serializes a value and writes it.
    ///
    /// If the value fails to serialize nothing is written.
    pub fn write(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        self.buffer.clear();
        self.encoder.encode(value, &mut self.buffer)?;
        self.writer.write_all(&self.buffer)?;
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
