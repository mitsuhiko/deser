//! Extensions to the data model.
//!
//! The core data model of deser is intentionally small (see [`Atom`]).  To
//! support values that do not map naturally onto it, the data model can be
//! extended with arbitrary types through [`Atom::Ext`].  An extension value
//! is a typed Rust value implementing [`Extension`] which is passed through
//! the system as is.
//!
//! Every extension value has to provide a [`fallback`](Extension::fallback)
//! which lowers the value into the core data model.  This means that producers
//! do not need to know if a consumer understands an extension:
//!
//! * a serializer which knows about an extension type can
//!   [downcast](ExtValue::downcast_ref) the value and handle it natively.
//!   Otherwise it serializes the fallback.
//! * a [`Sink`](crate::de::Sink) which knows about an extension type can
//!   downcast it.  Otherwise the default implementation of
//!   [`Sink::unexpected_atom`](crate::de::Sink::unexpected_atom) retries
//!   with the fallback.
//!
//! This avoids in-band signalling: the value keeps its identity for everybody
//! who understands it, and degrades gracefully for everybody else.
//!
//! Deser itself uses this for `u128` and `i128`.  For a more complete
//! example which annotates every value with its path and shows how that
//! information survives internal buffering, see the
//! [`located` example](https://github.com/mitsuhiko/deser/tree/main/examples/located).
//!
//! # Example
//!
//! ```
//! use deser::ext::{Extension, ExtValue};
//! use deser::ser::{Chunk, Serialize};
//! use deser::State;
//! use deser::{Atom, Error};
//!
//! /// A timestamp in seconds, falls back to an integer.
//! #[derive(Debug, Clone, PartialEq)]
//! pub struct Timestamp(pub i64);
//!
//! impl Extension for Timestamp {
//!     fn name(&self) -> &str {
//!         "timestamp"
//!     }
//!
//!     fn fallback(&self) -> Atom<'_> {
//!         Atom::I64(self.0)
//!     }
//! }
//!
//! impl Serialize for Timestamp {
//!     fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
//!         Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
//!     }
//! }
//! ```
use std::any::Any;
use std::fmt;

use crate::event::Atom;

/// A type that can be passed through deser as an extension to the data model.
///
/// See the [module level documentation](self) for more information.
pub trait Extension: Any + fmt::Debug + Clone + PartialEq + Send + Sync {
    /// Returns the human readable name of the extension type.
    ///
    /// This is used for error messages.
    fn name(&self) -> &str;

    /// Lowers the value into the core data model.
    ///
    /// This is used by consumers that do not understand this extension.  The
    /// fallback must not be an [`Atom::Ext`] itself.
    fn fallback(&self) -> Atom<'_>;
}

/// The object safe version of [`Extension`].
trait DynExtension: fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn fallback(&self) -> Atom<'_>;
    fn clone_box(&self) -> Box<dyn DynExtension>;
    fn dyn_eq(&self, other: &dyn DynExtension) -> bool;
    fn as_any(&self) -> &dyn Any;
}

impl<T: Extension> DynExtension for T {
    fn name(&self) -> &str {
        Extension::name(self)
    }

    fn fallback(&self) -> Atom<'_> {
        Extension::fallback(self)
    }

    fn clone_box(&self) -> Box<dyn DynExtension> {
        Box::new(self.clone())
    }

    fn dyn_eq(&self, other: &dyn DynExtension) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An extension value carried by [`Atom::Ext`].
///
/// The value is either borrowed (which is typical during serialization) or
/// owned (which is typical for deserialization where a data format creates
/// the value).
pub struct ExtValue<'a>(Repr<'a>);

enum Repr<'a> {
    Borrowed(&'a dyn DynExtension),
    Owned(Box<dyn DynExtension>),
}

impl<'a> ExtValue<'a> {
    /// Creates an extension value borrowing from a value.
    pub fn borrowed<T: Extension>(value: &'a T) -> ExtValue<'a> {
        ExtValue(Repr::Borrowed(value))
    }

    /// Creates an extension value that owns the value.
    pub fn owned<T: Extension>(value: T) -> ExtValue<'static> {
        ExtValue(Repr::Owned(Box::new(value)))
    }

    fn get(&self) -> &dyn DynExtension {
        match self.0 {
            Repr::Borrowed(value) => value,
            Repr::Owned(ref value) => &**value,
        }
    }

    /// Returns the human readable name of the extension type.
    pub fn name(&self) -> &str {
        self.get().name()
    }

    /// Returns the fallback atom of the value.
    ///
    /// See [`Extension::fallback`].
    pub fn fallback(&self) -> Atom<'_> {
        self.get().fallback()
    }

    /// Returns `true` if the value is of type `T`.
    pub fn is<T: Extension>(&self) -> bool {
        self.get().as_any().is::<T>()
    }

    /// Returns the value if it's of type `T`.
    pub fn downcast_ref<T: Extension>(&self) -> Option<&T> {
        self.get().as_any().downcast_ref::<T>()
    }

    /// Returns a value borrowing from this one.
    pub fn as_borrowed(&self) -> ExtValue<'_> {
        ExtValue(Repr::Borrowed(self.get()))
    }

    /// Makes a static clone of the value decoupling the lifetimes.
    pub fn to_static(&self) -> ExtValue<'static> {
        ExtValue(Repr::Owned(self.get().clone_box()))
    }
}

impl<'a> Clone for ExtValue<'a> {
    fn clone(&self) -> Self {
        match self.0 {
            Repr::Borrowed(value) => ExtValue(Repr::Borrowed(value)),
            Repr::Owned(ref value) => ExtValue(Repr::Owned(value.clone_box())),
        }
    }
}

impl<'a> PartialEq for ExtValue<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.get().dyn_eq(other.get())
    }
}

impl<'a> fmt::Debug for ExtValue<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.get(), f)
    }
}

impl Extension for u128 {
    fn name(&self) -> &str {
        "u128"
    }

    fn fallback(&self) -> Atom<'_> {
        match u64::try_from(*self) {
            Ok(value) => Atom::U64(value),
            Err(_) => Atom::Str(self.to_string().into()),
        }
    }
}

impl Extension for i128 {
    fn name(&self) -> &str {
        "i128"
    }

    fn fallback(&self) -> Atom<'_> {
        if let Ok(value) = i64::try_from(*self) {
            Atom::I64(value)
        } else if let Ok(value) = u64::try_from(*self) {
            Atom::U64(value)
        } else {
            Atom::Str(self.to_string().into())
        }
    }
}

#[test]
fn test_auto_traits() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ExtValue<'static>>();
    assert_send_sync::<Atom<'static>>();
    assert_send_sync::<crate::Event<'static>>();
    assert_send_sync::<crate::Error>();
}

#[test]
fn test_ext_value() {
    let value = 42u128;
    let ext = ExtValue::borrowed(&value);
    assert!(ext.is::<u128>());
    assert!(!ext.is::<i128>());
    assert_eq!(ext.downcast_ref::<u128>(), Some(&42));
    assert_eq!(ext.fallback(), Atom::U64(42));
    assert_eq!(ext.to_static(), ExtValue::owned(42u128));
    assert_ne!(ext, ExtValue::owned(42i128));
    assert_eq!(format!("{:?}", ext), "42");

    let big = u128::MAX;
    assert_eq!(
        ExtValue::borrowed(&big).fallback(),
        Atom::Str(u128::MAX.to_string().into())
    );
    assert_eq!(ExtValue::owned(-1i128).fallback(), Atom::I64(-1));
    assert_eq!(
        ExtValue::owned(u64::MAX as i128).fallback(),
        Atom::U64(u64::MAX)
    );
}
