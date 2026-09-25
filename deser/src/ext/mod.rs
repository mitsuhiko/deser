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
//! Deser itself uses this for `u128` and `i128` and a set of well-known
//! types (see below).  For a more complete example which annotates every
//! value with its path and shows how that information survives internal
//! buffering, see the
//! [`located` example](https://github.com/mitsuhiko/deser/tree/main/examples/located).
//!
//! # Borrowing Extensions
//!
//! Extension values can borrow data, for instance a number that keeps its
//! original text from the input.  Such extensions implement
//! [`BorrowedExtension`] on a `'static` key type which defines the type of
//! the values for a lifetime.  Their values are created with
//! [`ExtValue::borrowed_value`] or [`ExtValue::owned_value`] and looked up
//! with [`ExtValue::downcast_value_ref`].  When values are buffered they are
//! detached from the data they borrow with
//! [`BorrowedExtension::to_static`].
//!
//! # Well-Known Types
//!
//! Some types are common enough that many data formats support them
//! natively, but they are not part of the core data model.  For these deser
//! provides well-known extension types.  They are simple dependency free
//! representations that formats can understand and that the types of other
//! crates convert into:
//!
//! | Type          | Represents                           | Fallback                     |
//! |---------------|--------------------------------------|------------------------------|
//! | [`Datetime`]  | dates, times and date-times          | RFC 3339 string              |
//! | [`Timestamp`] | instants in time                     | RFC 3339 string in UTC       |
//! | [`Duration`]  | exact lengths of time                | ISO 8601 duration string     |
//! | [`Uuid`]      | UUIDs                                | hyphenated string            |
//! | [`Decimal`]   | exact decimal numbers                | decimal string               |
//! | [`BigInt`]    | integers that do not fit 128 bits    | decimal string               |
//!
//! All well-known types implement [`Serialize`](crate::Serialize) and
//! [`Deserialize`](crate::Deserialize).  When deserialized they accept
//! their own extension value, the fallback and other representations where
//! that makes sense (for instance 16 bytes for a [`Uuid`] or an integer for
//! a [`Timestamp`]).
//!
//! Types of the standard library and of other crates are serialized as
//! well-known types:
//!
//! * [`std::time::SystemTime`] as [`Timestamp`] and
//!   [`std::time::Duration`] as [`Duration`].
//! * `jiff` (feature `jiff`): `Timestamp` as [`Timestamp`], `Zoned`,
//!   `civil::DateTime`, `civil::Date` and `civil::Time` as [`Datetime`],
//!   `SignedDuration` as [`Duration`].
//! * `chrono` (feature `chrono`): `DateTime<Utc>` as [`Timestamp`],
//!   `DateTime<FixedOffset>`, `NaiveDateTime`, `NaiveDate` and `NaiveTime`
//!   as [`Datetime`], `TimeDelta` as [`Duration`].
//! * `time` (feature `time`): `UtcDateTime` as [`Timestamp`],
//!   `OffsetDateTime`, `PrimitiveDateTime`, `Date` and `Time` as
//!   [`Datetime`], `Duration` as [`Duration`].
//! * `uuid` (feature `uuid`): `Uuid` as [`Uuid`].
//! * `rust_decimal` (feature `rust_decimal`): `Decimal` as [`Decimal`].
//! * `bigdecimal` (feature `bigdecimal`): `BigDecimal` as [`Decimal`].
//! * `num-bigint` (feature `num-bigint`): `BigInt` and `BigUint` as
//!   integers, using [`BigInt`] for values that do not fit into 128 bits.
//!
//! Types that map onto [`Datetime`] require the matching kind of date-time
//! when deserialized: a `jiff::civil::Date` can only be deserialized from a
//! local date.
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
use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

use crate::event::Atom;

mod bigint;
mod bridges;
mod datetime;
mod decimal;
mod duration;
pub(crate) mod known;
mod uuid;

pub use self::bigint::BigInt;
pub use self::datetime::{Date, Datetime, Offset, Time, Timestamp};
pub use self::decimal::Decimal;
pub use self::duration::Duration;
pub use self::uuid::Uuid;

/// A type that can be passed through deser as an extension to the data model.
///
/// This is implemented for extension types without lifetimes, which covers
/// most extensions.  Types that borrow data implement
/// [`BorrowedExtension`] instead.
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

