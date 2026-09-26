//! Parsing of lexical atoms.
//!
//! See [`Atom::Lexical`](crate::Atom::Lexical) for the rules.
use std::fmt::Write;
use std::num::{IntErrorKind, ParseIntError};

use crate::error::{Error, ErrorKind};

/// The longest part of a value that is included in error messages.
const MAX_QUOTED: usize = 64;

/// Parses the lexical form of a boolean.
///
/// The spellings of booleans in query strings, environment variables and
/// command lines are accepted, ignoring ASCII case.
pub(crate) fn parse_bool(value: &str) -> Result<bool, Error> {
    const TRUE: [&str; 4] = ["true", "yes", "on", "1"];
    const FALSE: [&str; 4] = ["false", "no", "off", "0"];
    if TRUE.iter().any(|x| x.eq_ignore_ascii_case(value)) {
        Ok(true)
    } else if FALSE.iter().any(|x| x.eq_ignore_ascii_case(value)) {
        Ok(false)
    } else {
        Err(invalid(
            value,
            "bool (true, yes, on, 1, false, no, off or 0)",
        ))
    }
}

/// Converts the error of parsing an integer.
#[cold]
pub(crate) fn int_error(value: &str, err: ParseIntError, expecting: &str) -> Error {
    match err.kind() {
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => {
            Error::new(ErrorKind::OutOfRange, "value out of range for type")
        }
        _ => invalid(value, expecting),
    }
}

/// Creates the error for a lexical atom that cannot be parsed.
#[cold]
pub(crate) fn invalid(value: &str, expecting: &str) -> Error {
    let mut msg = String::from("invalid value ");
    match value.char_indices().nth(MAX_QUOTED) {
        Some((end, _)) => write!(msg, "{:?}...", &value[..end]).unwrap(),
        None => write!(msg, "{:?}", value).unwrap(),
    }
    write!(msg, ", expected {}", expecting).unwrap();
    Error::new(ErrorKind::Unexpected, msg)
}

#[test]
fn test_parse_bool() {
    for value in ["true", "TRUE", "Yes", "on", "1"] {
        assert!(parse_bool(value).unwrap(), "{}", value);
    }
    for value in ["false", "False", "NO", "off", "0"] {
        assert!(!parse_bool(value).unwrap(), "{}", value);
    }
    for value in ["", "2", "y", "n", "t", "truee", " true"] {
        assert!(parse_bool(value).is_err(), "{}", value);
    }
}

#[test]
fn test_invalid_truncates() {
    let err = invalid(&"x".repeat(100), "u32");
    assert_eq!(
        err.message(),
        format!("invalid value {:?}..., expected u32", "x".repeat(64))
    );
}
