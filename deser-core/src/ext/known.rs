//! Shared machinery for the well-known extension types and the types that
//! are bridged onto them.
use alloc::borrow::Cow;

use crate::State;
use crate::de::{Deserialize, Slot, default_atom};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ext::{ExtValue, Extension};

/// Implemented by the well-known extension types.
pub(crate) trait WellKnown: Extension + Sized {
    /// What the type expects, for error messages.
    const EXPECTING: &'static str;

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
pub(crate) trait Bridge: Sized + Send {
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

// `f64::trunc`, `f64::floor` and `f64::round` are not in `core`.  These
// are exact for finite values that fit into an `i64`, which is all the
// conversions of float seconds need.

/// Rounds towards zero.
pub(crate) fn trunc(value: f64) -> f64 {
    debug_assert!(value.is_finite() && value.abs() < 9.2e18);
    value as i64 as f64
}

/// Rounds towards negative infinity.
pub(crate) fn floor(value: f64) -> f64 {
    let rv = trunc(value);
    if rv > value { rv - 1.0 } else { rv }
}

/// Rounds to the nearest integer, half way cases away from zero.
pub(crate) fn round(value: f64) -> f64 {
    let rv = trunc(value);
    // exact, the difference is the fraction of the value
    let fraction = value - rv;
    if fraction >= 0.5 {
        rv + 1.0
    } else if fraction <= -0.5 {
        rv - 1.0
    } else {
        rv
    }
}

#[test]
fn test_rounding() {
    let mut values = vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.5,
        -1.5,
        2.5,
        -2.5,
        0.49999999999999994,
        -0.49999999999999994,
        1.0 - f64::EPSILON / 2.0,
        4503599627370495.5,
        -4503599627370495.5,
        4503599627370497.0,
        9.1e18,
        -9.1e18,
        1e-300,
        -1e-300,
        123.456,
        -123.456,
        999999999.5,
        0.9999999995,
    ];
    let mut x = 0.1f64;
    while x < 1e18 {
        values.extend([x, -x, x + 0.5, -x - 0.5]);
        x *= 1.7;
    }
    for value in values {
        assert_eq!(trunc(value), value.trunc(), "trunc {value}");
        assert_eq!(floor(value), value.floor(), "floor {value}");
        assert_eq!(round(value), value.round(), "round {value}");
    }
}

/// Deserializes an atom into a well-known type.
pub(crate) fn known_atom<'de, T: WellKnown + Deserialize<'de>>(
    slot: &mut Slot<T>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    match T::from_atom(&atom)? {
        Some(value) => {
            slot.set(value);
            Ok(())
        }
        None => default_atom(slot, atom, state),
    }
}

/// Deserializes an atom into a type bridged onto a well-known type.
pub(crate) fn bridge_atom<'de, T: Bridge + Deserialize<'de>>(
    slot: &mut Slot<T>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    let value = match T::Known::from_atom(&atom) {
        Ok(Some(known)) => T::from_known(known)?,
        Ok(None) => return default_atom(slot, atom, state),
        Err(err) => match atom {
            Atom::Str(ref s) => T::parse_fallback(s).ok_or(err)?,
            _ => return Err(err),
        },
    };
    slot.set(value);
    Ok(())
}

/// Implements `Serialize` and `Deserialize` for a well-known type.
macro_rules! impl_well_known {
    ($ty:ty) => {
        impl $crate::ser::Serialize for $ty {
            fn serialize<'a>(
                value: &'a Self,
                _state: &mut $crate::State,
            ) -> Result<$crate::ser::Emit<'a>, $crate::Error> {
                Ok($crate::ser::Emit::Atom($crate::Atom::Ext(
                    $crate::ext::ExtValue::borrowed(value),
                )))
            }
        }

        impl<'de> $crate::de::Deserialize<'de> for $ty {
            fn deserialize_atom(
                slot: &mut $crate::de::Slot<Self>,
                atom: $crate::Atom,
                state: &mut $crate::State,
            ) -> Result<(), $crate::Error> {
                $crate::ext::known::known_atom(slot, atom, state)
            }

            fn expecting() -> alloc::borrow::Cow<'static, str> {
                alloc::borrow::Cow::Borrowed(<$ty as $crate::ext::known::WellKnown>::EXPECTING)
            }
        }
    };
}

/// Implements `Serialize` and `Deserialize` for a bridged type.
macro_rules! impl_bridge {
    ($($ty:ty),* $(,)?) => {
        $(
            impl $crate::ser::Serialize for $ty {
                fn serialize<'a>(
                    value: &'a Self,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Emit<'a>, $crate::Error> {
                    $crate::ext::known::Bridge::serialize_atom(value).map($crate::ser::Emit::Atom)
                }

            }

            impl<'de> $crate::de::Deserialize<'de> for $ty {
                fn deserialize_atom(
                    slot: &mut $crate::de::Slot<Self>,
                    atom: $crate::Atom,
                    state: &mut $crate::State,
                ) -> Result<(), $crate::Error> {
                    $crate::ext::known::bridge_atom(slot, atom, state)
                }

                fn expecting() -> alloc::borrow::Cow<'static, str> {
                    alloc::borrow::Cow::Borrowed(
                        <$ty as $crate::ext::known::Bridge>::EXPECTING,
                    )
                }
            }
        )*
    };
}

pub(crate) use {impl_bridge, impl_well_known};
