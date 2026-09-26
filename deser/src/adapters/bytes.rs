//! Adapters for bytes.
use std::borrow::Cow;
use std::marker::PhantomData;

use crate::adapters::{DeserializeAs, SerializeAs};
use crate::bytes::{
    Base64, Base64NoPad, Base64Url, Base64UrlNoPad, BytesEncoding, BytesFormat, Hex, HexUpper,
};
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::descriptors::Descriptor;
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::{Begin, Chunk};
use crate::State;

mod sealed {
    use super::*;

    /// The types that the bytes adapters support.
    pub trait ByteBuf: Sized {
        fn bytes(&self) -> &[u8];
        fn from_vec(bytes: Vec<u8>) -> Result<Self, Error>;
    }

    impl ByteBuf for Vec<u8> {
        #[inline]
        fn bytes(&self) -> &[u8] {
            self
        }

        #[inline]
        fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
            Ok(bytes)
        }
    }

    impl<const N: usize> ByteBuf for [u8; N] {
        #[inline]
        fn bytes(&self) -> &[u8] {
            self
        }

        fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
            bytes
                .try_into()
                .map_err(|_| Error::new(ErrorKind::WrongLength, "byte array of wrong length"))
        }
    }

    impl<'a> ByteBuf for Cow<'a, [u8]> {
        #[inline]
        fn bytes(&self) -> &[u8] {
            self
        }

        #[inline]
        fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
            Ok(Cow::Owned(bytes))
        }
    }
}

use self::sealed::ByteBuf;

/// The descriptor of bytes that request a format.
struct FormatDescriptor<F>(PhantomData<fn() -> F>);

/// Provides the format of a [`FormatDescriptor`].
trait ProvideFormat: 'static {
    const FORMAT: BytesFormat;
}

impl<E: BytesEncoding> ProvideFormat for E {
    const FORMAT: BytesFormat = BytesFormat::encoded::<E>();
}

/// Marks the sequence format.
struct SeqFormat;

impl ProvideFormat for SeqFormat {
    const FORMAT: BytesFormat = BytesFormat::SEQ;
}

impl<F: ProvideFormat> Descriptor for FormatDescriptor<F> {
    fn name(&self) -> Option<&str> {
        Some("bytes")
    }

    fn bytes_format(&self) -> Option<BytesFormat> {
        Some(F::FORMAT)
    }
}

#[inline(always)]
fn format_descriptor<F: ProvideFormat>() -> &'static dyn Descriptor {
    &FormatDescriptor::<F>(PhantomData)
}

/// Deserializes bytes which are either native bytes or encoded strings.
struct EncodedSink<'a, T, E> {
    out: &'a mut Option<T>,
    _marker: PhantomData<fn() -> E>,
}

impl<'a, 'de, T: ByteBuf, E: BytesEncoding> Sink<'de> for EncodedSink<'a, T, E> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        format_descriptor::<E>()
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Owned(format!("bytes or {} string", E::NAME))
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let bytes = match atom {
            Atom::Bytes(value) => value.into_owned(),
            Atom::Str(value) => E::decode(&value)?,
            other => return self.unexpected_atom(other, state),
        };
        *self.out = Some(T::from_vec(bytes)?);
        Ok(())
    }
}

#[inline]
fn encoded_handle<'a, 'de, T: ByteBuf, E: BytesEncoding>(
    out: &'a mut Option<T>,
) -> SinkHandle<'a, 'de> {
    SinkHandle::boxed(EncodedSink::<T, E> {
        out,
        _marker: PhantomData,
    })
}

/// Represents bytes in an encoding in formats without native bytes.
///
/// The bytes are serialized as bytes which request the encoding `E` (see
/// [`Descriptor::bytes_format`]).  Formats without native bytes (like JSON
/// and TOML) write them as strings in the encoding, formats with native
/// bytes (like CBOR) write them as bytes.  When deserializing bytes and
/// strings in the encoding are accepted.
///
/// The encodings of [`deser::bytes`](crate::bytes) can be used as adapters
/// directly (`#[deser(as = Hex)]` is the same as
/// `#[deser(as = Encoded<Hex>)]`), this adapter is needed for custom
/// encodings.  It supports `Vec<u8>`, `[u8; N]` and `Cow<[u8]>`.
///
/// ```
/// use deser::adapters::Encoded;
/// use deser::bytes::Hex;
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Blob {
///     #[deser(as = Encoded<Hex>)]
///     sha1: [u8; 20],
///     #[deser(as = Option<Encoded<Hex>>)]
///     signature: Option<Vec<u8>>,
/// }
/// ```
///
/// To use the encoding in all formats use [`EncodedStr`].
pub struct Encoded<E>(PhantomData<fn() -> E>);

impl<T: ByteBuf, E: BytesEncoding> SerializeAs<T> for Encoded<E> {
    #[inline]
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(Cow::Borrowed(value.bytes()))))
    }

    #[inline]
    fn descriptor_as(_value: &T) -> &'static dyn Descriptor {
        format_descriptor::<E>()
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            Self::descriptor_as(value),
            false,
        ))
    }
}

