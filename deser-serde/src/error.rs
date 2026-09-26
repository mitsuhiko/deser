use std::fmt;

use deser::ErrorKind;

/// The error type used with serde.
///
/// Besides wrapping deser errors this has variants for the internal
/// signals which need to travel through serde code as errors.  They do not
/// allocate.
pub(crate) struct Error(Repr);

enum Repr {
    Deser(deser::Error),
    /// A missing value was requested to be deserialized as not optional.
    Missing,
    /// The serialization or deserialization was aborted.
    Cancelled,
}

impl Error {
    pub(crate) fn new<M: Into<std::borrow::Cow<'static, str>>>(kind: ErrorKind, msg: M) -> Error {
        Error(Repr::Deser(deser::Error::new(kind, msg)))
    }

    pub(crate) fn missing() -> Error {
        Error(Repr::Missing)
    }

    pub(crate) fn cancelled() -> Error {
        Error(Repr::Cancelled)
    }

    pub(crate) fn into_deser(self) -> deser::Error {
        match self.0 {
            Repr::Deser(err) => err,
            Repr::Missing => deser::Error::new(ErrorKind::MissingField, "missing value"),
            Repr::Cancelled => deser::Error::new(ErrorKind::Unexpected, "serde value was aborted"),
        }
    }
}

impl From<deser::Error> for Error {
    fn from(err: deser::Error) -> Error {
        Error(Repr::Deser(err))
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Repr::Deser(ref err) => fmt::Debug::fmt(err, f),
            Repr::Missing => f.write_str("Missing"),
            Repr::Cancelled => f.write_str("Cancelled"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Repr::Deser(ref err) => fmt::Display::fmt(err, f),
            Repr::Missing => f.write_str("missing value"),
            Repr::Cancelled => f.write_str("serde value was aborted"),
        }
    }
}

impl std::error::Error for Error {}

impl serde::de::Error for Error {
    #[cold]
    fn custom<T: fmt::Display>(msg: T) -> Error {
        Error::new(ErrorKind::Unexpected, msg.to_string())
    }

    #[cold]
    fn invalid_length(len: usize, exp: &dyn serde::de::Expected) -> Error {
        Error::new(
            ErrorKind::WrongLength,
            format!("invalid length {}, expected {}", len, exp),
        )
    }

    #[cold]
    fn missing_field(field: &'static str) -> Error {
        Error::new(
            ErrorKind::MissingField,
            format!("missing field `{}`", field),
        )
    }
}

impl serde::ser::Error for Error {
    #[cold]
    fn custom<T: fmt::Display>(msg: T) -> Error {
        Error::new(ErrorKind::Unexpected, msg.to_string())
    }
}
