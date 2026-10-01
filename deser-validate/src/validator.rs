//! The validator trait and violations.
use std::borrow::Cow;
use std::fmt;

use deser_core::{Error, ErrorAttachment, ErrorKind};

/// Validates values of type `T`.
///
/// Validators are types (usually without fields) so that they can be named
/// in the types of fields, for instance `Validated<String, Email>`.  They
/// are parameterized with const generics (`Len<1, 64>`) and combined with
/// tuples: `(NonEmpty, MaxLen<64>)` requires all of them.  Validators that
/// need data that cannot be a const generic (like a pattern) are types of
/// their own.  Implementing them for all types that are strings covers
/// `String`, `&str`, `Cow<str>` and `Box<str>`:
///
/// ```
/// use deser_validate::{Validator, Violation};
///
/// /// A lowercase identifier like `my-service`.
/// pub struct Slug;
///
/// impl<T: AsRef<str> + ?Sized> Validator<T> for Slug {
///     fn validate(value: &T) -> Result<(), Violation> {
///         let value = value.as_ref();
///         if !value.is_empty()
///             && value.bytes().all(|b| {
///                 b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
///             })
///         {
///             Ok(())
///         } else {
///             Err(Violation::new("slug", "must be a lowercase identifier"))
///         }
///     }
/// }
///
/// assert!(Slug::validate("my-service").is_ok());
/// assert!(Slug::validate(&"My Service".to_string()).is_err());
/// ```
///
/// Most validators are easier to write with the
/// [`validator!`](macro@crate::validator) macro, which turns a condition or
/// a function into a validator type.
/// Implementing the trait is needed for types with generics or lifetimes,
/// which the macro does not support:
///
/// ```
/// use deser_validate::{Validator, Violation};
///
/// pub struct Name<'a>(&'a str);
///
/// pub struct NonEmptyName;
///
/// impl<'a> Validator<Name<'a>> for NonEmptyName {
///     fn validate(value: &Name<'a>) -> Result<(), Violation> {
///         if value.0.is_empty() {
///             return Err(Violation::new("not_empty", "must not be empty"));
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait Validator<T: ?Sized> {
    /// Validates the value.
    fn validate(value: &T) -> Result<(), Violation>;
}

/// Accepts every value.
impl<T: ?Sized> Validator<T> for () {
    fn validate(_value: &T) -> Result<(), Violation> {
        Ok(())
    }
}

macro_rules! impl_tuple {
    ($($name:ident),*) => {
        /// Requires all validators, the first violation is reported.
        impl<T: ?Sized, $($name: Validator<T>),*> Validator<T> for ($($name,)*) {
            fn validate(value: &T) -> Result<(), Violation> {
                $($name::validate(value)?;)*
                Ok(())
            }
        }
    };
}

impl_tuple!(A);
impl_tuple!(A, B);
impl_tuple!(A, B, C);
impl_tuple!(A, B, C, D);
impl_tuple!(A, B, C, D, E);
impl_tuple!(A, B, C, D, E, F);

/// A parameter of a [`Violation`].
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// An integer.
    Int(i128),
    /// A string.
    Str(Cow<'static, str>),
}

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Param::Int(value) => fmt::Display::fmt(value, f),
            Param::Str(value) => fmt::Display::fmt(value, f),
        }
    }
}

impl From<i128> for Param {
    fn from(value: i128) -> Param {
        Param::Int(value)
    }
}

impl From<usize> for Param {
    fn from(value: usize) -> Param {
        Param::Int(value as i128)
    }
}

impl From<&'static str> for Param {
    fn from(value: &'static str) -> Param {
        Param::Str(Cow::Borrowed(value))
    }
}

impl From<String> for Param {
    fn from(value: String) -> Param {
        Param::Str(Cow::Owned(value))
    }
}

/// Why a value is invalid.
///
/// A violation has a code which identifies the rule that was violated (for
/// instance `length`), a message for humans and parameters (for instance
/// the minimum length).  Codes and parameters are meant for programs, for
/// instance to translate the messages or to point at the value in a user
/// interface.
///
/// When a value is invalid, the error has the violation attached (see
/// [`Error::attachment`]):
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::{Check, Email, Violation};
///
/// #[derive(Deserialize, Debug)]
/// struct User {
///     #[deser(as = Check<Email>)]
///     email: String,
/// }
///
/// let json = r#"{"email": "nope"}"#;
/// let err = deser_json::from_str::<User>(json).unwrap_err();
/// assert_eq!(
///     err.to_string(),
///     "Unexpected: invalid value: must be an email address \
///      at line 1 column 11"
/// );
/// assert_eq!(err.attachment::<Violation>().unwrap().code(), "email");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    code: Cow<'static, str>,
    message: Cow<'static, str>,
    params: Vec<(&'static str, Param)>,
}

impl Violation {
    /// Creates a violation with a code and a message.
    pub fn new<C, M>(code: C, message: M) -> Violation
    where
        C: Into<Cow<'static, str>>,
        M: Into<Cow<'static, str>>,
    {
        Violation {
            code: code.into(),
            message: message.into(),
            params: Vec::new(),
        }
    }

    /// Adds a parameter.
    pub fn with_param<P: Into<Param>>(mut self, name: &'static str, value: P) -> Violation {
        self.params.push((name, value.into()));
        self
    }

    /// Returns the code of the rule that was violated.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the parameters.
    pub fn params(&self) -> &[(&'static str, Param)] {
        &self.params
    }

    /// Returns the value of a parameter.
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    /// Creates the error for the violation.
    ///
    /// The error has the violation attached, the message is the same as
    /// the one of `#[deser(validate = ...)]`.
    pub fn into_error(self) -> Error {
        Error::new(
            ErrorKind::Unexpected,
            format!("invalid value: {}", self.message),
        )
        .with_attachment(self)
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl ErrorAttachment for Violation {}
