//! Read and write [deser](https://docs.rs/deser) values with
//! [tokio](https://tokio.rs).
//!
//! This crate connects the configurations of the data formats (which
//! implement [`Decoder`] and [`Encoder`], see [`deser::io`]) to tokio's
//! [`AsyncRead`] and [`AsyncWrite`].  It works
//! with every format and only buffers until a value is complete, so
//! streams of values (like JSON Lines, CBOR sequences or YAML documents)
//! can be read from sockets with bounded memory:
//!
//! ```
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), deser::Error> {
//! use deser::{Deserialize, Serialize};
//! use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
//! use deser_tokio::{Reader, Writer};
//!
//! const READ_LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
//! const WRITE_LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);
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
//! #     let mut requests = Writer::new(output, WRITE_LINES);
//! #     requests.write(&Request { id: 1, method: "ping".into() }).await.unwrap();
//! #     requests.shutdown().await.unwrap();
//! #     let mut responses = Reader::new(input, READ_LINES);
//! #     assert_eq!(responses.read::<u64>().await.unwrap(), Some(1));
//! # });
//! let (input, output) = tokio::io::split(server);
//!
//! // JSON Lines in, JSON Lines out
//! let mut requests = Reader::new(input, READ_LINES);
//! let mut responses = Writer::new(output, WRITE_LINES);
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
//! Values are only deserialized once they are complete and serialized
//! before they are written, so no deserialization or serialization is in
//! progress while the futures of this crate wait for IO.  The futures are
//! `Send` if the reader or writer and the decoder or encoder are, so they
//! can be spawned on multi-threaded runtimes.
//!
//! # Cancellation
//!
//! Reading is cancellation safe: if a future that reads a value is
//! dropped, the data read so far stays in the buffer of the [`Reader`] and
//! the next read continues with it.  This allows reading in
//! `tokio::select!`.  Writing is not cancellation safe, a value might have
//! been written partially.
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::future::poll_fn;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use deser::de::Decoder;
use deser::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser::io::{DecodeBuffer, Status};
use deser::ser::Encoder;
use deser::ser::{Serialize, SerializeDriver};
use deser::{Error, ErrorKind};
use futures_core::Stream;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

#[cfg(feature = "codec")]
mod codec;

#[cfg(feature = "codec")]
pub use self::codec::Codec;

/// Reads values from an [`AsyncRead`].
///
/// The values are split and deserialized with a [`Decoder`] (the
/// deserializer configuration of a data format).  The reader buffers the input so it does not need to be
/// buffered.
pub struct Reader<R, D: Decoder> {
    reader: R,
    buffer: DecodeBuffer<D>,
}

// the reader is never pinned structurally
impl<R, D: Decoder> Unpin for Reader<R, D> {}

impl<R: AsyncRead + Unpin, D: Decoder> Reader<R, D> {
    /// Creates a reader.
    pub fn new(reader: R, decoder: D) -> Reader<R, D> {
        Reader {
            reader,
            buffer: DecodeBuffer::new(decoder),
        }
    }

    /// Reads until the next value is complete.
    ///
    /// Resolves to `false` if there are no more values.
    fn poll_fill(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, Error>> {
        loop {
            match self.buffer.poll()? {
                Status::Ready => return Poll::Ready(Ok(true)),
                Status::End => return Poll::Ready(Ok(false)),
                Status::NeedInput => {}
            }
            let mut buf = ReadBuf::new(self.buffer.read_buf());
            ready!(Pin::new(&mut self.reader).poll_read(cx, &mut buf))?;
            let read = buf.filled().len();
            if read == 0 {
                self.buffer.set_eof();
            } else {
                self.buffer.filled(read);
            }
        }
    }

    /// Polls for the next value.
    ///
    /// This is the poll based version of [`read`](Self::read).
    pub fn poll_read<T: DeserializeOwned>(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<T>, Error>> {
        if !ready!(self.poll_fill(cx))? {
            return Poll::Ready(Ok(None));
        }
        Poll::Ready(self.buffer.deserialize().map(Some))
    }

    /// Reads the next value.
    ///
    /// Resolves to `None` if there are no more values.  Whether reading can
    /// continue after an error depends on the decoder (for instance with
    /// JSON Lines it continues with the next line).
    pub async fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        poll_fn(|cx| self.poll_read(cx)).await
    }

    /// Reads the next value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser::de::Layer)s.
    pub async fn read_with<T, F>(&mut self, setup: F) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
        F: FnOnce(&mut DeserializeDriver<'_, '_>),
    {
        if !poll_fn(|cx| self.poll_fill(cx)).await? {
            return Ok(None);
        }
        self.buffer
            .deserialize_with(|driver| setup(driver))
            .map(Some)
    }

    /// Reads the next value which can borrow from the reader's buffer.
    pub async fn read_borrowed<'a, T: Deserialize<'a>>(&'a mut self) -> Result<Option<T>, Error> {
        if !poll_fn(|cx| self.poll_fill(cx)).await? {
            return Ok(None);
        }
        self.buffer.deserialize().map(Some)
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
    pub fn into_stream<T: DeserializeOwned>(self) -> ReaderStream<R, D, T> {
        ReaderStream {
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

/// A [`Stream`] of the values of a [`Reader`].
///
/// Created with [`Reader::into_stream`].
pub struct ReaderStream<R, D: Decoder, T> {
    reader: Reader<R, D>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<R, D: Decoder, T> Unpin for ReaderStream<R, D, T> {}

impl<R, D: Decoder, T> ReaderStream<R, D, T> {
    /// Returns the reader.
    pub fn into_inner(self) -> Reader<R, D> {
        self.reader
    }
}

impl<R: AsyncRead + Unpin, D: Decoder, T: DeserializeOwned> Stream for ReaderStream<R, D, T> {
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

/// Writes values to an [`AsyncWrite`].
///
/// The values are serialized with an [`Encoder`] (the serializer
/// configuration of a data format).  Every
/// value is written with a single
/// [`write_all`](tokio::io::AsyncWriteExt::write_all), wrap the writer in a
/// [`BufWriter`](tokio::io::BufWriter) when writing many small values (and
/// [`flush`](Self::flush) it).
pub struct Writer<W, E> {
    writer: W,
    encoder: E,
    buffer: Vec<u8>,
    written: usize,
}

impl<W: AsyncWrite + Unpin, E: Encoder> Writer<W, E> {
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
    /// The value is serialized before anything is written.  If it fails to
    /// serialize, nothing is written.
    pub async fn write(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        self.write_with(value, |_| {}).await
    }

    /// Serializes a value with a configured driver and writes it.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser::ser::Layer)s.
    pub async fn write_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        deser::io::encode(&self.encoder, value, setup, self.written, &mut self.buffer)?;
        self.writer.write_all(&self.buffer).await?;
        self.written += 1;
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

/// Reads a single value from an [`AsyncRead`].
///
/// Fails if there is no value or if another value follows it.  How the
/// value is read depends on the decoder, for instance with JSON the reader
/// is read to the end.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let input = &b"[1, 2, 3]"[..];
/// let config = deser_json::DeserializerConfig::new();
/// let value: Vec<u32> = deser_tokio::from_reader(input, config)
///     .await
///     .unwrap();
/// assert_eq!(value, [1, 2, 3]);
/// # }
/// ```
pub async fn from_reader<T, R, D>(reader: R, decoder: D) -> Result<T, Error>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
    D: Decoder,
{
    let mut reader = Reader::new(reader, decoder);
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
/// deser_tokio::to_writer(&mut out, deser_json::SerializerConfig::new(), &vec![1, 2])
///     .await
///     .unwrap();
/// assert_eq!(out, b"[1,2]");
/// # }
/// ```
pub async fn to_writer<W, E>(writer: W, encoder: E, value: &dyn Serialize) -> Result<(), Error>
where
    W: AsyncWrite + Unpin,
    E: Encoder,
{
    let mut writer = Writer::new(writer, encoder);
    writer.write(value).await?;
    writer.flush().await
}