impl<'de, T: ByteBuf, E: BytesEncoding> DeserializeAs<'de, T> for Encoded<E> {
    #[inline]
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        encoded_handle::<T, E>(out)
    }
}

/// Represents bytes as strings in an encoding in all formats.
///
/// Unlike [`Encoded`] the bytes are serialized as strings, so formats with
/// native bytes (like CBOR) write strings too.  When deserializing strings
/// in the encoding and bytes are accepted.  It supports `Vec<u8>`,
/// `[u8; N]` and `Cow<[u8]>`.
///
/// ```
/// use deser::adapters::EncodedStr;
/// use deser::bytes::Hex;
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Commit {
///     // a hex string in all formats
///     #[deser(as = EncodedStr<Hex>)]
///     id: [u8; 20],
/// }
/// ```
pub struct EncodedStr<E>(PhantomData<fn() -> E>);

impl<T: ByteBuf, E: BytesEncoding> SerializeAs<T> for EncodedStr<E> {
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        let mut rv = String::new();
        E::encode(value.bytes(), &mut rv);
        Ok(Chunk::Atom(Atom::Str(Cow::Owned(rv))))
    }

    #[inline]
    fn descriptor_as(_value: &T) -> &'static dyn Descriptor {
        format_descriptor::<E>()
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            Self::descriptor_as(value),
            false,
        ))
    }
}

impl<'de, T: ByteBuf, E: BytesEncoding> DeserializeAs<'de, T> for EncodedStr<E> {
    #[inline]
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        encoded_handle::<T, E>(out)
    }
}

/// Represents bytes as sequences of integers in formats without native bytes.
///
/// The bytes are serialized as bytes which request
/// [`BytesFormat::SEQ`] (see [`Descriptor::bytes_format`]).  Formats
/// without native bytes (like JSON and TOML) write them as sequences of
/// integers (like `serde_json`), formats with native bytes (like CBOR)
/// write them as bytes.  Deserialization is not changed, bytes accept
/// sequences of integers anyways.  It supports `Vec<u8>`, `[u8; N]` and
/// `Cow<[u8]>`.
///
/// ```
/// use deser::adapters::ByteSeq;
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Legacy {
///     // `[1, 2]` in JSON
///     #[deser(as = ByteSeq)]
///     data: Vec<u8>,
/// }
/// ```
///
/// To write sequences for all bytes, configure the format instead.
pub struct ByteSeq;

impl<T: ByteBuf> SerializeAs<T> for ByteSeq {
    #[inline]
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(Cow::Borrowed(value.bytes()))))
    }

    #[inline]
    fn descriptor_as(_value: &T) -> &'static dyn Descriptor {
        format_descriptor::<SeqFormat>()
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            Self::descriptor_as(value),
            false,
        ))
    }
}

impl<'de, T: ByteBuf + Deserialize<'de>> DeserializeAs<'de, T> for ByteSeq {
    #[inline]
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        T::deserialize_into(out)
    }
}

/// Makes encodings adapters that work like [`Encoded`].
macro_rules! encoding_adapter {
    ($($ty:ty),* $(,)?) => {
        $(
            impl<T: ByteBuf> SerializeAs<T> for $ty {
                #[inline]
                fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
                    <Encoded<$ty> as SerializeAs<T>>::serialize_as(value, state)
                }

                #[inline]
                fn descriptor_as(value: &T) -> &'static dyn Descriptor {
                    <Encoded<$ty> as SerializeAs<T>>::descriptor_as(value)
                }

                #[inline]
                fn __private_begin_as<'a>(
                    value: &'a T,
                    state: &mut State,
                ) -> Result<Begin<'a>, Error> {
                    <Encoded<$ty> as SerializeAs<T>>::__private_begin_as(value, state)
                }
            }

            impl<'de, T: ByteBuf> DeserializeAs<'de, T> for $ty {
                #[inline]
                fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
                    encoded_handle::<T, $ty>(out)
                }
            }
        )*
    };
}

encoding_adapter!(
    Base64,
    Base64NoPad,
    Base64Url,
    Base64UrlNoPad,
    Hex,
    HexUpper
);

#[cfg(feature = "bytes-encoding")]
encoding_adapter!(
    crate::bytes::Base32,
    crate::bytes::Base32NoPad,
    crate::bytes::Base32Hex,
    crate::bytes::Base32HexNoPad,
);
