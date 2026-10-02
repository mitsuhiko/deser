//! Reading and writing values from and to streams.
//!
//! This module requires the `io` feature (which is enabled by default).
//! For values in memory, the formats have deserializers (which read values
//! from slices) and serializers (which write values into buffers) that do
//! not need this module.
//!
//! Data formats parse complete inputs (slices) and serialize into complete
//! outputs.  For streams they have stream serializers and stream
//! deserializers which do not do IO themselves (see
//! [`deser::stream`](crate::stream)).  This module connects them to
//! [`Read`] and [`Write`], such as files, sockets or pipes.  A [`Reader`]
//! reads values with a
//! [`StreamDeserializer`], a [`Writer`]
//! writes values with a [`StreamSerializer`].
//! The configurations of the formats create them (`config.reader(input)`
//! and `config.writer(output)`):
//!
//! ```
//! # fn example() -> Result<(), deser::Error> {
//! use deser::io::{Reader, Writer};
//! # use deser::de::{DeserializeDriver, Frame, StreamDeserializer};
//! # use deser::ser::{SerializeDriver, Serializer, StreamSerializer};
//! # use deser::Error;
//! # /// A format with a number per line.
//! # #[derive(Default)]
//! # struct Lines(Vec<u8>);
//! # impl StreamDeserializer for Lines {
//! #     fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
//! #         Ok(match input.iter().position(|&b| b == b'\n') {
//! #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
//! #             None if eof && input.is_empty() => Frame::End,
//! #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
//! #             None => Frame::Incomplete { consumed: 0 },
//! #         })
//! #     }
//! #     fn drive_frame<'de>(&mut self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
//! #         let value: u64 = std::str::from_utf8(frame).unwrap().parse().unwrap();
//! #         driver.emit(value)
//! #     }
//! # }
//! # impl Serializer for Lines {
//! #     fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
//! #         driver.drive(|event, _| {
//! #             if let deser::Event::Atom(deser::Atom::U64(v)) = event {
//! #                 self.0.extend_from_slice(format!("{v}\n").as_bytes());
//! #             }
//! #             Ok(())
//! #         })
//! #     }
//! # }
//! # impl StreamSerializer for Lines {
//! #     fn output(&self) -> &[u8] { &self.0 }
//! #     fn clear_output(&mut self) { self.0.clear() }
//! # }
//! // `Lines` is the stream serializer and deserializer of a format with a
//! // number per line
//! let mut reader = Reader::new(&b"1\n2\n3\n"[..], Lines::default());
//! let mut writer = Writer::new(Vec::new(), Lines::default());
//! while let Some(value) = reader.read::<u64>()? {
//!     writer.write(&(value * 2))?;
//! }
//! assert_eq!(writer.into_inner(), b"2\n4\n6\n");
//! # Ok(()) } example().unwrap();
//! ```
//!
//! Readers deserialize values while their input arrives if the format
//! supports it (see [`StreamDeserializer::drive_partial`]),
//! otherwise the complete value is buffered first.  Values which borrow
//! from the reader's buffer are read with [`Reader::read_borrowed`].
//!
//! # Generic Code
//!
//! A [`Writer`] is a [`Serializer`] and a [`Reader`]
//! is a [`Deserializer`], so code which is generic
//! over serializers and deserializers (like `deser-transcode`) can write
//! to and read from streams.
//!
//! # Large Sequences
//!
//! Values which contain a large (or unbounded) sequence can be processed
//! while they are read: a [`Streamed`](crate::stream::Streamed) sequence hands out
//! its elements as they are read with [`Reader::read_next`] (and behaves
//! like a `Vec` otherwise).
//!
//! # Writing Large Values
//!
//! Formats whose output can be written before the value is complete (like
//! JSON and CBOR) serialize values in parts (see
//! [`StreamSerializer::drive_partial`]).
//! [`Writer::write`] uses this if possible: once the output of a value
//! exceeds the [buffer limit](Writer::set_buffer_limit), what was
//! serialized so far is written and the serialization continues, which
//! means that the memory used does not depend on the size of the values.
//! Values below the limit are written at once.  Output that can still
//! change (for instance the header of a container whose length is not
//! known upfront) is held back until it's final.
//!
//! # Errors
//!
//! Errors refer to positions in the stream: the offsets, lines and columns
//! of errors are relative to the start of the stream, not to the start of
//! the frame.  Failed reads and writes are errors of the kind
//! [`ErrorKind::Io`] with the IO error as source.
use core::any::Any;
use core::marker::PhantomData;
use std::io::{Read, Write};

