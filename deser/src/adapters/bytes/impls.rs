//! The adapters for bytes.
use std::borrow::Cow;
use std::marker::PhantomData;

use crate::State;
use crate::adapters::bytes::{BytesEncoding, BytesFormat};
use crate::adapters::{DeserializeAs, SerializeAs};
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Bytes, ContainerShape};
use crate::ser::{Begin, Chunk};

mod sealed {
    use super::*;

    pub trait BytesBufImpl: Sized {
        fn bytes(&self) -> &[u8];
        fn from_vec(bytes: Vec<u8>) -> Result<Self, Error>;
        fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>) -> SinkHandle<'a, 'de>;
    }

    pub trait BytesFallbackFormatImpl: 'static {
        const FORMAT: BytesFormat;
        fn deserialize_into<'a, 'de, T: BytesBufImpl>(
            out: &'a mut Option<T>,
        ) -> SinkHandle<'a, 'de>;
    }
}

use self::sealed::{BytesBufImpl, BytesFallbackFormatImpl};

/// The types that the bytes adapters support.
///
/// These are `Vec<u8>`, `[u8; N]` and `Cow<[u8]>`.  The trait is
/// sealed, it cannot be implemented outside of deser.
pub trait BytesBuf: BytesBufImpl {}

impl<T: BytesBufImpl> BytesBuf for T {}

impl BytesBufImpl for Vec<u8> {
    #[inline]
    fn bytes(&self) -> &[u8] {
        self
    }

    #[inline]
    fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        Ok(bytes)
    }

    #[inline]
    fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>) -> SinkHandle<'a, 'de> {
        Deserialize::deserialize_into(out)
    }
}

impl<const N: usize> BytesBufImpl for [u8; N] {
    #[inline]
    fn bytes(&self) -> &[u8] {
        self
    }

    fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        bytes
            .try_into()
            .map_err(|_| Error::new(ErrorKind::WrongLength, "byte array of wrong length"))
    }

    #[inline]
    fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>) -> SinkHandle<'a, 'de> {
        Deserialize::deserialize_into(out)
    }
}

impl<'c> BytesBufImpl for Cow<'c, [u8]> {
    #[inline]
    fn bytes(&self) -> &[u8] {
        self
    }

    #[inline]
    fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        Ok(Cow::Owned(bytes))
    }

    #[inline]
    fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>) -> SinkHandle<'a, 'de> {
        Deserialize::deserialize_into(out)
    }
}

/// The representations that [`BytesFallback`] supports.
///
/// These are all [encodings](BytesEncoding) and [`IntSeq`].  The trait is
/// sealed, it cannot be implemented outside of deser.  New encodings are
/// added by implementing [`BytesEncoding`].
pub trait BytesFallbackFormat: BytesFallbackFormatImpl {}

impl<F: BytesFallbackFormatImpl> BytesFallbackFormat for F {}

impl<E: BytesEncoding> BytesFallbackFormatImpl for E {
    const FORMAT: BytesFormat = BytesFormat::encoded::<E>();

    #[inline]
    fn deserialize_into<'a, 'de, T: BytesBufImpl>(out: &'a mut Option<T>) -> SinkHandle<'a, 'de> {
        encoded_handle::<T, E>(out)
    }
}

/// Represents bytes as sequences of integers.
///
/// This is only used with [`BytesFallback`], `BytesFallback<IntSeq>` writes
/// bytes as sequences of integers in formats without native bytes (like
/// `serde_json`).  It corresponds to [`BytesFormat::SEQ`].
pub struct IntSeq;

impl BytesFallbackFormatImpl for IntSeq {
    const FORMAT: BytesFormat = BytesFormat::SEQ;

    #[inline]
    fn deserialize_into<'a, 'de, T: BytesBufImpl>(out: &'a mut Option<T>) -> SinkHandle<'a, 'de> {
        // bytes accept sequences of integers anyways
        T::deserialize_into(out)
    }
}

/// Deserializes bytes which are either native bytes or encoded strings.
struct EncodedSink<'a, T, E> {
    out: &'a mut Option<T>,
    _marker: PhantomData<fn() -> E>,
}

