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
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::mem::take;

use ::bstr::{BStr, BString};

use crate::BytesFormat;
use crate::State;
use crate::Text;
use crate::adapters::bytes::{BytesBufImpl, encoding_adapter};
use crate::de::impls::{Via, deserialize_via};
use crate::de::{Deserialize, Sink, SinkHandle, Slot, default_atom};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Bytes};
use crate::ser::{Emit, Serialize, plain_atom};

/// Returns the atom of a byte string.
#[inline]
fn bstr_atom(bytes: &[u8]) -> Atom<'_> {
    match core::str::from_utf8(bytes) {
        Ok(value) => Atom::Str(Text::borrowed(value)),
        Err(_) => {
            let mut bytes = Bytes::borrowed(bytes);
            bytes.fallback = Some(const { &BytesFormat::SEQ });
            Atom::Bytes(bytes)
        }
    }
}

impl Serialize for BStr {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(bstr_atom(value)))
    }
}

impl Serialize for BString {
    begin_without_finish!();
    plain_atom!(|v| bstr_atom(v));

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(bstr_atom(value)))
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

/// What byte strings expect.
const BSTRING_NAME: &str = "byte string";

impl<'de> Sink<'de> for BStringSink<'_> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(BSTRING_NAME)
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match bstring_from_atom(atom) {
            Ok(value) => {
                *self.out = Some(value);
                Ok(())
            }
            Err(other) => default_atom(self, other, state),
        }
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        self.is_seq = true;
        Ok(())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(u8::deserialize_into(&mut self.element, state))
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
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            BStringSink {
                out,
                bytes: Vec::new(),
                element: None,
                is_seq: false,
            },
            state,
        )
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(BSTRING_NAME)
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

encoding_adapter!([] BString);

/// Borrows strings and bytes from the data.
impl<'de: 'a, 'a> Deserialize<'de> for &'a BStr {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(_) | Atom::Lexical(_) | Atom::Bytes(_) => Err(Error::new(
                ErrorKind::UnsupportedType,
                "unexpected owned byte string, expected a borrowed byte string (the data \
                 format or the type buffering the value does not support borrowing)",
            )),
            other => default_atom(slot, other, state),
        }
    }

    fn deserialize_borrowed_atom(
        slot: &mut Slot<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            Atom::Str(ref text) | Atom::Lexical(ref text) if text.is_borrowed() => {
                **slot = text.borrowed_str().map(BStr::new);
                Ok(())
            }
            Atom::Implicit(ref value) if value.text().is_borrowed() => {
                **slot = value.text().borrowed_str().map(BStr::new);
                Ok(())
            }
            Atom::Bytes(ref value) if value.is_borrowed() => {
                **slot = value.borrowed_data().map(BStr::new);
                Ok(())
            }
            other => Self::deserialize_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("borrowed byte string")
    }
}
