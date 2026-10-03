use std::marker::PhantomData;

use bytes::BytesMut;
use deser_core::Error;
use deser_core::de::StreamDeserializer;
use deser_core::de::{DeserializeOwned, OwnedDriver};
use deser_core::ser::{Serialize, StreamSerializer};
use deser_core::stream::{InputBuffer, Status};

/// Implements the codec traits of [`tokio-util`](https://docs.rs/tokio-util).
///
/// The codec decodes values of type `T` with a [`StreamDeserializer`] and
/// encodes values with a [`StreamSerializer`] (for instance
/// `deser_json::StreamDeserializer` and `deser_json::Serializer`).  This
/// makes it usable with `FramedRead`, `FramedWrite` and `Framed`:
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// use futures_util::{SinkExt, StreamExt};
/// use deser_json::{DeserializerConfig, Serializer, SerializerConfig, StreamDeserializer, Trailing};
/// use deser_tokio::Codec;
/// use tokio_util::codec::Framed;
///
/// const READ_LINES: DeserializerConfig =
///     DeserializerConfig::builder().trailing(Trailing::Newline).build();
/// const WRITE_LINES: SerializerConfig =
///     SerializerConfig::builder().trailing(Trailing::Newline).build();
///
/// let (client, server) = tokio::io::duplex(1024);
/// let codec = || {
///     Codec::<_, _, Vec<u32>>::new(
///         StreamDeserializer::with_config(&READ_LINES),
///         Serializer::with_config(&WRITE_LINES),
///     )
/// };
/// let mut client = Framed::new(client, codec());
/// let mut server = Framed::new(server, codec());
/// client.send(vec![1, 2]).await.unwrap();
/// assert_eq!(server.next().await.unwrap().unwrap(), [1, 2]);
/// # }
/// ```
///
/// The data read by the framed reader is moved into the codec's buffer, so
/// errors refer to positions in the stream.  If the format supports it
/// (see [`StreamDeserializer::supports_partial`]), values are deserialized
/// while their input arrives.
#[cfg_attr(docsrs, doc(cfg(feature = "codec")))]
pub struct Codec<D: StreamDeserializer, S: StreamSerializer, T> {
    buffer: InputBuffer<D>,
    serializer: S,
    // the value which is deserialized while its input arrives
    pending: Option<OwnedDriver<'static, T>>,
    _marker: PhantomData<fn() -> T>,
}

impl<D: StreamDeserializer, S: StreamSerializer, T> Codec<D, S, T> {
    /// Creates a codec.
    pub fn new(deserializer: D, serializer: S) -> Codec<D, S, T> {
        Codec {
            buffer: InputBuffer::new(deserializer),
            serializer,
            pending: None,
            _marker: PhantomData,
        }
    }

    /// Returns the stream deserializer.
    pub fn deserializer(&self) -> &D {
        self.buffer.deserializer()
    }

    /// Returns the stream serializer.
    pub fn serializer(&self) -> &S {
        &self.serializer
    }

    /// Sets the context the values are deserialized and serialized in.
    ///
    /// This replaces the context of the stream deserializer (see
    /// [`StreamDeserializer::context`](deser_core::de::StreamDeserializer::context)).
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)).
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.buffer.set_context(context);
    }

    /// Returns the context the values are deserialized and serialized in.
    pub fn context(&self) -> &deser_core::Context {
        self.buffer.context()
    }

    fn decode_buffered(&mut self, src: &mut BytesMut) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        if !src.is_empty() {
            self.buffer.extend_from_slice(src);
            src.clear();
        }
        if !self.buffer.supports_partial() {
            return match self.buffer.poll()? {
                Status::Ready => self.buffer.deserialize().map(Some),
                Status::NeedInput | Status::End => Ok(None),
            };
        }
        let mut driver = self.pending.take().unwrap_or_default();
        match driver.with(|driver| self.buffer.drive_partial(driver))? {
            Status::Ready => driver.finish().map(Some),
            Status::End => Ok(None),
            Status::NeedInput => {
                self.pending = Some(driver);
                Ok(None)
            }
        }
    }
}

impl<D: StreamDeserializer, S: StreamSerializer, T: DeserializeOwned> tokio_util::codec::Decoder
    for Codec<D, S, T>
{
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

impl<D: StreamDeserializer, S: StreamSerializer, T, V: Serialize> tokio_util::codec::Encoder<V>
    for Codec<D, S, T>
{
    type Error = Error;

    fn encode(&mut self, item: V, dst: &mut BytesMut) -> Result<(), Error> {
        // the value is written at once, the output is moved to the
        // destination
        let mut driver = deser_core::ser::SerializeDriver::new(&item);
        driver
            .state_mut()
            .set_default_context(self.buffer.context().clone());
        self.serializer.drive(&mut driver)?;
        dst.extend_from_slice(self.serializer.output());
        self.serializer.clear_output();
        Ok(())
    }
}
