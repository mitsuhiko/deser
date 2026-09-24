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
    EndOfFile,
}

/// An error for deser.
pub struct Error {
    // boxed so that results stay small.  Errors are rare but results are
    // passed around for every single value.
    inner: Box<ErrorInner>,
}

#[derive(Debug)]
struct ErrorInner {
    kind: ErrorKind,
    msg: Cow<'static, str>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl Error {
    /// Creates a new error.
    #[cold]
    pub fn new<M: Into<Cow<'static, str>>>(kind: ErrorKind, msg: M) -> Error {
        Error {
            inner: Box::new(ErrorInner {
                kind,
                msg: msg.into(),
                source: None,
            }),
        }
    }

    /// Attaches another error as source to this error.
    pub fn with_source<E: std::error::Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.inner.source = Some(Box::new(source));
        self
    }

    /// Returns the kind of the error.
    pub fn kind(&self) -> ErrorKind {
        self.inner.kind
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.inner.kind)
            .field("msg", &self.inner.msg)
            .field("source", &self.inner.source)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.inner.kind, self.inner.msg)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source.as_ref().map(|err| err.as_ref() as _)
    }
}
