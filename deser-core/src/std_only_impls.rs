//! `Serialize` and `Deserialize` for leaf types that are only in `std`.
//!
//! The ones that are in `core` and `alloc` are in `std_impls`.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;

use crate::adapters::Same;
use crate::de::impls::{Via, deserialize_via, via_handle};
use crate::de::{Deserialize, SinkHandle, Slot, default_atom};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::{Describe, Emit, Serialize};
use crate::{State, Text};

// OnceLock

/// Serializes like an `Option`: as the value if it's set, as null if not.
impl<T: Serialize + Send> Serialize for OnceLock<T> {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        match this.get() {
            Some(value) => T::serialize(value, state),
            None => Ok(Emit::Atom(Atom::Null)),
        }
    }

    fn finish(this: &Self, state: &mut State) -> Result<(), Error> {
        match this.get() {
            Some(value) => T::finish(value, state),
            None => Ok(()),
        }
    }

    fn is_optional(value: &Self) -> bool {
        value.get().is_none()
    }

    fn container_shape(this: &Self) -> crate::ContainerShape {
        match this.get() {
            Some(value) => T::container_shape(value),
            None => crate::ContainerShape::new(),
        }
    }

    fn describe(this: &Self, d: &mut dyn Describe) {
        match this.get() {
            Some(value) => {
                d.some();
                T::describe(value, d);
            }
            None => d.none(),
        }
    }
}

impl<T: Send> Via<Option<T>> for OnceLock<T> {
    #[inline]
    fn convert(value: Option<T>) -> Result<Self, Error> {
        Ok(match value {
            Some(value) => OnceLock::from(value),
            None => OnceLock::new(),
        })
    }
}

/// Deserializes like an `Option`: null and missing values are not set.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for OnceLock<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        via_handle::<Option<T>, Self, Same>(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        T::expecting()
    }

    fn initial_value() -> Option<Self> {
        Some(OnceLock::new())
    }
}

// Paths

/// Serializes as a string.  Paths which are not valid UTF-8 fail to
/// serialize.
impl Serialize for Path {
    begin_without_finish!();

    fn serialize<'a>(this: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        match this.to_str() {
            Some(value) => Ok(Emit::Atom(Atom::Str(Text::borrowed(value)))),
            None => Err(Error::new(
                ErrorKind::InvalidValue,
                "path contains invalid UTF-8 characters",
            )),
        }
    }
}

impl Serialize for PathBuf {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Path::serialize(value.as_path(), state)
    }
}

impl<'de> Deserialize<'de> for PathBuf {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                slot.set(PathBuf::from(value.into_owned()));
                Ok(())
            }
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("path")
    }

    slot_atom_into!();
}

impl Via<PathBuf> for Box<Path> {
    #[inline]
    fn convert(value: PathBuf) -> Result<Self, Error> {
        Ok(value.into_boxed_path())
    }
}

deserialize_via! {
    [] Box<Path> => PathBuf;
}

// OS strings

/// Serializes as a string like [`Path`].  OS strings which are not valid
/// UTF-8 fail to serialize.
impl Serialize for OsStr {
    begin_without_finish!();

    fn serialize<'a>(this: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        match this.to_str() {
            Some(value) => Ok(Emit::Atom(Atom::Str(Text::borrowed(value)))),
            None => Err(Error::new(
                ErrorKind::InvalidValue,
                "OS string contains invalid UTF-8 characters",
            )),
        }
    }
}

impl Serialize for OsString {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        OsStr::serialize(value.as_os_str(), state)
    }
}

impl Via<String> for OsString {
    #[inline]
    fn convert(value: String) -> Result<Self, Error> {
        Ok(OsString::from(value))
    }
}

impl Via<OsString> for Box<OsStr> {
    #[inline]
    fn convert(value: OsString) -> Result<Self, Error> {
        Ok(value.into_boxed_os_str())
    }
}

deserialize_via! {
    [] OsString => String;
    [] Box<OsStr> => OsString;
}

// Locks

// `Mutex` and `RwLock` are only deserialized: serializing would have to hold
// the lock guard until the value is serialized, but serializations can move
// between threads which guards must not.

impl<T: Send> Via<T> for Mutex<T> {
    #[inline]
    fn convert(value: T) -> Result<Self, Error> {
        Ok(Mutex::new(value))
    }
}

impl<T: Send> Via<T> for RwLock<T> {
    #[inline]
    fn convert(value: T) -> Result<Self, Error> {
        Ok(RwLock::new(value))
    }
}

deserialize_via! {
    [T: Deserialize<'de>] Mutex<T> => T;
    [T: Deserialize<'de>] RwLock<T> => T;
}
