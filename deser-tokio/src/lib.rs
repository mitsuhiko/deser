//! Read and write [deser](https://docs.rs/deser) values with
//! [tokio](https://tokio.rs).
//!
//! This crate connects the stream serializers and stream deserializers of
//! the data formats (which implement [`StreamSerializer`] and
//! [`StreamDeserializer`], see [`deser::stream`](deser_core::stream)) to
//! tokio's [`AsyncRead`] and [`AsyncWrite`].  It works with every format.  Values
//! of formats which support it (like JSON and CBOR) are deserialized while
//! their input arrives, other values are buffered until they are complete,
//! so streams of values (like JSON Lines, CBOR sequences or YAML documents)
//! can be read from sockets with bounded memory:
//!
//! ```
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), deser::Error> {
//! use deser::{Deserialize, Serialize};
//! use deser_json::{DeserializerConfig, Serializer, SerializerConfig, StreamDeserializer, Trailing};
//! use deser_tokio::{Reader, Writer};
//!
//! const READ_LINES: DeserializerConfig =
//!     DeserializerConfig::builder().trailing(Trailing::Newline).build();
//! const WRITE_LINES: SerializerConfig =
//!     SerializerConfig::builder().trailing(Trailing::Newline).build();
//!
//! #[derive(Debug, Serialize, Deserialize)]
//! struct Request {
//!     id: u64,
//!     method: String,
//! }
//!
//! # let (client, server) = tokio::io::duplex(1024);
//! # let client = tokio::spawn(async move {
//! #     let (input, output) = tokio::io::split(client);
//! #     let mut requests = Writer::new(output, Serializer::with_config(&WRITE_LINES));
//! #     requests.write(&Request { id: 1, method: "ping".into() }).await.unwrap();
//! #     requests.shutdown().await.unwrap();
//! #     let mut responses = Reader::new(input, StreamDeserializer::with_config(&READ_LINES));
//! #     assert_eq!(responses.read::<u64>().await.unwrap(), Some(1));
//! # });
//! let (input, output) = tokio::io::split(server);
//!
//! // JSON Lines in, JSON Lines out
//! let mut requests = Reader::new(input, StreamDeserializer::with_config(&READ_LINES));
//! let mut responses = Writer::new(output, Serializer::with_config(&WRITE_LINES));
//! while let Some(request) = requests.read::<Request>().await? {
//!     responses.write(&request.id).await?;
//! }
//! # client.await.unwrap();
//! # Ok(()) }
//! ```
//!
//! Single values are read with [`from_reader`] and written with
//! [`to_writer`].  With the `codec` feature, [`Codec`] implements the codec
//! traits of [`tokio-util`](https://docs.rs/tokio-util) for use with
//! `FramedRead`, `FramedWrite` and `Framed`.
//!
//! # Multi-Threaded Runtimes
//!
//! The futures are `Send` if the reader or writer and the stream
//! deserializer or serializer are, so they can be spawned on multi-threaded
//! runtimes.
//!
//! # Large Values
//!
//! Formats which support it (like JSON and CBOR) serialize values in
//! parts: once the output of a value exceeds the
//! [buffer limit](Writer::set_buffer_limit), what was serialized so far is
//! written and the serialization continues after that.  The memory used for
//! writing does not depend on the size of the values either.
//!
//! # Cancellation
//!
//! Reading is cancellation safe: if a future that reads a value is
//! dropped, the data read so far stays in the buffer of the [`Reader`] and
//! the next read continues with it.  This allows reading in
//! `tokio::select!`.  Writing is not cancellation safe, a value might have
//! been written partially.  A [`Writer`] refuses to write more values after
//! a value was abandoned after a part of it was written.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::any::Any;
use std::future::poll_fn;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use deser_core::de::{
    Deserialize, DeserializeDriver, DeserializeOwned, OwnedDriver, StreamDeserializer,
};
use deser_core::ser::{Serialize, SerializeDriver, StreamSerializer};
use deser_core::stream::{
    DEFAULT_BUFFER_LIMIT, ElementReader, ElementStatus, InputBuffer, Part, Status,
};
use deser_core::{Error, ErrorKind};
use futures_core::Stream;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

#[cfg(feature = "codec")]
mod codec;

#[cfg(feature = "codec")]
pub use self::codec::Codec;

/// Reads values from an [`AsyncRead`].
///
/// The values are split and deserialized with a [`StreamDeserializer`] (for
/// instance `deser_json::StreamDeserializer`).  The reader buffers the
/// input so it does not need to be buffered.  If the format supports it
/// (see [`StreamDeserializer::supports_partial`]), values are deserialized
/// while their input arrives which means that only incomplete tokens are
/// buffered.
pub struct Reader<R, D: StreamDeserializer> {
    reader: R,
    buffer: InputBuffer<D>,
    // a value that is being deserialized while its input arrives (an
    // `OwnedDriver<'static, T>`), kept when a read is cancelled.
    pending: Option<Box<dyn Any + Send>>,
}

// the reader is never pinned structurally
impl<R, D: StreamDeserializer> Unpin for Reader<R, D> {}

impl<R: AsyncRead + Unpin, D: StreamDeserializer> Reader<R, D> {
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

    /// Sets the context the values are deserialized in.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)).  A context set
    /// by the callback of [`read_with`](Self::read_with) takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.buffer.set_context(context);
    }

    /// Returns the context the values are deserialized in.
    pub fn context(&self) -> &deser_core::Context {
        self.buffer.context()
    }

    /// Reads more input into the buffer.
    fn poll_read_more(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let mut buf = ReadBuf::new(self.buffer.read_buf());
        ready!(Pin::new(&mut self.reader).poll_read(cx, &mut buf))?;
        let read = buf.filled().len();
        if read == 0 {
            self.buffer.set_eof();
        } else {
            self.buffer.filled(read);
        }
        Poll::Ready(Ok(()))
    }

    /// Reads until the frame of the next value is complete.
    ///
    /// Resolves to `false` if there are no more values.
    fn poll_fill(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, Error>> {
        loop {
            match self.buffer.poll()? {
                Status::Ready => return Poll::Ready(Ok(true)),
                Status::End => return Poll::Ready(Ok(false)),
                Status::NeedInput => ready!(self.poll_read_more(cx))?,
            }
        }
    }

    /// Reads the next value, the setup is invoked with its driver.
    fn poll_read_setup<T, F>(
        &mut self,
        cx: &mut Context<'_>,
        setup: &mut Option<F>,
    ) -> Poll<Result<Option<T>, Error>>
    where
        T: DeserializeOwned + 'static,
        F: FnOnce(&mut DeserializeDriver<'_, '_>),
    {
        if !self.buffer.supports_partial() {
            if !ready!(self.poll_fill(cx))? {
                return Poll::Ready(Ok(None));
            }
            let setup = setup.take();
            return Poll::Ready(
                self.buffer
                    .deserialize_with(|driver| {
                        if let Some(setup) = setup {
                            setup(driver);
                        }
                    })
                    .map(Some),
            );
        }

        // continue a value that is being read or start a new one
        let mut driver = match self.pending.take() {
            Some(pending) => match pending.downcast::<OwnedDriver<'static, T>>() {
                Ok(driver) => *driver,
                Err(pending) => {
                    self.pending = Some(pending);
                    return Poll::Ready(Err(Error::new(
                        ErrorKind::InvalidState,
                        "a value of another type is being read",
                    )));
                }
            },
            None => {
                let mut driver = OwnedDriver::<'static, T>::new();
                if let Some(setup) = setup.take() {
                    driver.with(|driver| setup(driver));
                }
                driver
            }
        };
        loop {
            match driver.with(|driver| self.buffer.drive_partial(driver))? {
                Status::Ready => return Poll::Ready(driver.finish().map(Some)),
                Status::End => return Poll::Ready(Ok(None)),
                Status::NeedInput => match self.poll_read_more(cx) {
                    Poll::Ready(Ok(())) => {}
                    rv => {
                        // the value continues with the next read
                        self.pending = Some(Box::new(driver));
                        return rv.map(|rv| rv.map(|_| None));
                    }
                },
            }
        }
    }

    /// Polls for the next value.
    ///
    /// This is the poll based version of [`read`](Self::read).
    pub fn poll_read<T: DeserializeOwned + 'static>(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<T>, Error>> {
        self.poll_read_setup(cx, &mut None::<fn(&mut DeserializeDriver<'_, '_>)>)
    }

    /// Reads the next value.
    ///
    /// Resolves to `None` if there are no more values.  If the future is
    /// dropped before it resolves, the next read continues where it
    /// stopped (a value of another type cannot be read then).  Whether
    /// reading can continue after an error depends on the format (for
    /// instance with JSON Lines it continues with the next line).
    pub async fn read<T: DeserializeOwned + 'static>(&mut self) -> Result<Option<T>, Error> {
        poll_fn(|cx| self.poll_read(cx)).await
    }

    /// Reads the next value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser_core::de::Layer)s.
    pub async fn read_with<T, F>(&mut self, setup: F) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned + 'static,
        F: FnOnce(&mut DeserializeDriver<'_, '_>),
    {
        let mut setup = Some(setup);
        poll_fn(|cx| self.poll_read_setup(cx, &mut setup)).await
    }

    /// Polls for the next element of the [`Streamed`](deser_core::stream::Streamed) sequence of a value or
    /// the value.
    ///
    /// This is the poll based version of [`read_next`](Self::read_next).
    pub fn poll_read_next<T, E>(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Part<E, T>>, Error>>
    where
        T: DeserializeOwned + 'static,
        E: Send + 'static,
    {
        let mut reader = match self.pending.take() {
            Some(pending) => match pending.downcast::<ElementReader<T, E>>() {
                Ok(reader) => reader,
                Err(pending) => {
                    self.pending = Some(pending);
                    return Poll::Ready(Err(Error::new(
                        ErrorKind::InvalidState,
                        "a value of another type is being read",
                    )));
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
                    return Poll::Ready(Ok(Some(next)));
                }
                ElementStatus::End => return Poll::Ready(Ok(None)),
                ElementStatus::NeedInput => match self.poll_read_more(cx) {
                    Poll::Ready(Ok(())) => {}
                    rv => {
                        // the value continues with the next read
                        self.pending = Some(reader);
                        return rv.map(|rv| rv.map(|_| None));
                    }
                },
            }
        }
    }

    /// Reads the next element of the [`Streamed`](deser_core::stream::Streamed) sequence of a value or
    /// the value.
    ///
    /// `T` is the type of the value and `E` the type of the elements of a
    /// [`Streamed<E>`](deser_core::stream::Streamed) sequence within it.  The elements are
    /// handed out as they are read ([`Part::Element`]), the value once it's
    /// complete ([`Part::Done`]).  The next call continues with the next
    /// value.  Resolves to `None` if there are no more values.  This is
    /// cancellation safe, until the value is complete the reader can only
    /// be used to read the value with the same types.
    pub async fn read_next<T, E>(&mut self) -> Result<Option<Part<E, T>>, Error>
    where
        T: DeserializeOwned + 'static,
        E: Send + 'static,
    {
        poll_fn(|cx| self.poll_read_next(cx)).await
    }

    /// Converts the reader into a [`Stream`] of the elements of the
    /// [`Streamed`](deser_core::stream::Streamed) sequence of values and the values.
    ///
    /// See [`read_next`](Self::read_next).  The stream ends after the first
    /// error.
    pub fn into_element_stream<T, E>(self) -> ElementStream<R, D, T, E>
    where
        T: DeserializeOwned + 'static,
        E: Send + 'static,
    {
        ElementStream {
            reader: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Reads the next value which can borrow from the reader's buffer.
    ///
    /// The complete value is buffered first.
    pub async fn read_borrowed<'a, T: Deserialize<'a>>(&'a mut self) -> Result<Option<T>, Error> {
        if !poll_fn(|cx| self.poll_fill(cx)).await? {
            return Ok(None);
        }
        self.buffer.deserialize().map(Some)
    }

    /// Returns `true` if there are no more values.
    ///
    /// This reads until the start of the next value or the end of the
    /// stream (see
    /// [`deser::io::Reader::is_end`](https://docs.rs/deser/latest/deser/io/struct.Reader.html#method.is_end)).
    pub async fn is_end(&mut self) -> Result<bool, Error> {
        poll_fn(|cx| {
            loop {
                match self.buffer.peek()? {
                    Status::Ready => return Poll::Ready(Ok(false)),
                    Status::End => return Poll::Ready(Ok(true)),
                    Status::NeedInput => ready!(self.poll_read_more(cx))?,
                }
            }
        })
        .await
    }

    /// Checks that there are no more values.
    ///
    /// Fails if another value follows (or if the data that follows is not
    /// valid).
    pub async fn end(&mut self) -> Result<(), Error> {
        if poll_fn(|cx| self.poll_fill(cx)).await? {
            return Err(self.buffer.trailing_error());
        }
        Ok(())
    }

    /// Converts the reader into a [`Stream`] of values.
    ///
    /// The stream ends after the first error.
    pub fn into_stream<T: DeserializeOwned + 'static>(self) -> ReaderStream<R, D, T> {
        ReaderStream {
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
}

/// A [`Stream`] of the values of a [`Reader`].
///
/// Created with [`Reader::into_stream`].
pub struct ReaderStream<R, D: StreamDeserializer, T> {
    reader: Reader<R, D>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<R, D: StreamDeserializer, T> Unpin for ReaderStream<R, D, T> {}

impl<R, D: StreamDeserializer, T> ReaderStream<R, D, T> {
    /// Returns the reader.
    pub fn into_inner(self) -> Reader<R, D> {
        self.reader
    }
}

impl<R: AsyncRead + Unpin, D: StreamDeserializer, T: DeserializeOwned + 'static> Stream
    for ReaderStream<R, D, T>
{
    type Item = Result<T, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.failed {
            return Poll::Ready(None);
        }
        match ready!(self.reader.poll_read(cx)) {
            Ok(value) => Poll::Ready(value.map(Ok)),
            Err(err) => {
                self.failed = true;
                Poll::Ready(Some(Err(err)))
            }
        }
    }
}

/// A [`Stream`] of the elements of the [`Streamed`](deser_core::stream::Streamed) sequence of values and
/// the values.
///
/// Created with [`Reader::into_element_stream`].
pub struct ElementStream<R, D: StreamDeserializer, T, E> {
    reader: Reader<R, D>,
    failed: bool,
    _marker: PhantomData<fn() -> (T, E)>,
}

impl<R, D: StreamDeserializer, T, E> Unpin for ElementStream<R, D, T, E> {}

impl<R, D: StreamDeserializer, T, E> ElementStream<R, D, T, E> {
    /// Returns the reader.
    pub fn into_inner(self) -> Reader<R, D> {
        self.reader
    }
}

impl<R, D, T, E> Stream for ElementStream<R, D, T, E>
where
    R: AsyncRead + Unpin,
    D: StreamDeserializer,
    T: DeserializeOwned + 'static,
    E: Send + 'static,
{
    type Item = Result<Part<E, T>, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.failed {
            return Poll::Ready(None);
        }
        match ready!(self.reader.poll_read_next(cx)) {
            Ok(next) => Poll::Ready(next.map(Ok)),
            Err(err) => {
                self.failed = true;
                Poll::Ready(Some(Err(err)))
            }
        }
    }
}

/// Writes values to an [`AsyncWrite`].
///
/// The values are serialized with a [`StreamSerializer`] (the serializer
/// of a data format, for instance `deser_json::Serializer`) and its output
/// is written with [`write_all`](tokio::io::AsyncWriteExt::write_all), wrap
/// the writer in a [`BufWriter`](tokio::io::BufWriter) when writing many
/// small values (and [`flush`](Self::flush) it).  If the format supports it
/// (see [`StreamSerializer::supports_partial`]), the output of large values
/// is written in parts while they are serialized, so the memory used does
/// not depend on the size of the values (see
/// [`set_buffer_limit`](Self::set_buffer_limit)).
pub struct Writer<W, S: StreamSerializer> {
    writer: W,
    serializer: S,
    limit: usize,
    // output is being written, if the future is dropped meanwhile it's
    // unknown what was written
    writing: bool,
    // the context the values are serialized in
    context: deser_core::Context,
}

impl<W: AsyncWrite + Unpin, S: StreamSerializer> Writer<W, S> {
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
            writing: false,
            context: deser_core::Context::new(),
        }
    }

    /// Sets the context the values are serialized in.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)).  A context set
    /// by the callback of [`write_with`](Self::write_with) takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.context = context;
    }

    /// Returns the context the values are serialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.context
    }

    /// Sets how much output of a value is buffered before it's written.
    ///
    /// If the format supports it, the output of a value is written once it
    /// exceeds the limit, the serialization continues after that.  The
    /// default is [`DEFAULT_BUFFER_LIMIT`] (8 KiB).  With `usize::MAX`
    /// every value is serialized completely before it's written.  See
    /// [`deser::io::Writer::set_buffer_limit`](https://docs.rs/deser/latest/deser/io/struct.Writer.html#method.set_buffer_limit).
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
    /// it was written, because it fails to serialize, a write fails or the
    /// future is dropped, the stream holds an incomplete value and the
    /// writer refuses to write more values.
    pub async fn write<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.write_with(value, |_| {}).await
    }

    /// Serializes a value with a configured driver and writes it.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub async fn write_with<T, F>(&mut self, value: &T, setup: F) -> Result<(), Error>
    where
        T: Serialize + ?Sized,
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        if self.writing || self.serializer.in_progress() {
            return Err(Error::new(
                ErrorKind::InvalidState,
                "a value was only partially written, the stream cannot continue",
            ));
        }
        let mut driver = SerializeDriver::new(&value);
        setup(&mut driver);
        driver.state_mut().set_default_context(self.context.clone());
        // output that was not written (for instance of values serialized
        // before the serializer was given to the writer) comes first
        self.write_output().await?;
        let limit = match self.serializer.supports_partial() {
            true => self.limit.max(1),
            false => usize::MAX,
        };
        loop {
            let done = self.serializer.drive_partial(&mut driver, limit)?;
            self.write_output().await?;
            if done {
                return Ok(());
            }
        }
    }

    /// Writes the output of the serializer and clears it.
    async fn write_output(&mut self) -> Result<(), Error> {
        if self.serializer.output().is_empty() {
            return Ok(());
        }
        // stays set if the future is dropped or the write fails
        self.writing = true;
        self.writer.write_all(self.serializer.output()).await?;
        self.serializer.clear_output();
        self.writing = false;
        Ok(())
    }

    /// Flushes the underlying writer.
    pub async fn flush(&mut self) -> Result<(), Error> {
        self.writer.flush().await?;
        Ok(())
    }

    /// Shuts down the underlying writer.
    pub async fn shutdown(&mut self) -> Result<(), Error> {
        self.writer.shutdown().await?;
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

/// Reads a single value from an [`AsyncRead`].
///
/// Fails if there is no value or if another value follows it.  How the
/// value is read depends on the format, for instance with JSON the reader
/// is read to the end.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let input = &b"[1, 2, 3]"[..];
/// let de = deser_json::StreamDeserializer::new();
/// let value: Vec<u32> = deser_tokio::from_reader(input, de).await.unwrap();
/// assert_eq!(value, [1, 2, 3]);
/// # }
/// ```
pub async fn from_reader<T, R, D>(reader: R, deserializer: D) -> Result<T, Error>
where
    T: DeserializeOwned + 'static,
    R: AsyncRead + Unpin,
    D: StreamDeserializer,
{
    let mut reader = Reader::new(reader, deserializer);
    let value = reader
        .read()
        .await?
        .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))?;
    reader.end().await?;
    Ok(value)
}

/// Writes a single value to an [`AsyncWrite`] and flushes it.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let mut out = Vec::new();
/// let ser = deser_json::Serializer::new();
/// deser_tokio::to_writer(&mut out, ser, &vec![1, 2]).await.unwrap();
/// assert_eq!(out, b"[1,2]");
/// # }
/// ```
pub async fn to_writer<W, S, T>(writer: W, serializer: S, value: &T) -> Result<(), Error>
where
    W: AsyncWrite + Unpin,
    S: StreamSerializer,
    T: Serialize + ?Sized,
{
    let mut writer = Writer::new(writer, serializer);
    writer.write(value).await?;
    writer.flush().await
}
