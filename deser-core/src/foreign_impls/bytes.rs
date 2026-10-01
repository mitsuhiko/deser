//! `Bytes` and `BytesMut` of `bytes`.
//!
//! They are serialized like `Vec<u8>`: as bytes, which formats without
//! native bytes represent as strings (base64 by default).  When
//! deserialized, they accept what `Vec<u8>` accepts.
use ::bytes::{Bytes, BytesMut};
use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::State;
use crate::adapters::bytes::{BytesBufImpl, encoding_adapter};
use crate::de::impls::{Via, deserialize_via};
use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::event::Atom;
use crate::ser::{Emit, Serialize, plain_atom};

macro_rules! byte_buffer {
    ($($ty:ty => $convert:expr;)*) => {
        $(
            impl Serialize for $ty {
                begin_without_finish!();
                plain_atom!(|v| Atom::Bytes(crate::Bytes::new(&v[..])));

                fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
                    Ok(Emit::Atom(Atom::Bytes(crate::Bytes::new(&value[..]))))
                }
            }

            impl Via<Vec<u8>> for $ty {
                #[inline]
                fn convert(value: Vec<u8>) -> Result<Self, Error> {
                    Ok($convert(value))
                }
            }

            deserialize_via! {
                [] $ty => Vec<u8>;
            }

            impl BytesBufImpl for $ty {
                #[inline]
                fn bytes(&self) -> &[u8] {
                    self
                }

                #[inline]
                fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
                    Ok($convert(bytes))
                }

                #[inline]
                fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>, state: &mut State) -> SinkHandle<'a, 'de> {
                    <Self as Deserialize<'de>>::deserialize_into(out, state)
                }

                fn expecting() -> Cow<'static, str> {
                    <Self as Deserialize<'static>>::expecting()
                }
            }

            encoding_adapter!([] $ty);
        )*
    };
}

byte_buffer! {
    Bytes => Bytes::from;
    // converting from `Bytes` does not copy as the buffer is unique
    BytesMut => |value| BytesMut::from(Bytes::from(value));
}
