//! The validators provided by this crate.
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;

use crate::{Validator, Violation};

/// Values that have a length (see [`Len`] and [`NonEmpty`]).
///
/// The length of strings is the number of characters, of collections the
/// number of items.
pub trait Length {
    /// Returns the length.
    fn length(&self) -> usize;
}

impl Length for str {
    fn length(&self) -> usize {
        self.chars().count()
    }
}

impl Length for String {
    fn length(&self) -> usize {
        self.as_str().length()
    }
}

impl<T> Length for [T] {
    fn length(&self) -> usize {
        self.len()
    }
}

impl<T, const N: usize> Length for [T; N] {
    fn length(&self) -> usize {
        N
    }
}

macro_rules! impl_length {
    ($($ty:ty => [$($param:tt)*];)*) => {
        $(
            impl<$($param)*> Length for $ty {
                fn length(&self) -> usize {
                    self.len()
                }
            }
        )*
    };
}

impl_length! {
    Vec<T> => [T];
    VecDeque<T> => [T];
    BTreeSet<T> => [T];
    HashSet<T, S> => [T, S];
    BTreeMap<K, V> => [K, V];
    HashMap<K, V, S> => [K, V, S];
}

impl<T: Length + ?Sized> Length for &T {
    fn length(&self) -> usize {
        (**self).length()
    }
}

impl<T: Length + ?Sized> Length for Box<T> {
    fn length(&self) -> usize {
        (**self).length()
    }
}

impl<T: Length + ToOwned + ?Sized> Length for Cow<'_, T> {
    fn length(&self) -> usize {
        (**self).length()
    }
}

/// Requires a string or collection that is not empty.
pub struct NonEmpty;

impl<T: Length + ?Sized> Validator<T> for NonEmpty {
    fn validate(value: &T) -> Result<(), Violation> {
        if value.length() == 0 {
            Err(Violation::new("non_empty", "must not be empty"))
        } else {
            Ok(())
        }
    }
}

/// Requires the length of a string or collection to be in a range.
///
/// Both ends are inclusive.  Strings count characters, collections items
/// (see [`Length`]).  The violation has the code `length` and the
/// parameters `min` and `max` (if they are set).
///
/// ```
/// use deser_validate::{Len, MaxLen, Validator};
///
/// assert!(Len::<1, 3>::validate("abc").is_ok());
/// assert_eq!(
///     Len::<1, 3>::validate("abcd").unwrap_err().message(),
///     "length must be between 1 and 3"
/// );
/// assert!(MaxLen::<2>::validate(&vec![1, 2, 3]).is_err());
/// ```
pub struct Len<const MIN: usize, const MAX: usize = { usize::MAX }>;

/// Requires a minimum length (see [`Len`]).
pub type MinLen<const N: usize> = Len<N>;

/// Requires a maximum length (see [`Len`]).
pub type MaxLen<const N: usize> = Len<0, N>;

impl<T: Length + ?Sized, const MIN: usize, const MAX: usize> Validator<T> for Len<MIN, MAX> {
    fn validate(value: &T) -> Result<(), Violation> {
        let len = value.length();
        if (MIN..=MAX).contains(&len) {
            return Ok(());
        }
        let violation = match (MIN, MAX) {
            (min, usize::MAX) => {
                Violation::new("length", format!("length must be at least {}", min))
                    .with_param("min", min)
            }
            (0, max) => Violation::new("length", format!("length must be at most {}", max))
                .with_param("max", max),
            (min, max) => Violation::new(
                "length",
                format!("length must be between {} and {}", min, max),
            )
            .with_param("min", min)
            .with_param("max", max),
        };
        Err(violation)
    }
}

/// Integers (see [`Range`]).
pub trait Integer {
    /// Returns the value as `i128`, `None` if it's larger.
    fn to_i128(&self) -> Option<i128>;
}

macro_rules! impl_integer {
    ($($ty:ty),*) => {
        $(
            impl Integer for $ty {
                fn to_i128(&self) -> Option<i128> {
                    i128::try_from(*self).ok()
                }
            }
        )*
    };
}

impl_integer!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

/// Requires an integer to be in a range.
///
/// Both ends are inclusive.  The violation has the code `range` and the
/// parameters `min` and `max` (if they are set).
///
/// ```
/// use deser_validate::{Min, Range, Validator};
///
/// assert!(Range::<1, 65535>::validate(&80u16).is_ok());
/// assert_eq!(
///     Range::<1, 65535>::validate(&0u16).unwrap_err().message(),
///     "must be between 1 and 65535"
/// );
/// assert!(Min::<1>::validate(&0u32).is_err());
/// ```
pub struct Range<const MIN: i128, const MAX: i128>;

