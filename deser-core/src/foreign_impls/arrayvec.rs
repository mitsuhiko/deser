//! `ArrayVec` and `ArrayString` of `arrayvec`.
//!
//! `ArrayVec` is serialized like `Vec`: `ArrayVec<u8, CAP>` as bytes,
//! everything else as sequence.  `ArrayString` is serialized like `String`.
//! Deserializing more elements or a longer string than the capacity is an
//! error.
use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;

use ::arrayvec::{ArrayString, ArrayVec};

use crate::State;
use crate::Text;
use crate::adapters::bytes::{BytesBufImpl, encoding_adapter};
use crate::adapters::ser_impls::serialize_as_slice;
use crate::adapters::{DeserializeAs, Same, SerializeAs};
use crate::de::impls::{SeqTarget, collection_methods, collection_methods_as, seq_sink};
use crate::de::update::Collection;
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::impls::serialize_slice;
use crate::ser::{Chunk, Serialize, plain_atom};

make_slot_wrapper!(SlotWrapper);

serialize_slice!(
    [T: Serialize, const CAP: usize] ArrayVec<T, CAP>,
);

serialize_as_slice! {
    [T: Sync, A: SerializeAs<T>, const CAP: usize] ArrayVec<T, CAP> => ArrayVec<A, CAP>;
}

/// Creates the error for values that exceed the capacity.
#[cold]
fn capacity_exceeded(what: &str, capacity: usize) -> Error {
    Error::new(
        ErrorKind::WrongLength,
        format!("{what} exceeds the capacity of {capacity}"),
    )
}

/// Converts a vector into an `ArrayVec`.
fn array_vec_from_vec<T, const CAP: usize>(vec: Vec<T>) -> Result<ArrayVec<T, CAP>, Error> {
    if vec.len() > CAP {
        return Err(capacity_exceeded("ArrayVec", CAP));
    }
    let mut rv = ArrayVec::new();
    rv.extend(vec);
    Ok(rv)
}

impl<T: Send, const CAP: usize> SeqTarget<T> for ArrayVec<T, CAP> {
    const NAME: &'static str = "ArrayVec";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        array_vec_from_vec(vec)
    }
}

impl<'de, T: Deserialize<'de>, const CAP: usize> Deserialize<'de> for ArrayVec<T, CAP> {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        seq_sink::<Self, T, Same>(out, state)
    }

    collection_methods!(Same);
}

impl<'de, T: Send, A: DeserializeAs<'de, T>, const CAP: usize> DeserializeAs<'de, ArrayVec<T, CAP>>
    for ArrayVec<A, CAP>
{
    fn deserialize_into_as<'out>(
        out: &'out mut Option<ArrayVec<T, CAP>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        seq_sink::<ArrayVec<T, CAP>, T, A>(out, state)
    }

    collection_methods_as!(ArrayVec<T, CAP>);
}

impl<T: Send, const CAP: usize> Collection<T> for ArrayVec<T, CAP> {
    fn empty() -> Self {
        ArrayVec::new()
    }

    fn add(&mut self, value: T) -> Result<(), Error> {
        self.try_push(value)
            .map_err(|_| capacity_exceeded("ArrayVec", CAP))
    }
}

impl<const CAP: usize> BytesBufImpl for ArrayVec<u8, CAP> {
    #[inline]
    fn bytes(&self) -> &[u8] {
        self
    }

    #[inline]
    fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        array_vec_from_vec(bytes)
    }

    #[inline]
    fn deserialize_into<'a, 'de>(
        out: &'a mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        Deserialize::deserialize_into(out, state)
    }
}

encoding_adapter!(
    [const CAP: usize,] ArrayVec<u8, CAP>,
);

impl<const CAP: usize> Serialize for ArrayString<CAP> {
    begin_without_finish!();
    plain_atom!(|v| Atom::Str(Text::borrowed(v.as_str())));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str(Text::borrowed(self.as_str()))))
    }
}

impl<'de, const CAP: usize> Sink<'de> for SlotWrapper<ArrayString<CAP>> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("string")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let mut rv = ArrayString::new();
        match atom {
            Atom::Str(ref value) | Atom::Lexical(ref value) => rv
                .try_push_str(value)
                .map_err(|_| capacity_exceeded("string", CAP))?,
            Atom::Char(value) => rv
                .try_push(value)
                .map_err(|_| capacity_exceeded("string", CAP))?,
            other => return self.unexpected_atom(other, state),
        }
        **self = Some(rv);
        Ok(())
    }
}

impl<'de, const CAP: usize> Deserialize<'de> for ArrayString<CAP> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SlotWrapper::make_handle(out)
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let sink = SlotWrapper::wrap(out);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        // the sink does not borrow
        Self::__private_atom_into(out, atom, state)
    }
}
