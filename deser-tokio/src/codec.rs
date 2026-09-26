use std::marker::PhantomData;

use bytes::BytesMut;
use deser::Error;
use deser::de::DeserializeOwned;
use deser::io::{DecodeBuffer, Decoder, Encoder, Status};
use deser::ser::Serialize;

/// Implements the codec traits of [`tokio-util`](https://docs.rs/tokio-util).
///
/// The codec decodes values of type `T` with a [`Decoder`] and encodes
/// values with an [`Encoder`] (the configurations of a data format).  This
/// makes it usable with `FramedRead`, `FramedWrite` and `Framed`:
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// use futures_util::{SinkExt, StreamExt};
/// use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
/// use deser_tokio::Codec;
/// use tokio_util::codec::Framed;
///
/// const READ_LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
/// const WRITE_LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);
///
/// let (client, server) = tokio::io::duplex(1024);
/// let mut client = Framed::new(client, Codec::<_, _, Vec<u32>>::new(READ_LINES, WRITE_LINES));
/// let mut server = Framed::new(server, Codec::<_, _, Vec<u32>>::new(READ_LINES, WRITE_LINES));
/// client.send(vec![1, 2]).await.unwrap();
/// assert_eq!(server.next().await.unwrap().unwrap(), [1, 2]);
/// # }
/// ```
///
/// The data read by the framed reader is moved into the codec's buffer, so
/// errors refer to positions in the stream.
#[cfg_attr(docsrs, doc(cfg(feature = "codec")))]
pub struct Codec<D: Decoder, E, T> {
    buffer: DecodeBuffer<D>,
    encoder: E,
    written: usize,
    _marker: PhantomData<fn() -> T>,
}

impl<D: Decoder, E: Encoder, T> Codec<D, E, T> {
    /// Creates a codec.
    pub fn new(decoder: D, encoder: E) -> Codec<D, E, T> {
        Codec {
            buffer: DecodeBuffer::new(decoder),
            encoder,
            written: 0,
            _marker: PhantomData,
        }
    }

    /// Returns the decoder.
    pub fn decoder(&self) -> &D {
        self.buffer.decoder()
    }

    /// Returns the encoder.
    pub fn encoder(&self) -> &E {
        &self.encoder
    }

    fn decode_buffered(&mut self, src: &mut BytesMut) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        if !src.is_empty() {
            self.buffer.extend_from_slice(src);
            src.clear();
        }
        match self.buffer.poll()? {
            Status::Ready => self.buffer.deserialize().map(Some),
            Status::NeedInput | Status::End => Ok(None),
        }
    }
}

impl<D: Decoder, E: Encoder, T: DeserializeOwned> tokio_util::codec::Decoder for Codec<D, E, T> {
    type Item = T;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<T>, Error> {
        self.decode_buffered(src)
    }

    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<T>, Error> {
        if !src.is_empty() {
            self.buffer.extend_from_slice(src);
            src.clear();
        }
        if !self.buffer.is_eof() {
            self.buffer.set_eof();
        }
        self.decode_buffered(src)
    }
}

impl<D: Decoder, E: Encoder, T, V: Serialize> tokio_util::codec::Encoder<V> for Codec<D, E, T> {
    type Error = Error;

    fn encode(&mut self, item: V, dst: &mut BytesMut) -> Result<(), Error> {
        let mut out = Vec::new();
        deser::io::encode(&self.encoder, &item, |_| {}, self.written, &mut out)?;
        dst.extend_from_slice(&out);
        self.written += 1;
        Ok(())
    }
}
