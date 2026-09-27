//! Validation for [deser](https://docs.rs/deser).
//!
//! Values are validated while they are deserialized, so errors point at the
//! value in the input (with its line, column and path) like the errors of
//! the format.  Validators are types (see [`Validator`]), so they are
//! named in adapters and in the types of fields:
//!
//! * The [`Check<V>`](Check) adapter (`#[deser(as = Check<V>)]`) fails the
//!   deserialization if the value is invalid.  The type of the field does
//!   not change.
//! * [`Checked<T, V>`](Checked) does the same, a `Checked` value is always
//!   valid.
//! * [`Validated<T, V>`](Validated) keeps the errors of the value in it
//!   instead of failing, so the value around it can still be
//!   deserialized.  This also covers errors like a string that is given
//!   where a number is expected.
//! * [`Collect<T>`](Collect) collects all errors of a value instead of
//!   failing on the first.
//! * A [`Validation`] reports all problems of an input.
//!
//! ```
//! use deser::Deserialize;
//! use deser_validate::{Checked, Email, Len, NonEmpty, Validated, Validation};
//!
//! #[derive(Deserialize)]
//! struct Signup {
//!     // must be valid, or the signup is invalid
//!     name: Checked<String, (NonEmpty, Len<1, 32>)>,
//!     // kept even if it is invalid
//!     email: Validated<String, Email>,
//!     newsletter: bool,
//! }
//!
//! let input = r#"{"name": "", "email": "jane@", "newsletter": "yes"}"#;
//! let validation = Validation::new();
//! let rv = deser_json::Deserializer::from_str(input)
//!     .deserialize_with::<Signup, _>(|driver| validation.setup(driver));
//! let report = validation.finish(rv).into_result().err().unwrap();
//! let issues: Vec<_> = report
//!     .iter()
//!     .map(|issue| format!("{}: {}", issue.path().unwrap(), issue.message()))
//!     .collect();
//! assert_eq!(
//!     issues,
//!     [
//!         "name: invalid value: must not be empty",
//!         "email: invalid value: must be an email address",
//!         "newsletter: unexpected string, expected bool",
//!     ]
//! );
//! ```
//!
//! # Validators
//!
//! The validators of this crate are [`NonEmpty`], [`Len`] (and [`MinLen`]
//! and [`MaxLen`]), [`Range`] (and [`Min`] and [`Max`]), [`Email`] and
//! [`Each`], which validates the items of collections.  Tuples of
//! validators require all of them.  The [`validator!`] macro creates
//! validators from conditions and functions:
//!
//! ```
//! use deser::Deserialize;
//! use deser_validate::{Check, validator};
//!
//! validator!(NonZero(port: &u16) => *port != 0, "must not be zero");
//!
//! #[derive(Deserialize)]
//! struct Server {
//!     #[deser(as = Check<NonZero>)]
//!     port: u16,
//! }
//! ```
//!
//! Custom validators can also implement [`Validator`] themselves.
//!
//! Validators report a [`Violation`] with a code and parameters for
//! programs and a message for humans.  Errors have the violation attached.
mod check;
mod checked;
mod collect;
mod macros;
mod report;
mod validated;
mod validator;
mod validators;

pub use self::check::Check;
pub use self::checked::Checked;
pub use self::collect::Collect;
#[doc(hidden)]
pub use self::macros::__private;
pub use self::macros::{IntoViolation, ValidationResult};
pub use self::report::{Issue, Outcome, Report, Validation};
pub use self::validated::Validated;
pub use self::validator::{Param, Validator, Violation, check};
pub use self::validators::{
    Each, Email, Integer, Len, Length, Max, MaxLen, Min, MinLen, NonEmpty, Range,
};
