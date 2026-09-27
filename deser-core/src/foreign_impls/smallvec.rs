//! `SmallVec` of `smallvec`.
//!
//! It's serialized like `Vec`: `SmallVec<[u8; N]>` as bytes, everything
//! else as sequence.  The impls are for arrays as backing store
//! (`SmallVec<[T; N]>`) which requires the `const_generics` feature of
//! `smallvec`.
use ::smallvec::SmallVec;
use alloc::vec::Vec;

use crate::adapters::bytes::{BytesBufImpl, encoding_adapter};
use crate::adapters::ser_impls::serialize_as_slice;
use crate::adapters::{DeserializeAs, Same, SerializeAs};
use crate::de::impls::{SeqTarget, seq_sink};
use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::ser::Serialize;
use crate::ser::impls::serialize_slice;

serialize_slice!(
    [T: Serialize, const N: usize] SmallVec<[T; N]>,
);

serialize_as_slice! {
    [T: Sync, A: SerializeAs<T>, const N: usize] SmallVec<[T; N]> => SmallVec<[A; N]>;
}

impl<T: Send, const N: usize> SeqTarget<T> for SmallVec<[T; N]> {
    const NAME: &'static str = "SmallVec";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(SmallVec::from_vec(vec))
    }
}

impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for SmallVec<[T; N]> {
    #[inline]
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        seq_sink::<Self, T, Same>(out)
    }
}

impl<'de, T: Send, A: DeserializeAs<'de, T>, const N: usize> DeserializeAs<'de, SmallVec<[T; N]>>
    for SmallVec<[A; N]>
{
    fn deserialize_into_as(out: &mut Option<SmallVec<[T; N]>>) -> SinkHandle<'_, 'de> {
        seq_sink::<SmallVec<[T; N]>, T, A>(out)
    }
}

impl<const N: usize> BytesBufImpl for SmallVec<[u8; N]> {
    #[inline]
    fn bytes(&self) -> &[u8] {
        self
    }

    #[inline]
    fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        Ok(SmallVec::from_vec(bytes))
    }

    #[inline]
    fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>) -> SinkHandle<'a, 'de> {
        Deserialize::deserialize_into(out)
    }
}

encoding_adapter!(
    [const N: usize,] SmallVec<[u8; N]>,
);
