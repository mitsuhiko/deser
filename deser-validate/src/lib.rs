//! Validation for [deser](https://docs.rs/deser).
//!
//! Values are validated while they are deserialized, so errors point at the
//! value in the input (with its line, column and path) like the errors of
//! the format.  Validators are types (see [`Validator`]), so they are
//! named in adapters and in the types of fields:
//!
//! * The [`Check<V>`](Check) adapter (`#[deser(as = Check<V>)]`) fails the
//!   deserialization if the value is invalid.  The type of the field does
//!   not change.  On a type (`#[deser(deserialize_as = Check<V, _>)]`) it
//!   checks the whole value, for rules that span fields.
//! * [`Validated<T, V>`](Validated) keeps all errors of the value in it
//!   instead of failing, so the value around it can still be
//!   deserialized.  This also covers errors like a string that is given
//!   where a number is expected.
//! * A [`Validation`] reports all problems of an input.
//!
//! ```
//! use deser::Deserialize;
//! use deser_validate::{
//!     Check, Email, MaxLen, NonEmpty, Validated, Validation,
//! };
//!
//! #[derive(Deserialize)]
//! struct Signup {
//!     // must be valid, or the signup is invalid
//!     #[deser(as = Check<(NonEmpty, MaxLen<32>)>)]
//!     name: String,
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
//!     .map(|issue| {
//!         format!("{}: {}", issue.path().unwrap(), issue.message())
//!     })
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
//! Types that are always valid are newtypes which check themselves:
//!
//! ```
//! use deser::Deserialize;
//! use deser_validate::Check;
//!
//! #[derive(Deserialize)]
//! #[deser(transparent)]
//! pub struct Email(#[deser(as = Check<deser_validate::Email>)] String);
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
//! // a condition and a message
//! validator!(NonZero(port: &u16) => *port != 0, "must not be zero");
//!
//! // a function
//! fn check_host(host: &str) -> Result<(), String> {
//!     match host.contains(' ') {
//!         true => Err(format!("`{}` is not a host name", host)),
//!         false => Ok(()),
//!     }
//! }
//! validator!(HostName(host: &str) = check_host);
//!
//! #[derive(Deserialize)]
//! struct Server {
//!     #[deser(as = Check<HostName>)]
//!     host: String,
//!     #[deser(as = Check<NonZero>)]
//!     port: u16,
//! }
//! ```
//!
//! Validators of types with generics or lifetimes implement [`Validator`]
//! themselves, the macro does not support them.
//!
//! Validators report a [`Violation`] with a code and parameters for
//! programs and a message for humans.  Errors have the violation attached.
//!
//! # Naming Validators
//!
//! Validators are named for the property that valid values have, as a
//! noun or an adjective: `Email`, `Slug`, `NonEmpty`, `NonZero`,
//! `MaxLen<64>`.  They read as `Check<NonZero>` and do not need affixes like
//! `Valid`, `Is` or `Rules`.  Negations start with `Non` (like
//! [`NonZero`](std::num::NonZero)).  Validators of a whole type name the
//! rule they check (`OrderedBounds`, not `BoundsRules`), combinators name
//! their structure ([`Each`]).  The code of a violation is the name of the
//! validator in snake case (`max_len`, `non_zero`), for the validators of
//! this crate and the ones [`validator!`] creates.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

mod check;
mod macros;
mod report;
mod validated;
mod validator;
mod validators;

pub use self::check::Check;
#[doc(hidden)]
pub use self::macros::__private;
pub use self::macros::{IntoViolation, ValidationResult};
pub use self::report::{Issue, Outcome, Report, Validation};
pub use self::validated::Validated;
pub use self::validator::{Param, Validator, Violation};
pub use self::validators::{
    Each, Email, Integer, Len, Length, Max, MaxLen, Min, MinLen, NonEmpty, Range,
};

// the examples of the readme are tested
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
