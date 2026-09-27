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
/// the validator in snake case as code (`NonZero` has the code `non_zero`).
/// Functions that return `false` fail with the message `is not valid`.
///
/// The code is part of what clients see, renaming the validator changes it.
/// All forms accept a code that is used instead of the name as last
/// argument, violations that functions return keep their code:
///
/// ```
/// use deser_validate::{Validator, validator};
///
/// validator!(pub Port(port: &u16) => *port != 0, "must not be zero", code = "port");
/// assert_eq!(Port::validate(&0u16).unwrap_err().code(), "port");
/// ```
///
/// The macro does not support types with generics or lifetimes (like
/// `Either<T>` or `Name<'a>`).  For those, implement
/// [`Validator`](crate::Validator) yourself (see there).
#[macro_export]
macro_rules! validator {
    (
        @impl [$(#[$meta:meta])*]
        $vis:vis $name:ident($arg:ident: &$ty:ty) $body:block ($code:expr)
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
                    $code,
                )
            }
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) => $cond:expr, $message:expr,
        code = $code:literal $(,)?
    ) => {
        $crate::validator!(
            @impl [$(#[$meta])*] $vis $name($arg: &$ty) {
                if $cond { Ok(()) } else { Err($message) }
            } (::std::option::Option::Some($code))
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) => $cond:expr, $message:expr $(,)?
    ) => {
        $crate::validator!(
            @impl [$(#[$meta])*] $vis $name($arg: &$ty) {
                if $cond { Ok(()) } else { Err($message) }
            } (::std::option::Option::None)
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) = $func:path, code = $code:literal $(,)?
    ) => {
        $crate::validator!(
            @impl [$(#[$meta])*] $vis $name($arg: &$ty) {
                $func($arg)
            } (::std::option::Option::Some($code))
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) = $func:path $(,)?
    ) => {
        $crate::validator!(
            @impl [$(#[$meta])*] $vis $name($arg: &$ty) {
                $func($arg)
            } (::std::option::Option::None)
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) $body:block, code = $code:literal $(,)?
    ) => {
        $crate::validator!(
            @impl [$(#[$meta])*] $vis $name($arg: &$ty) $body (::std::option::Option::Some($code))
        );
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident($arg:ident: &$ty:ty) $body:block $(,)?
    ) => {
        $crate::validator!(
            @impl [$(#[$meta])*] $vis $name($arg: &$ty) $body (::std::option::Option::None)
        );
    };
}

/// What the functions of validators return.
pub trait ValidationResult {
    /// Converts the result.
    ///
    /// `name` is the name of the validator, `code` the code it was given.
    fn into_result(self, name: &'static str, code: Option<&'static str>) -> Result<(), Violation>;
}

impl ValidationResult for bool {
    fn into_result(self, name: &'static str, code: Option<&'static str>) -> Result<(), Violation> {
        match self {
            true => Ok(()),
            false => Err("is not valid".into_violation(name, code)),
        }
    }
}

impl<E: IntoViolation> ValidationResult for Result<(), E> {
    fn into_result(self, name: &'static str, code: Option<&'static str>) -> Result<(), Violation> {
        self.map_err(|err| err.into_violation(name, code))
    }
}

/// What the functions of validators fail with.
pub trait IntoViolation {
    /// Converts the error.
    ///
    /// `name` is the name of the validator, `code` the code it was given.
    /// Messages become violations with the code, or the name in snake case
    /// if it has none.
    fn into_violation(self, name: &'static str, code: Option<&'static str>) -> Violation;
}

impl IntoViolation for Violation {
    fn into_violation(self, _name: &'static str, _code: Option<&'static str>) -> Violation {
        self
    }
}

/// Returns the code of a validator.
fn code_of(name: &'static str, code: Option<&'static str>) -> Cow<'static, str> {
    match code {
        Some(code) => Cow::Borrowed(code),
        None => Cow::Owned(snake_case(name)),
    }
}

impl IntoViolation for &'static str {
    fn into_violation(self, name: &'static str, code: Option<&'static str>) -> Violation {
        Violation::new(code_of(name, code), self)
    }
}

impl IntoViolation for String {
    fn into_violation(self, name: &'static str, code: Option<&'static str>) -> Violation {
        Violation::new(code_of(name, code), self)
    }
}

impl IntoViolation for Cow<'static, str> {
    fn into_violation(self, name: &'static str, code: Option<&'static str>) -> Violation {
        Violation::new(code_of(name, code), self)
    }
}

/// Returns the code of a validator: its name in snake case.
///
/// Acronyms are kept together (`URLCheck` is `url_check`).
fn snake_case(name: &str) -> String {
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
    pub fn into_result<R: ValidationResult>(
        rv: R,
        name: &'static str,
        code: Option<&'static str>,
    ) -> Result<(), Violation> {
        rv.into_result(name, code)
    }
}

#[test]
fn test_snake_case() {
    assert_eq!(snake_case("NonZero"), "non_zero");
    assert_eq!(snake_case("Slug"), "slug");
    assert_eq!(snake_case("URL"), "url");
    assert_eq!(snake_case("URLCheck"), "url_check");
    assert_eq!(snake_case("MaxLen2"), "max_len2");
}