use crate::de::{
    Deserialize, DeserializeDriver, DeserializeOwned, Deserializer, StreamDeserializer,
};
use crate::error::{Error, ErrorKind};
use crate::ser::{Serialize, SerializeDriver, Serializer, StreamSerializer};
use crate::stream::{
    DEFAULT_BUFFER_LIMIT, ElementReader, ElementStatus, InputBuffer, Part, Status,
};

/// Reads values from a [`Read`].
///
/// The values are split and deserialized with a [`StreamDeserializer`]
/// (for instance `deser_json::StreamDeserializer`, which the configuration
/// of the format creates with `config.reader(input)`).  The reader buffers
/// the input so it does not need to be buffered.
///
/// Readers implement [`Deserializer`]: every call to
/// [`drive`](Deserializer::drive) reads the next value.  Values read this
/// way cannot borrow from the reader, use
/// [`read_borrowed`](Self::read_borrowed) for that.
pub struct Reader<R, D: StreamDeserializer> {
    reader: R,
    buffer: InputBuffer<D>,
    // the value that is read with `read_next` (an `ElementReader`)
    pending: Option<Box<dyn Any + Send>>,
}

impl<R: Read, D: StreamDeserializer> Reader<R, D> {
    /// Creates a reader.
    ///
    /// To continue a stream whose context is known (for instance the
    /// names of the columns of a CSV file), create the stream deserializer
    /// with that context.
    pub fn new(reader: R, deserializer: D) -> Reader<R, D> {
        Reader {
            reader,
            buffer: InputBuffer::new(deserializer),
            pending: None,
        }
    }