/// An extension whose values can borrow data.
///
/// The trait is implemented by a `'static` key type which identifies the
/// extension.  The values are of type [`Value<'a>`](Self::Value) and can
/// borrow for `'a`.  Typically the key is the value type with a `'static`
/// lifetime.  Every [`Extension`] is a borrowed extension whose values are
/// of the type itself.
///
/// All methods are associated functions that take the value.  Values are
/// created with [`ExtValue::borrowed_value`] and
/// [`ExtValue::owned_value`] and looked up with
/// [`ExtValue::downcast_value_ref`] with the key type:
///
/// ```
/// use std::borrow::Cow;
/// use deser::ext::{BorrowedExtension, ExtValue};
/// use deser::Atom;
///
/// /// A number with its original text.
/// #[derive(Debug, Clone, PartialEq)]
/// pub struct Literal<'a> {
///     pub text: Cow<'a, str>,
///     pub value: f64,
/// }
///
/// impl BorrowedExtension for Literal<'static> {
///     type Value<'a> = Literal<'a>;
///
///     fn name<'v>(_value: &'v Literal<'_>) -> &'v str {
///         "literal"
///     }
///
///     fn fallback<'v>(value: &'v Literal<'_>) -> Atom<'v> {
///         Atom::F64(value.value)
///     }
///
///     fn to_static(value: &Literal<'_>) -> Literal<'static> {
///         Literal {
///             text: Cow::Owned(value.text.to_string()),
///             value: value.value,
///         }
///     }
///
///     fn shorten<'s, 'l: 's>(value: &'s Literal<'l>) -> &'s Literal<'s> {
///         value
///     }
/// }
///
/// let input = String::from("1.50");
/// let literal = Literal { text: Cow::Borrowed(&input), value: 1.5 };
/// let ext = ExtValue::borrowed_value::<Literal>(&literal);
/// assert_eq!(ext.downcast_value_ref::<Literal>().unwrap().text, "1.50");
/// assert_eq!(ext.fallback(), Atom::F64(1.5));
/// ```
pub trait BorrowedExtension: 'static {
    /// The type of the values of this extension.
    type Value<'a>: fmt::Debug + PartialEq + Send + Sync + 'a;

    /// Returns the human readable name of the extension type.
    ///
    /// This is used for error messages.
    fn name<'v>(value: &'v Self::Value<'_>) -> &'v str;

    /// Lowers the value into the core data model.
    ///
    /// See [`Extension::fallback`].
    fn fallback<'v>(value: &'v Self::Value<'_>) -> Atom<'v>;

    /// Detaches a value from the data it borrows.
    ///
    /// This is used when values are buffered (see
    /// [`ExtValue::to_static`]).
    fn to_static(value: &Self::Value<'_>) -> Self::Value<'static>;

    /// Shortens the lifetime of a value.
    ///
    /// This proves that a value can be used with a shorter lifetime.  For
    /// types that are covariant in their lifetime (which is the case for
    /// most types that hold references or [`Cow`](std::borrow::Cow)s) the
    /// implementation is just `value`.
    fn shorten<'s, 'l: 's>(value: &'s Self::Value<'l>) -> &'s Self::Value<'s>;
}

impl<T: Extension> BorrowedExtension for T {
    type Value<'a> = T;

    fn name(value: &T) -> &str {
        Extension::name(value)
    }

    fn fallback<'v>(value: &'v T) -> Atom<'v> {
        Extension::fallback(value)
    }

    fn to_static(value: &T) -> T {
        value.clone()
    }

    fn shorten<'s, 'l: 's>(value: &'s T) -> &'s T {
        value
    }
}

/// The object safe interface to extension values.
trait ErasedExtension: fmt::Debug + Send + Sync {
    /// The type id of the key of the extension.
    fn key(&self) -> TypeId;
    fn name(&self) -> &str;
    fn fallback(&self) -> Atom<'_>;
    fn to_static(&self) -> Arc<dyn ErasedExtension>;
    /// Returns a pointer to a `K::Value<'s>` where `'s` is the lifetime of
    /// the borrow of self.
    fn value_ptr(&self) -> *const ();
    fn dyn_eq(&self, other: &dyn ErasedExtension) -> bool;
}

/// Holds the value of an extension with the key `K`.
#[repr(transparent)]
struct Holder<'x, K: BorrowedExtension>(K::Value<'x>);

impl<'x, K: BorrowedExtension> fmt::Debug for Holder<'x, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<'x, K: BorrowedExtension> ErasedExtension for Holder<'x, K> {
    fn key(&self) -> TypeId {
        TypeId::of::<K>()
    }

    fn name(&self) -> &str {
        K::name(&self.0)
    }

