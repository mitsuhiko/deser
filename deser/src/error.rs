//! Error interface.
use std::borrow::Cow;
use std::fmt;

/// Describes the kind of error.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum ErrorKind {
    InvalidValue,
    UnsupportedType,
    Unexpected,
    MissingField,
    OutOfRange,
    WrongLength,
    EndOfFile,
}

/// An error for deser.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    msg: Cow<'static, str>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl Error {
    /// Creates a new error.
    pub fn new<M: Into<Cow<'static, str>>>(kind: ErrorKind, msg: M) -> Error {
        Error {
            kind,
            msg: msg.into(),
            source: None,
        }
    }

    /// Attaches another error as source to this error.
    pub fn with_source<E: std::error::Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.source = Some(Box::new(source));
        self
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

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|err| err.as_ref() as _)
    }
}
