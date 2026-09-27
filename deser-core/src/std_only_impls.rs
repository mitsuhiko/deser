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
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::{Chunk, Describe, Serialize};
use crate::{State, Text};

make_slot_wrapper!(SlotWrapper);

// OnceLock

/// Serializes like an `Option`: as the value if it's set, as null if not.
impl<T: Serialize + Send> Serialize for OnceLock<T> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.get() {
            Some(value) => value.serialize(state),
            None => Ok(Chunk::Atom(Atom::Null)),
        }
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        match self.get() {
            Some(value) => value.finish(state),
            None => Ok(()),
        }
    }

    fn is_optional(&self) -> bool {
        self.get().is_none()
    }

    fn container_shape(&self) -> crate::ContainerShape {
        match self.get() {
            Some(value) => value.container_shape(),
            None => crate::ContainerShape::new(),
        }
    }

    fn describe(&self, d: &mut dyn Describe) {
        match self.get() {
            Some(value) => {
                d.some();
                value.describe(d);
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        via_handle::<Option<T>, Self, Same>(out)
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

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.to_str() {
            Some(value) => Ok(Chunk::Atom(Atom::Str(Text::borrowed(value)))),
            None => Err(Error::new(
                ErrorKind::Unexpected,
                "path contains invalid UTF-8 characters",
            )),
        }
    }
}

impl Serialize for PathBuf {
    begin_without_finish!();

    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.as_path().serialize(state)
    }
}

impl<'de> Sink<'de> for SlotWrapper<PathBuf> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("path")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                **self = Some(PathBuf::from(value.into_owned()));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

impl<'de> Deserialize<'de> for PathBuf {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
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
        Self::__private_atom_into(out, atom, state)
    }
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

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.to_str() {
            Some(value) => Ok(Chunk::Atom(Atom::Str(Text::borrowed(value)))),
            None => Err(Error::new(
                ErrorKind::Unexpected,
                "OS string contains invalid UTF-8 characters",
            )),
        }
    }
}

impl Serialize for OsString {
    begin_without_finish!();

    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.as_os_str().serialize(state)
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