/// Requires a minimum value (see [`Range`]).
pub type Min<const N: i128> = Range<N, { i128::MAX }>;

/// Requires a maximum value (see [`Range`]).
pub type Max<const N: i128> = Range<{ i128::MIN }, N>;

impl<T: Integer + ?Sized, const MIN: i128, const MAX: i128> Validator<T> for Range<MIN, MAX> {
    fn validate(value: &T) -> Result<(), Violation> {
        // values that do not fit into i128 are larger than every maximum
        let valid = match value.to_i128() {
            Some(value) => (MIN..=MAX).contains(&value),
            None => MAX == i128::MAX,
        };
        if valid {
            return Ok(());
        }
        let violation = match (MIN, MAX) {
            (min, i128::MAX) => {
                Violation::new("range", format!("must be at least {}", min)).with_param("min", min)
            }
            (i128::MIN, max) => {
                Violation::new("range", format!("must be at most {}", max)).with_param("max", max)
            }
            (min, max) => Violation::new("range", format!("must be between {} and {}", min, max))
                .with_param("min", min)
                .with_param("max", max),
        };
        Err(violation)
    }
}

/// Requires a string that looks like an email address.
///
/// This checks the structure only: a local part and a domain separated by
/// a single `@`, the domain has at least two labels and there is no
/// whitespace.  Whether the address exists can only be found out by
/// sending an email.  The violation has the code `email`.
///
/// ```
/// use deser_validate::{Email, Validator};
///
/// assert!(Email::validate("jane@example.com").is_ok());
/// assert!(Email::validate("jane@localhost").is_err());
/// assert!(Email::validate("jane example.com").is_err());
/// ```
pub struct Email;

impl<T: AsRef<str> + ?Sized> Validator<T> for Email {
    fn validate(value: &T) -> Result<(), Violation> {
        let value = value.as_ref();
        let valid = match value.split_once('@') {
            Some((local, domain)) => {
                !local.is_empty()
                    && !domain.contains('@')
                    && !value.chars().any(char::is_whitespace)
                    && domain.contains('.')
                    && domain.split('.').all(|label| !label.is_empty())
            }
            None => false,
        };
        if valid {
            Ok(())
        } else {
            Err(Violation::new("email", "must be an email address"))
        }
    }
}

/// Validates every item of a collection with `V`.
///
/// The violation of the first invalid item is reported with its index as
/// parameter `index`.  For an `Option` the value is validated if there is
/// one.
///
/// ```
/// use deser_validate::{Each, Email, Validator};
///
/// let emails = vec!["jane@example.com".to_string(), "nope".to_string()];
/// let violation = Each::<Email>::validate(&emails).unwrap_err();
/// assert_eq!(violation.message(), "item 1: must be an email address");
/// assert!(Each::<Email>::validate(&None::<String>).is_ok());
/// ```
pub struct Each<V>(PhantomData<fn() -> V>);

impl<V> Each<V> {
    fn validate_items<'a, T: 'a, I>(items: I) -> Result<(), Violation>
    where
        V: Validator<T>,
        I: IntoIterator<Item = &'a T>,
    {
        for (index, item) in items.into_iter().enumerate() {
            if let Err(violation) = V::validate(item) {
                let message = format!("item {}: {}", index, violation.message());
                let mut rv = Violation::new(violation.code().to_string(), message);
                for (name, value) in violation.params() {
                    rv = rv.with_param(name, value.clone());
                }
                return Err(rv.with_param("index", index));
            }
        }
        Ok(())
    }
}

macro_rules! impl_each {
    ($($ty:ty => [$($param:tt)*];)*) => {
        $(
            impl<V: Validator<T>, $($param)*> Validator<$ty> for Each<V> {
                fn validate(value: &$ty) -> Result<(), Violation> {
                    Each::<V>::validate_items(value.iter())
                }
            }
        )*
    };
}

impl_each! {
    [T] => [T];
    Vec<T> => [T];
    VecDeque<T> => [T];
    BTreeSet<T> => [T];
    HashSet<T, S> => [T, S];
    Option<T> => [T];
}

impl<V: Validator<T>, T, const N: usize> Validator<[T; N]> for Each<V> {
    fn validate(value: &[T; N]) -> Result<(), Violation> {
        Each::<V>::validate_items(value.iter())
    }
}
