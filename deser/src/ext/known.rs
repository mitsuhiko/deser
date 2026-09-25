//! Shared machinery for the well-known extension types and the types that
//! are bridged onto them.
use std::borrow::Cow;

use crate::de::Sink;
use crate::descriptors::Descriptor;
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ext::{ExtValue, Extension};
use crate::State;

/// Implemented by the well-known extension types.
pub(crate) trait WellKnown: Extension + Sized {
    /// What the type expects, for error messages.
    const EXPECTING: &'static str;

    /// The descriptor of the type.
    fn descriptor() -> &'static dyn Descriptor;

    /// Converts an atom into the type.
    ///
    /// Returns `Ok(None)` if the atom is of an unsupported type and an error
    /// if the atom is of a supported type but invalid.
    fn from_atom(atom: &Atom) -> Result<Option<Self>, Error>;
}

/// A type that is serialized and deserialized as a well-known type.
///
/// This is implemented for the types of other crates (`jiff`, `uuid`, ...)
/// and some types of the standard library.
pub(crate) trait Bridge: Sized {
    /// The well-known type.
    type Known: WellKnown;

    /// What the type expects, for error messages.
    const EXPECTING: &'static str = <Self::Known as WellKnown>::EXPECTING;

    /// Converts the value into the well-known type.
    fn to_known(&self) -> Result<Self::Known, Error>;

    /// Converts the well-known type into the value.
    fn from_known(value: Self::Known) -> Result<Self, Error>;

    /// Parses strings that the well-known type does not understand.
    ///
    /// This allows types to accept their own string formats.
    fn parse_fallback(value: &str) -> Option<Self> {
        let _ = value;
        None
    }

    /// Serializes the value.
    fn serialize_atom(&self) -> Result<Atom<'static>, Error> {
        Ok(Atom::Ext(ExtValue::owned(self.to_known()?)))
    }
}

/// Creates a conversion error.
#[cold]
pub(crate) fn invalid(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::new(ErrorKind::Unexpected, msg)
}

/// Creates an out of range error.
#[cold]
pub(crate) fn out_of_range(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::new(ErrorKind::OutOfRange, msg)
}

/// Deserializes a well-known type.
pub(crate) struct KnownSink<'a, T>(pub(crate) &'a mut Option<T>);

impl<'a, T: WellKnown> Sink for KnownSink<'a, T> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        T::descriptor()
    }

    fn expecting(&self) -> Cow<'_, str> {
        T::EXPECTING.into()
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match T::from_atom(&atom)? {
            Some(value) => {
                *self.0 = Some(value);
                Ok(())
            }
            None => self.unexpected_atom(atom, state),
        }
    }
}

/// Deserializes a type bridged onto a well-known type.
pub(crate) struct BridgeSink<'a, T>(pub(crate) &'a mut Option<T>);

impl<'a, T: Bridge> Sink for BridgeSink<'a, T> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        <T::Known as WellKnown>::descriptor()
    }

    fn expecting(&self) -> Cow<'_, str> {
        T::EXPECTING.into()
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let value = match T::Known::from_atom(&atom) {
            Ok(Some(known)) => T::from_known(known)?,
            Ok(None) => return self.unexpected_atom(atom, state),
            Err(err) => match atom {
                Atom::Str(ref s) => T::parse_fallback(s).ok_or(err)?,
                _ => return Err(err),
            },
        };
        *self.0 = Some(value);
        Ok(())
    }
}

/// Implements `Serialize` and `Deserialize` for a well-known type.
macro_rules! impl_well_known {
    ($ty:ty) => {
        impl $crate::ser::Serialize for $ty {
            fn serialize(
                &self,
                _state: &mut $crate::State,
            ) -> Result<$crate::ser::Chunk<'_>, $crate::Error> {
                Ok($crate::ser::Chunk::Atom($crate::Atom::Ext(
                    $crate::ext::ExtValue::borrowed(self),
                )))
            }

            fn descriptor(&self) -> &'static dyn $crate::Descriptor {
                <$ty as $crate::ext::known::WellKnown>::descriptor()
            }
        }

        impl $crate::de::Deserialize for $ty {
            fn deserialize_into(out: &mut Option<Self>) -> $crate::de::SinkHandle<'_> {
                $crate::de::SinkHandle::boxed($crate::ext::known::KnownSink(out))
            }
        }
    };
}

/// Implements `Serialize` and `Deserialize` for a bridged type.
#[allow(unused_macros)]
macro_rules! impl_bridge {
    ($($ty:ty),* $(,)?) => {
        $(
            impl $crate::ser::Serialize for $ty {
                fn serialize(
                    &self,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Chunk<'_>, $crate::Error> {
                    $crate::ext::known::Bridge::serialize_atom(self).map($crate::ser::Chunk::Atom)
                }

                fn descriptor(&self) -> &'static dyn $crate::Descriptor {
                    <<$ty as $crate::ext::known::Bridge>::Known as $crate::ext::known::WellKnown>::descriptor()
                }
            }

            impl $crate::de::Deserialize for $ty {
                fn deserialize_into(out: &mut Option<Self>) -> $crate::de::SinkHandle<'_> {
                    $crate::de::SinkHandle::boxed($crate::ext::known::BridgeSink(out))
                }
            }
        )*
    };
}

pub(crate) use {impl_bridge, impl_well_known};