impl<'a, 'de, T: BytesBufImpl, E: BytesEncoding> Sink<'de> for EncodedSink<'a, T, E> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Owned(format!("bytes or {} string", E::NAME))
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let bytes = match atom {
            Atom::Bytes(value) => value.into_data().into_owned(),
            Atom::Str(value) => E::decode(&value)?,
            other => return self.unexpected_atom(other, state),
        };
        *self.out = Some(T::from_vec(bytes)?);
        Ok(())
    }
}

#[inline]
fn encoded_handle<'a, 'de, T: BytesBufImpl, E: BytesEncoding>(
    out: &'a mut Option<T>,
) -> SinkHandle<'a, 'de> {
    SinkHandle::boxed(EncodedSink::<T, E> {
        out,
        _marker: PhantomData,
    })
}

/// Makes the encodings adapters which represent bytes as strings in all
/// formats and accept strings in the encoding and native bytes.
///
/// The impls are for the concrete types rather than all [`BytesBuf`]s as
/// otherwise they would conflict with the impls for `Box<U>`.
macro_rules! encoding_adapter {
    ($([$($gen:tt)*] $ty:ty),* $(,)?) => {
        $(
            impl<$($gen)* E: BytesEncoding> SerializeAs<$ty> for E {
                fn serialize_as<'a>(value: &'a $ty, _state: &mut State) -> Result<Chunk<'a>, Error> {
                    let mut rv = String::new();
                    E::encode(value.bytes(), &mut rv);
                    Ok(Chunk::Atom(Atom::Str(Cow::Owned(rv))))
                }

                #[inline]
                fn __private_begin_as<'a>(value: &'a $ty, state: &mut State) -> Result<Begin<'a>, Error> {
                    Ok(Begin::chunk(
                        Self::serialize_as(value, state)?,
                        ContainerShape::new(),
                        false,
                    ))
                }
            }

            impl<'de, $($gen)* E: BytesEncoding> DeserializeAs<'de, $ty> for E {
                #[inline]
                fn deserialize_into_as<'a>(out: &'a mut Option<$ty>) -> SinkHandle<'a, 'de> {
                    encoded_handle::<$ty, E>(out)
                }
            }
        )*
    };
}

encoding_adapter!(
    [] Vec<u8>,
    [const N: usize,] [u8; N],
    ['c,] Cow<'c, [u8]>,
);

/// Represents bytes as bytes, with a fallback for formats without native bytes.
///
/// The bytes are serialized as bytes which carry the format `F` as
/// fallback (see [`Bytes::fallback`](crate::Bytes::fallback)).  Formats with native bytes (like CBOR)
/// write them as bytes, formats without native bytes (like JSON and TOML)
/// write them in `F`: strings in an [encoding](BytesEncoding) or sequences
/// of integers for [`IntSeq`].  When deserializing native bytes and the
/// representation of `F` are accepted.  It supports the types of
/// [`BytesBuf`].
///
/// ```
/// use deser::adapters::bytes::{BytesFallback, Hex, IntSeq};
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Blob {
///     // hex in JSON and TOML, bytes in CBOR
///     #[deser(as = BytesFallback<Hex>)]
///     sha1: [u8; 20],
///     #[deser(as = Option<BytesFallback<Hex>>)]
///     signature: Option<Vec<u8>>,
///     // `[1, 2]` in JSON and TOML, bytes in CBOR
///     #[deser(as = BytesFallback<IntSeq>)]
///     legacy: Vec<u8>,
/// }
/// ```
///
/// To use an encoding in all formats, use the encoding as adapter (for
/// instance `#[deser(as = Hex)]`).  To change the representation of all
/// bytes, configure the format instead.
pub struct BytesFallback<F>(PhantomData<fn() -> F>);

impl<T: BytesBuf, F: BytesFallbackFormat> SerializeAs<T> for BytesFallback<F> {
    #[inline]
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(
            Bytes::borrowed(value.bytes()).with_fallback(const { &F::FORMAT }),
        )))
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            ContainerShape::new(),
            false,
        ))
    }
}

impl<'de, T: BytesBuf, F: BytesFallbackFormat> DeserializeAs<'de, T> for BytesFallback<F> {
    #[inline]
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        F::deserialize_into(out)
    }
}