    /// Fails if a value is being read with [`read_next`](Self::read_next).
    fn ensure_idle(&self) -> Result<(), Error> {
        match self.pending {
            Some(_) => Err(Error::new(
                ErrorKind::InvalidState,
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

    /// Feeds the next value into a driver.
    ///
    /// Returns `false` if there are no more values.
    fn drive_partial(&mut self, driver: &mut DeserializeDriver<'_, '_>) -> Result<bool, Error> {
        loop {
            match self.buffer.drive_partial(driver)? {
                Status::Ready => return Ok(true),
                Status::End => return Ok(false),
                Status::NeedInput => self.read_more()?,
            }
        }
    }

    /// Reads the next value.
    ///
    /// Returns `None` if there are no more values.  If the format supports
    /// it (see [`StreamDeserializer::supports_partial`]), the value is
    /// deserialized while the input is read which means that only
    /// incomplete tokens are buffered.  Otherwise the complete value is
    /// buffered first.  Whether reading can continue after an error depends
    /// on the format (for instance with JSON Lines it continues with the
    /// next line).
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
        if !self.buffer.supports_partial() {
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
            if !self.drive_partial(&mut driver)? {
                return Ok(None);
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
    /// #         driver.emit_borrowed(std::str::from_utf8(frame).unwrap())
    /// #     }
    /// # }
    /// use deser::io::Reader;
    ///
    /// // `Lines` is the stream deserializer of a format with a string per
    /// // line
    /// let mut reader = Reader::new(&b"hello\nworld\n"[..], Lines);
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

    /// Reads the next element of the [`Streamed`](crate::stream::Streamed) sequence of a value or
    /// the value.
    ///
    /// `T` is the type of the value and `E` the type of the elements of a
    /// [`Streamed<E>`](crate::stream::Streamed) sequence within it.  The elements are
    /// handed out as they are read ([`Part::Element`]), the value once it's
    /// complete ([`Part::Done`]).  The next call continues with the next
    /// value.  Returns `None` if there are no more values.  See [`Streamed`](crate::stream::Streamed)
    /// for an example.
    ///
    /// Until the value is complete, the reader can only be used to read the
    /// value with the same types.
    pub fn read_next<T, E>(&mut self) -> Result<Option<Part<E, T>>, Error>
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
                        ErrorKind::InvalidState,
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

    /// Returns `true` if there are no more values.
    ///
    /// This reads until the start of the next value or the end of the
    /// stream.  If the format cannot find the start of a value on its own
    /// (see [`StreamDeserializer::peek`]), the next value is buffered
    /// completely.  This is useful to read values with the reader's
    /// [`Deserializer`] implementation:
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
    /// #         driver.emit_borrowed(std::str::from_utf8(frame).unwrap())
    /// #     }
    /// # }
    /// use deser::de::Deserializer;
    /// use deser::io::Reader;
    ///
    /// // `Lines` is the stream deserializer of a format with a string per
    /// // line
    /// let mut reader = Reader::new(&b"hello\nworld\n"[..], Lines);
    /// let mut values = Vec::new();
    /// while !reader.is_end().unwrap() {
    ///     values.push(reader.deserialize::<String>().unwrap());
    /// }
    /// assert_eq!(values, ["hello", "world"]);
    /// ```
    pub fn is_end(&mut self) -> Result<bool, Error> {
        self.ensure_idle()?;
        loop {
            match self.buffer.peek()? {
                Status::Ready => return Ok(false),
                Status::End => return Ok(true),
                Status::NeedInput => self.read_more()?,
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

    /// Returns the stream deserializer.
    ///
    /// This gives access to what the stream established so far, for
    /// instance the names of the columns of a CSV file.
    pub fn deserializer(&self) -> &D {
        self.buffer.deserializer()
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

    /// Returns the underlying reader and the stream deserializer.
    ///
    /// Data that was read into the buffer but not deserialized yet is lost.
    pub fn into_parts(self) -> (R, D) {
        (self.reader, self.buffer.into_parts().0)
    }
}

/// Reads the next value.
///
/// Values cannot borrow from the reader: borrowed data is passed on like
/// data that is only valid for the call (see
/// [`DeserializeDriver::transient`]).  If there are no more values this
/// fails (see [`Reader::is_end`]).
impl<'de, R: Read, D: StreamDeserializer> Deserializer<'de> for Reader<R, D> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
        self.ensure_idle()?;
        let found = if self.buffer.supports_partial() {
            self.drive_partial(driver)?
        } else if self.fill()? {
            self.buffer.drive_transient(driver)?;
            true
        } else {
            false
        };
        match found {
            true => Ok(()),
            false => Err(Error::new(ErrorKind::EndOfFile, "empty input")),
        }
    }
}

/// An iterator over the values of a [`Reader`].
pub struct Iter<'r, R, D: StreamDeserializer, T> {
    reader: &'r mut Reader<R, D>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<R: Read, D: StreamDeserializer, T: DeserializeOwned> Iterator for Iter<'_, R, D, T> {
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
/// The values are serialized with a [`StreamSerializer`] (the serializer
/// of a format, which the configuration of the format creates with
/// `config.writer(output)`).  The output of a value is written with
/// [`write_all`](Write::write_all), wrap the writer in a
/// [`BufWriter`](std::io::BufWriter) when writing many small values.  If
/// the format supports it, the output of large values is written in parts
/// while they are serialized (see
/// [`set_buffer_limit`](Self::set_buffer_limit)).
///
/// Writers implement [`Serializer`]: [`serialize`](Serializer::serialize)
/// is the same as [`write`](Self::write).
pub struct Writer<W, S: StreamSerializer> {
    writer: W,
    serializer: S,
    limit: usize,
}

impl<W: Write, S: StreamSerializer> Writer<W, S> {
    /// Creates a writer.
    ///
    /// To append to a stream that was written before, create the
    /// serializer with the state of the stream (for instance the number of
    /// values that were written).
    pub fn new(writer: W, serializer: S) -> Writer<W, S> {
        Writer {
            writer,
            serializer,
            limit: DEFAULT_BUFFER_LIMIT,
        }
    }

    /// Sets how much output of a value is buffered before it's written.
    ///
    /// If the format supports it (see
    /// [`StreamSerializer::supports_partial`]), the output of a value is
    /// written once it exceeds the limit, the serialization continues
    /// after that.  This way the memory used does not depend on the size of
    /// the values.  The default is [`DEFAULT_BUFFER_LIMIT`] (8 KiB).  With
    /// `usize::MAX` every value is serialized completely before it's
    /// written.
    ///
    /// ```
    /// use deser::io::Writer;
    /// # use deser::ser::{EventSink, SerializeDriver, SerializeRef, Serializer, StreamSerializer};
    /// # use deser::{Error, Event, State};
    /// # /// Writes `x` for every event, can stop between values.
    /// # #[derive(Default)]
    /// # struct Xs { out: Vec<u8>, partial: bool }
    /// # struct Sink<'a>(&'a mut Vec<u8>, usize);
    /// # impl EventSink for Sink<'_> {
    /// #     fn event(&mut self, _: Event<'_>, _: SerializeRef<'_>, _: &mut State) -> Result<(), Error> {
    /// #         self.0.push(b'x');
    /// #         Ok(())
    /// #     }
    /// #     fn pause(&mut self) -> bool {
    /// #         self.0.len() >= self.1
    /// #     }
    /// # }
    /// # impl Serializer for Xs {
    /// #     fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
    /// #         driver.drive(|_, _| Ok(self.out.push(b'x')))
    /// #     }
    /// # }
    /// # impl StreamSerializer for Xs {
    /// #     fn output(&self) -> &[u8] { &self.out }
    /// #     fn clear_output(&mut self) { self.out.clear() }
    /// #     fn supports_partial(&self) -> bool { true }
    /// #     fn drive_partial(&mut self, driver: &mut SerializeDriver<'_>, limit: usize) -> Result<bool, Error> {
    /// #         let done = driver.drive_until(&mut Sink(&mut self.out, limit))?;
    /// #         self.partial = !done;
    /// #         Ok(done)
    /// #     }
    /// #     fn in_progress(&self) -> bool { self.partial }
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
    /// let mut writer = Writer::new(Counter(0), Xs::default());
    /// writer.set_buffer_limit(100);
    /// writer.write(&vec![0; 20_000]).unwrap();
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
    /// more values (see [`StreamSerializer::in_progress`]).
    pub fn write<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.write_driver(&mut SerializeDriver::new(&value))
    }

    /// Serializes a value with a configured driver and writes it.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](crate::ser::Layer)s.
    pub fn write_with<T, F>(&mut self, value: &T, setup: F) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(&value);
        setup(&mut driver);
        self.write_driver(&mut driver)
    }

    /// Serializes the value of a driver and writes it.
    fn write_driver(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        if self.serializer.in_progress() {
            return Err(Error::in_progress());
        }
        // output that was not written (for instance of values serialized
        // before the serializer was given to the writer) comes first
        write_output(&mut self.writer, &mut self.serializer)?;
        let limit = match self.serializer.supports_partial() {
            true => self.limit.max(1),
            false => usize::MAX,
        };
        loop {
            let done = self.serializer.drive_partial(driver, limit)?;
            write_output(&mut self.writer, &mut self.serializer)?;
            if done {
                return Ok(());
            }
        }
    }

    /// Flushes the underlying writer.
    pub fn flush(&mut self) -> Result<(), Error> {
        self.writer.flush()?;
        Ok(())
    }

    /// Returns the stream serializer.
    ///
    /// This gives access to the state of the stream, for instance the
    /// number of values that were written.
    pub fn serializer(&self) -> &S {
        &self.serializer
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

    /// Returns the underlying writer and the stream serializer.
    pub fn into_parts(self) -> (W, S) {
        (self.writer, self.serializer)
    }
}

/// Writes the output of a serializer and clears it.
fn write_output<W: Write, S: StreamSerializer>(
    writer: &mut W,
    serializer: &mut S,
) -> Result<(), Error> {
    let output = serializer.output();
    if output.is_empty() {
        return Ok(());
    }
    // the output is gone even if the write fails, a part of it might have
    // been written
    let rv = writer.write_all(output);
    serializer.clear_output();
    rv.map_err(Error::from)
}

/// Serializes values and writes them, like [`Writer::write`].
impl<W: Write, S: StreamSerializer> Serializer for Writer<W, S> {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        self.write_driver(driver)
    }
}

/// Reads a single value from a [`Read`].
///
/// Fails if there is no value or if another value follows it.
pub fn from_reader<T, R, D>(reader: R, deserializer: D) -> Result<T, Error>
where
    T: DeserializeOwned,
    R: Read,
    D: StreamDeserializer,
{
    let mut reader = Reader::new(reader, deserializer);
    let value = reader
        .read()?
        .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    reader.end()?;
    Ok(value)
}

/// Writes a single value to a [`Write`].
pub fn to_writer<W, S, T>(writer: W, serializer: S, value: &T) -> Result<(), Error>
where
    W: Write,
    S: StreamSerializer,
    T: Serialize + ?Sized,
{
    Writer::new(writer, serializer).write(value)
}
