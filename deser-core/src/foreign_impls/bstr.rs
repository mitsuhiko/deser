//! `BString` and `BStr` of `bstr`.
//!
//! Byte strings are conventionally UTF-8.  They are serialized as strings if
//! they are valid UTF-8 and as bytes otherwise.  The bytes carry sequences
//! of integers as fallback (see [`BytesFormat::SEQ`]) so that formats
//! without native bytes (like JSON) can tell them apart from strings.
//!
//! When deserialized, strings are taken as their UTF-8 bytes (like the
//! serde implementation of `bstr`).  Besides strings, byte strings accept
//! bytes and sequences of integers.  `&BStr` borrows strings and bytes from
//! the data.
use std::borrow::Cow;
use std::mem::take;

use ::bstr::{BStr, BString};

use crate::State;
use crate::Text;
use crate::adapters::BytesFormat;
use crate::adapters::bytes::{BytesBufImpl, encoding_adapter};
use crate::de::impls::{Via, deserialize_via};
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Bytes};
use crate::ser::{Chunk, Serialize, plain_atom};

make_slot_wrapper!(SlotWrapper);

/// Returns the atom of a byte string.
#[inline]
fn bstr_atom(bytes: &[u8]) -> Atom<'_> {
    match std::str::from_utf8(bytes) {
        Ok(value) => Atom::Str(Text::borrowed(value)),
        Err(_) => Atom::Bytes(Bytes::borrowed(bytes).with_fallback(const { &BytesFormat::SEQ })),
    }
}

impl Serialize for BStr {
    begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(bstr_atom(self)))
    }
}

impl Serialize for BString {
    begin_without_finish!();
    plain_atom!(|v| bstr_atom(v));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(bstr_atom(self)))
    }
}

/// Converts an atom into a byte string.
///
/// Returns the atom back if it's not a string or bytes.
#[inline]
fn bstring_from_atom(atom: Atom) -> Result<BString, Atom> {
    match atom {
        Atom::Str(value) | Atom::Lexical(value) => Ok(BString::from(value.into_owned())),
        Atom::Char(value) => Ok(BString::from(value.to_string())),
        Atom::Bytes(value) => Ok(BString::from(value.into_owned())),
        other => Err(other),
    }
}

/// Deserializes byte strings from strings, bytes and sequences of integers.
struct BStringSink<'a> {
    out: &'a mut Option<BString>,
    bytes: Vec<u8>,
    element: Option<u8>,
    is_seq: bool,
}

impl BStringSink<'_> {
    #[inline]
    fn flush(&mut self) {
        if let Some(byte) = self.element.take() {
            self.bytes.push(byte);
        }
    }
}

impl<'de> Sink<'de> for BStringSink<'_> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("byte string")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match bstring_from_atom(atom) {
            Ok(value) => {
                *self.out = Some(value);
                Ok(())
            }
            Err(other) => self.unexpected_atom(other, state),
        }
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        self.is_seq = true;
        Ok(())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(u8::deserialize_into(&mut self.element))
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.flush();
        u8::__private_atom_into(&mut self.element, atom, state)
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        if self.is_seq {
            self.flush();
            *self.out = Some(BString::from(take(&mut self.bytes)));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for BString {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(BStringSink {
            out,
            bytes: Vec::new(),
            element: None,
            is_seq: false,
        })
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        match bstring_from_atom(atom) {
            Ok(value) => {
                *out = Some(value);
                Ok(())
            }
            Err(other) => {
                let mut sink = BStringSink {
                    out,
                    bytes: Vec::new(),
                    element: None,
                    is_seq: false,
                };
                sink.atom(other, state)
            }
        }
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

impl Via<BString> for Box<BStr> {
    #[inline]
    fn convert(value: BString) -> Result<Self, Error> {
        Ok(Box::from(Vec::from(value).into_boxed_slice()))
    }
}

deserialize_via! {
    [] Box<BStr> => BString;
}

/// The bytes adapters represent the raw bytes, not the string.
impl BytesBufImpl for BString {
    #[inline]
    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }

    #[inline]
    fn from_vec(bytes: Vec<u8>) -> Result<Self, Error> {
        Ok(BString::from(bytes))
    }

    #[inline]
    fn deserialize_into<'a, 'de>(out: &'a mut Option<Self>) -> SinkHandle<'a, 'de> {
        Deserialize::deserialize_into(out)
    }
}

encoding_adapter!([] BString);

impl<'de: 'a, 'a> Sink<'de> for SlotWrapper<&'a BStr> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("borrowed byte string")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(_) | Atom::Lexical(_) | Atom::Bytes(_) => Err(Error::new(
                ErrorKind::Unexpected,
                "unexpected owned byte string, expected a borrowed byte string (the data \
                 format or the type buffering the value does not support borrowing)",
            )),
            other => self.unexpected_atom(other, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref text) | Atom::Lexical(ref text) if text.is_borrowed() => {
                **self = text.borrowed_str().map(BStr::new);
                Ok(())
            }
            Atom::Implicit(ref value) if value.text().is_borrowed() => {
                **self = value.text().borrowed_str().map(BStr::new);
                Ok(())
            }
            Atom::Bytes(ref value) if value.is_borrowed() => {
                **self = value.borrowed_data().map(BStr::new);
                Ok(())
            }
            other => self.atom(other, state),
        }
    }
}

/// Borrows strings and bytes from the data.
impl<'de: 'a, 'a> Deserialize<'de> for &'a BStr {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SlotWrapper::make_handle(out)
    }
}