    fn fallback(&self) -> Atom<'_> {
        K::fallback(&self.0)
    }

    fn to_static(&self) -> Arc<dyn ErasedExtension> {
        Arc::new(Holder::<'static, K>(K::to_static(&self.0)))
    }

    fn value_ptr(&self) -> *const () {
        // the value is shortened through the implementation of the
        // extension, which proves that it's valid for the shorter lifetime.
        K::shorten(&self.0) as *const K::Value<'_> as *const ()
    }

    fn dyn_eq(&self, other: &dyn ErasedExtension) -> bool {
        other.key() == TypeId::of::<K>()
            // SAFETY: the keys match, so the pointer points to a
            // `K::Value<'s>` for the borrow of `other`.
            && K::shorten(&self.0) == unsafe { &*(other.value_ptr() as *const K::Value<'_>) }
    }
}

/// An extension value carried by [`Atom::Ext`].
///
/// The value is either borrowed (which is typical during serialization and
/// for values that formats create while parsing) or owned.  Owned values are
/// reference counted, so cloning an extension value is cheap.
///
/// Extension values are covariant in their lifetime and they can borrow
/// data (see [`BorrowedExtension`]).
pub struct ExtValue<'a>(Repr<'a>);

enum Repr<'a> {
    Borrowed(&'a (dyn ErasedExtension + 'a)),
    Owned(Arc<dyn ErasedExtension + 'a>),
}

impl<'a> ExtValue<'a> {
    /// Creates an extension value borrowing from a value.
    pub fn borrowed<T: Extension>(value: &'a T) -> ExtValue<'a> {
        ExtValue::borrowed_value::<T>(value)
    }

    /// Creates an extension value that owns the value.
    pub fn owned<T: Extension>(value: T) -> ExtValue<'a> {
        ExtValue::owned_value::<T>(value)
    }

    /// Creates an extension value borrowing from the value of an extension.
    ///
    /// The extension is identified by its key `K` (see
    /// [`BorrowedExtension`]).
    pub fn borrowed_value<K: BorrowedExtension>(value: &'a K::Value<'a>) -> ExtValue<'a> {
        // SAFETY: the holder is a transparent wrapper around the value
        let holder = unsafe { &*(value as *const K::Value<'a> as *const Holder<'a, K>) };
        ExtValue(Repr::Borrowed(holder))
    }

    /// Creates an extension value that owns the value of an extension.
    ///
    /// The extension is identified by its key `K` (see
    /// [`BorrowedExtension`]).
    pub fn owned_value<K: BorrowedExtension>(value: K::Value<'a>) -> ExtValue<'a> {
        ExtValue(Repr::Owned(Arc::new(Holder::<'a, K>(value))))
    }

    fn get(&self) -> &(dyn ErasedExtension + 'a) {
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

    /// Returns `true` if the value is of the extension with the key `K`.
    ///
    /// For extensions without lifetimes the key is the type.
    pub fn is<K: BorrowedExtension>(&self) -> bool {
        self.get().key() == TypeId::of::<K>()
    }

    /// Returns the value if it's of type `T`.
    ///
    /// ```
    /// use deser::ext::ExtValue;
    ///
    /// let ext = ExtValue::owned(42u128);
    /// assert_eq!(ext.downcast_ref::<u128>(), Some(&42));
    /// ```
    ///
    /// For extensions that borrow use
    /// [`downcast_value_ref`](Self::downcast_value_ref).
    pub fn downcast_ref<T: Extension>(&self) -> Option<&T> {
        self.downcast_value_ref::<T>()
    }

    /// Returns the value if it's of the extension with the key `K`.
    ///
    /// See [`BorrowedExtension`] for an example.  The returned value borrows
    /// from this extension value.
    pub fn downcast_value_ref<K: BorrowedExtension>(&self) -> Option<&K::Value<'_>> {
        let value = self.get();
        if value.key() == TypeId::of::<K>() {
            // SAFETY: the keys match, so the pointer points to a
            // `K::Value<'s>` for the borrow of self.
            Some(unsafe { &*(value.value_ptr() as *const K::Value<'_>) })
        } else {
            None
        }
    }

    /// Returns a value borrowing from this one.
    pub fn as_borrowed(&self) -> ExtValue<'_> {
        ExtValue(Repr::Borrowed(self.get()))
    }

    /// Makes a static clone of the value decoupling the lifetimes.
    ///
    /// Values that borrow data are detached from it (see
    /// [`BorrowedExtension::to_static`]).
    pub fn to_static(&self) -> ExtValue<'static> {
        ExtValue(Repr::Owned(self.get().to_static()))
    }
}

impl<'a> Clone for ExtValue<'a> {
    fn clone(&self) -> Self {
        match self.0 {
            Repr::Borrowed(value) => ExtValue(Repr::Borrowed(value)),
            Repr::Owned(ref value) => ExtValue(Repr::Owned(value.clone())),
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
