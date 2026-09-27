//! The `validator!` macro.
use std::borrow::Cow;

use crate::Violation;

/// Creates a validator type.
///
/// The validator is a unit struct which implements [`Validator`](crate::Validator)
/// for the type of its argument and for all types that borrow as it (see
/// [`Borrow`](std::borrow::Borrow)): a validator of `str` validates `String`,
/// `Box<str>` and `Cow<str>` too, a validator of `[T]` validates `Vec<T>`.
/// There are three forms:
///
/// ```
/// use deser_validate::{Validator, validator};
///
/// // a condition and the message if it's false
/// validator!(pub NonZero(port: &u16) => *port != 0, "must not be zero");
///
/// // a function
/// fn check_slug(value: &str) -> Result<(), &'static str> {
///     if value.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
///         Ok(())
///     } else {
///         Err("must be a lowercase identifier")
///     }
/// }
/// validator!(pub Slug(value: &str) = check_slug);
///
/// // a block
/// validator!(
///     /// Requires an even number of items.
///     pub EvenLength(items: &[u32]) {
///         if items.len() % 2 != 0 {
///             return Err(format!("must have an even number of items, not {}", items.len()));
///         }
///         Ok(())
///     }
/// );
///
/// assert!(NonZero::validate(&80u16).is_ok());
/// assert!(Slug::validate(&String::from("my-service")).is_ok());
/// let violation = EvenLength::validate(&vec![1, 2, 3]).unwrap_err();
/// assert_eq!(violation.code(), "even_length");
/// assert_eq!(violation.message(), "must have an even number of items, not 3");
/// ```
///
/// Functions and blocks return a `Result<(), E>`, where the error is a
/// message (`&'static str`, `String` or `Cow<'static, str>`) or a
/// [`Violation`], or a `bool`.  Messages become violations with the name of
/// the validator as code (`NonZero` has the code `non_zero`).  Functions
/// that return `false` fail with the message `is not valid`.
#[macro_export]
macro_rules! validator {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) => $cond:expr, $message:expr $(,)?
    ) => {
        $crate::validator!(
            $(#[$meta])*
            $vis $name($arg: &$ty) {
                if $cond { Ok(()) } else { Err($message) }
            }
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) = $func:path $(,)?
    ) => {
        $crate::validator!(
            $(#[$meta])*
            $vis $name($arg: &$ty) {
                $func($arg)
            }
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) $body:block
    ) => {
        $(#[$meta])*
        $vis struct $name;

        impl<T> $crate::Validator<T> for $name
        where
            T: ?::std::marker::Sized + ::std::borrow::Borrow<$ty>,
        {
            fn validate(value: &T) -> ::std::result::Result<(), $crate::Violation> {
                let check = |$arg: &$ty| $body;
                $crate::__private::into_result(
                    check(::std::borrow::Borrow::borrow(value)),
                    ::std::stringify!($name),
                )
            }
        }
    };
}

/// What the functions of validators return.
pub trait ValidationResult {
    /// Converts the result, `name` is the name of the validator.
    fn into_result(self, name: &'static str) -> Result<(), Violation>;
}

impl ValidationResult for bool {
    fn into_result(self, name: &'static str) -> Result<(), Violation> {
        match self {
            true => Ok(()),
            false => Err(Violation::new(code(name), "is not valid")),
        }
    }
}

impl<E: IntoViolation> ValidationResult for Result<(), E> {
    fn into_result(self, name: &'static str) -> Result<(), Violation> {
        self.map_err(|err| err.into_violation(name))
    }
}

/// What the functions of validators fail with.
pub trait IntoViolation {
    /// Converts the error, `name` is the name of the validator.
    fn into_violation(self, name: &'static str) -> Violation;
}

impl IntoViolation for Violation {
    fn into_violation(self, _name: &'static str) -> Violation {
        self
    }
}

impl IntoViolation for &'static str {
    fn into_violation(self, name: &'static str) -> Violation {
        Violation::new(code(name), self)
    }
}

impl IntoViolation for String {
    fn into_violation(self, name: &'static str) -> Violation {
        Violation::new(code(name), self)
    }
}

impl IntoViolation for Cow<'static, str> {
    fn into_violation(self, name: &'static str) -> Violation {
        Violation::new(code(name), self)
    }
}

/// Returns the code of a validator: its name in snake case.
///
/// Acronyms are kept together (`URLCheck` is `url_check`).
fn code(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut rv = String::with_capacity(name.len() + 4);
    for (idx, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && idx > 0 {
            let prev = chars[idx - 1];
            let next_is_lower = chars.get(idx + 1).is_some_and(|x| x.is_lowercase());
            if prev.is_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_uppercase() && next_is_lower)
            {
                rv.push('_');
            }
        }
        rv.extend(c.to_lowercase());
    }
    rv
}

/// Used by the `validator!` macro.  Not public API.
#[doc(hidden)]
pub mod __private {
    use super::ValidationResult;
    use crate::Violation;

    #[inline]
    pub fn into_result<R: ValidationResult>(rv: R, name: &'static str) -> Result<(), Violation> {
        rv.into_result(name)
    }
}

#[test]
fn test_code() {
    assert_eq!(code("NonZero"), "non_zero");
    assert_eq!(code("Slug"), "slug");
    assert_eq!(code("URL"), "url");
    assert_eq!(code("URLCheck"), "url_check");
    assert_eq!(code("MaxLen2"), "max_len2");
}
