//! Error interface.
use std::borrow::Cow;
use std::fmt;

/// Describes the kind of error.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum ErrorKind {
    UnsupportedType,
    Unexpected,
    MissingField,
    OutOfRange,
    WrongLength,
}

/// An error for deser.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    msg: Cow<'static, str>,
}

impl Error {
    /// Creates a new error.
    pub fn new<M: Into<Cow<'static, str>>>(kind: ErrorKind, msg: M) -> Error {
        Error {
            kind,
            msg: msg.into(),
        }
    }

    /// Returns the kind of the error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.msg)
    }
}

impl std::error::Error for Error {}
