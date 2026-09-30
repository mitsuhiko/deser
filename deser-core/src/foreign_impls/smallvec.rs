//! `SmallVec` of `smallvec`.
//!
//! It's serialized like `Vec`: `SmallVec<[u8; N]>` as bytes, everything
//! else as sequence.  The impls are for arrays as backing store
//! (`SmallVec<[T; N]>`) which requires the `const_generics` feature of
//! `smallvec`.
use ::smallvec::SmallVec;
use alloc::vec::Vec;

use crate::State;
use crate::adapters::bytes::{BytesBufImpl, encoding_adapter};
use crate::de::impls::{SeqTarget, collection_methods, seq_sink};
use crate::de::update::Collection;
use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::ser::Serialize;
use crate::ser::impls::serialize_slice;

serialize_slice! {
    [T: Sync, A: Serialize<T>, const N: usize] SmallVec<[T; N]> => SmallVec<[A; N]>, A;
}

impl<T: Send, const N: usize> SeqTarget<T> for SmallVec<[T; N]> {
    const NAME: &'static str = "SmallVec";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(SmallVec::from_vec(vec))
    }
}

impl<'de, T: Send, A: Deserialize<'de, T>, const N: usize> Deserialize<'de, SmallVec<[T; N]>>
    for SmallVec<[A; N]>
{
    fn deserialize_into<'out>(
        out: &'out mut Option<SmallVec<[T; N]>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        seq_sink::<SmallVec<[T; N]>, T, A>(out, state)
    }

    collection_methods!(SmallVec<[T; N]>);
}

impl<T: Send, const N: usize> Collection<T> for SmallVec<[T; N]> {
    fn empty() -> Self {
        SmallVec::new()
    }

    fn add(&mut self, value: T) -> Result<(), Error> {
        self.push(value);
        Ok(())
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
    fn deserialize_into<'a, 'de>(
        out: &'a mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        <Self as Deserialize<'de>>::deserialize_into(out, state)
    }
}

encoding_adapter!(
    [const N: usize,] SmallVec<[u8; N]>,
);
