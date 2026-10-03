//! The adapters for bytes.
use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::BytesFormat;
use crate::State;
use crate::adapters::bytes::BytesEncoding;
use crate::de::{Deserialize, Sink, SinkHandle, default_atom};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Bytes, ContainerShape};
use crate::ser::{Begin, Emit, Serialize};

mod sealed {
    use super::*;

    pub trait BytesBufImpl: Sized + Send + Sync {
        fn bytes(&self) -> &[u8];
        fn from_vec(bytes: Vec<u8>) -> Result<Self, Error>;
        fn deserialize_into<'a, 'de>(
            out: &'a mut Option<Self>,
            state: &mut State,
        ) -> SinkHandle<'a, 'de>;
        fn expecting() -> Cow<'static, str>;
    }

    pub trait BytesFallbackFormatImpl: Send + Sync + 'static {
        const FORMAT: BytesFormat;
        fn deserialize_into<'a, 'de, T: BytesBufImpl>(
            out: &'a mut Option<T>,
            state: &mut State,
        ) -> SinkHandle<'a, 'de>;
        fn expecting<T: BytesBufImpl>() -> Cow<'static, str>;
    }
}

pub(crate) use self::sealed::BytesBufImpl;
use self::sealed::BytesFallbackFormatImpl;

/// The types that the bytes adapters support.
///
/// These are `Vec<u8>`, `[u8; N]` and `Cow<[u8]>`.  With the features of
/// the same name also `bytes::Bytes`, `bytes::BytesMut`,
/// `bstr::BString`, `smallvec::SmallVec<[u8; N]>` and
/// `arrayvec::ArrayVec<u8, N>`.  The trait is sealed, it cannot be
/// implemented outside of deser.
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
    fn deserialize_into<'a, 'de>(
        out: &'a mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        <Self as Deserialize<'de>>::deserialize_into(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        <Self as Deserialize<'static>>::expecting()
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
    fn deserialize_into<'a, 'de>(
        out: &'a mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        <Self as Deserialize<'de>>::deserialize_into(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        <Self as Deserialize<'static>>::expecting()
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
    fn deserialize_into<'a, 'de>(
        out: &'a mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        <Self as Deserialize<'de>>::deserialize_into(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        <Self as Deserialize<'static>>::expecting()
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
    fn deserialize_into<'a, 'de, T: BytesBufImpl>(
        out: &'a mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        encoded_handle::<T, E>(out, state)
    }

    fn expecting<T: BytesBufImpl>() -> Cow<'static, str> {
        encoded_expecting::<E>()
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
    fn deserialize_into<'a, 'de, T: BytesBufImpl>(
        out: &'a mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        // bytes accept sequences of integers anyways
        T::deserialize_into(out, state)
    }

    fn expecting<T: BytesBufImpl>() -> Cow<'static, str> {
        T::expecting()
    }
}

/// What bytes with an encoding expect.
pub(crate) fn encoded_expecting<E: BytesEncoding>() -> Cow<'static, str> {
    Cow::Owned(format!("bytes or {} string", E::NAME))
}

/// Deserializes bytes which are either native bytes or encoded strings.
struct EncodedSink<'a, T, E> {
    out: &'a mut Option<T>,
    _marker: PhantomData<fn() -> E>,
}

impl<'a, 'de, T: BytesBufImpl, E: BytesEncoding> Sink<'de> for EncodedSink<'a, T, E> {
    fn expecting(&self) -> Cow<'_, str> {
        encoded_expecting::<E>()
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let bytes = match atom {
            Atom::Bytes(value) => value.into_data().into_owned(),
            Atom::Str(value) => E::decode(&value)?,
            other => return default_atom(self, other, state),
        };
        *self.out = Some(T::from_vec(bytes)?);
        Ok(())
    }
}

#[inline]
pub(crate) fn encoded_handle<'a, 'de, T: BytesBufImpl, E: BytesEncoding>(
    out: &'a mut Option<T>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    SinkHandle::arena(
        EncodedSink::<T, E> {
            out,
            _marker: PhantomData,
        },
        state,
    )
}

/// Makes the encodings adapters which represent bytes as strings in all
/// formats and accept strings in the encoding and native bytes.
///
/// The impls are for the concrete types rather than all [`BytesBuf`]s as
/// otherwise they would conflict with the impls for `Box<U>`.
macro_rules! encoding_adapter {
    ($([$($gen:tt)*] $ty:ty),* $(,)?) => {
        $(
            impl<$($gen)* E: $crate::adapters::BytesEncoding> $crate::ser::Serialize<$ty> for E {
                fn serialize<'a>(
                    value: &'a $ty,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Emit<'a>, $crate::Error> {
                    let mut rv = alloc::string::String::new();
                    E::encode(
                        $crate::adapters::bytes::BytesBufImpl::bytes(value),
                        &mut rv,
                    );
                    Ok($crate::ser::Emit::Atom($crate::Atom::Str($crate::Text::owned(rv))))
                }

                // rustdoc shows the hidden method in blanket impls
                #[doc(hidden)]
                #[inline]
                fn __private_begin<'a>(
                    value: &'a $ty,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'a>, $crate::Error> {
                    Ok($crate::ser::Begin::emit(
                        Self::serialize(value, state)?,
                        $crate::ContainerShape::new(),
                        false,
                    ))
                }
            }

            impl<'de, $($gen)* E: $crate::adapters::BytesEncoding>
                $crate::de::Deserialize<'de, $ty> for E
            {
                #[inline]
                fn deserialize_into<'a>(
                    out: &'a mut Option<$ty>, state: &mut $crate::State) -> $crate::de::SinkHandle<'a, 'de> {
                    $crate::adapters::bytes::encoded_handle::<$ty, E>(out, state)
                }

                fn expecting() -> alloc::borrow::Cow<'static, str> {
                    $crate::adapters::bytes::encoded_expecting::<E>()
                }
            }
        )*
    };
}

// also used for the byte buffers of other crates
#[allow(unused_imports)]
pub(crate) use encoding_adapter;

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
/// use deser::adapters::{Base64Url, BytesFallback, IntSeq};
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Blob {
///     // URL-safe base64 in JSON and TOML, bytes in CBOR
///     #[deser(as = BytesFallback<Base64Url>)]
///     sha1: [u8; 20],
///     #[deser(as = Option<BytesFallback<Base64Url>>)]
///     signature: Option<Vec<u8>>,
///     // `[1, 2]` in JSON and TOML, bytes in CBOR
///     #[deser(as = BytesFallback<IntSeq>)]
///     legacy: Vec<u8>,
/// }
/// ```
///
/// To use an encoding in all formats, use the encoding as adapter (for
/// instance `#[deser(as = Base64Url)]`).  To change the representation of all
/// bytes, configure the format instead.
pub struct BytesFallback<F>(PhantomData<fn() -> F>);

impl<T: BytesBuf, F: BytesFallbackFormat> Serialize<T> for BytesFallback<F> {
    #[inline]
    fn serialize<'a>(value: &'a T, _state: &mut State) -> Result<Emit<'a>, Error> {
        let mut bytes = Bytes::borrowed(value.bytes());
        bytes.fallback = Some(const { &F::FORMAT });
        Ok(Emit::Atom(Atom::Bytes(bytes)))
    }

    #[inline]
    fn __private_begin<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::emit(
            Self::serialize(value, state)?,
            ContainerShape::new(),
            false,
        ))
    }
}

impl<'de, T: BytesBuf, F: BytesFallbackFormat> Deserialize<'de, T> for BytesFallback<F> {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        F::deserialize_into(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        F::expecting::<T>()
    }
}
