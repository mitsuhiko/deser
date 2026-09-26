//! Error interface.
use std::any::{Any, TypeId};
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
    /// Reading or writing failed (see `deser::io`).  The IO error is
    /// the [`source`](std::error::Error::source) of the error.
    Io,
}

/// Additional information attached to an [`Error`].
///
/// Besides the location in the input, which is built into errors, layers
/// and other code can attach typed values to errors with
/// [`Error::with_attachment`] and retrieve them with
/// [`Error::attachment`].  An error holds at most one attachment per type.
/// For instance the `deser-path` crate attaches the path of the value an
/// error refers to.
///
/// Attachments can contribute to the [`Display`](fmt::Display) output of
/// the error with [`fmt_context`](Self::fmt_context).
///
/// ```
/// use std::fmt;
/// use deser::{Error, ErrorAttachment, ErrorKind};
///
/// #[derive(Debug)]
/// struct FileName(String);
///
/// impl ErrorAttachment for FileName {
///     fn fmt_context(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         write!(f, " in {}", self.0)
///     }
/// }
///
/// let err = Error::new(ErrorKind::Unexpected, "unexpected string")
///     .with_position(12, 2, 5)
///     .with_attachment(FileName("config.json".into()));
/// assert_eq!(err.attachment::<FileName>().unwrap().0, "config.json");
/// assert_eq!(
///     err.to_string(),
///     "Unexpected: unexpected string at line 2 column 5 in config.json"
/// );
/// ```
pub trait ErrorAttachment: Any + fmt::Debug + Send + Sync {
    /// Writes the attachment as part of the error message.
    ///
    /// The output is appended to the message and the location of the
    /// error, so it typically starts with a space.  By default attachments
    /// are not shown.
    fn fmt_context(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        Ok(())
    }
}

/// An error for deser.
///
/// Besides a kind and a message an error can carry context: the location
/// in the input it refers to (see [`offset`](Self::offset),
/// [`line`](Self::line) and [`column`](Self::column)) and typed
/// attachments (see [`ErrorAttachment`]).  The context is part of the
/// [`Display`](fmt::Display) output:
///
/// ```
/// use deser::{Error, ErrorKind};
///
/// let err = Error::new(ErrorKind::Unexpected, "unexpected string")
///     .with_position(12, 2, 5);
/// assert_eq!(
///     err.to_string(),
///     "Unexpected: unexpected string at line 2 column 5"
/// );
/// ```
///
/// Errors raised while deserializing a value (for instance by a
/// [`Sink`](crate::de::Sink)) get the context attached by the
/// [`DeserializeDriver`](crate::de::DeserializeDriver): the start of the
/// input range of the event (see [`State::input_range`](crate::State::input_range))
/// and the context of the types registered with
/// [`State::add_error_context`](crate::State::add_error_context).  Formats
/// resolve the offsets into lines and columns.
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
    offset: Option<usize>,
    // line and column (1-based)
    line_column: Option<(usize, usize)>,
    // in the order they were attached, at most one per type
    attachments: Vec<Attachment>,
    // `true` once the driver attached the context of the current event.
    has_context: bool,
}

#[derive(Debug)]
struct Attachment {
    // Invariant: the type of the value
    type_id: TypeId,
    value: Box<dyn ErrorAttachment>,
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
                offset: None,
                line_column: None,
                attachments: Vec::new(),
                has_context: false,
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

    /// Returns the message of the error (without context).
    pub fn message(&self) -> &str {
        &self.inner.msg
    }

    /// Sets the byte offset in the input the error refers to.
    ///
    /// A previously set line and column are discarded.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.inner.offset = Some(offset);
        self.inner.line_column = None;
        self
    }

    /// Sets the byte offset together with its line and column (1-based).
    pub fn with_position(mut self, offset: usize, line: usize, column: usize) -> Self {
        self.inner.offset = Some(offset);
        self.inner.line_column = Some((line, column));
        self
    }

    /// Resolves the offset into line and column.
    ///
    /// The source is the input the offset refers to.  Columns are counted
    /// in characters (bytes that are not UTF-8 continuation bytes).  If the
    /// error has no offset or already has a line and column, it's returned
    /// unchanged.  Text formats call this for the errors they return.
    ///
    /// ```
    /// use deser::{Error, ErrorKind};
    ///
    /// let err = Error::new(ErrorKind::Unexpected, "bad value")
    ///     .with_offset(7)
    ///     .resolve_position(b"[1,\n  x]");
    /// assert_eq!((err.line(), err.column()), (Some(2), Some(4)));
    /// ```
    pub fn resolve_position(mut self, source: &[u8]) -> Self {
        if let (Some(offset), None) = (self.inner.offset, self.inner.line_column) {
            let before = &source[..offset.min(source.len())];
            let line_start = before
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |x| x + 1);
            let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
            let column = before[line_start..]
                .iter()
                .filter(|&&b| b & 0xc0 != 0x80)
                .count()
                + 1;
            self.inner.line_column = Some((line, column));
        }
        self
    }

    /// Moves the position of the error by the position of the input it
    /// refers to.
    ///
    /// This is used for errors of inputs which are part of a larger input,
    /// the base is the position (offset, line and column) of the start of
    /// the part.
    #[cfg(feature = "io")]
    pub(crate) fn shift_position(mut self, offset: usize, line: usize, column: usize) -> Self {
        if let Some(ref mut error_offset) = self.inner.offset {
            *error_offset += offset;
        }
        if let Some((ref mut error_line, ref mut error_column)) = self.inner.line_column {
            if *error_line == 1 {
                *error_column += column - 1;
            }
            *error_line += line - 1;
        }
        self
    }

    /// Returns the byte offset in the input the error refers to.
    pub fn offset(&self) -> Option<usize> {
        self.inner.offset
    }

    /// Returns the line (1-based) the error refers to.
    pub fn line(&self) -> Option<usize> {
        self.inner.line_column.map(|x| x.0)
    }

    /// Returns the column (1-based, in characters) the error refers to.
    pub fn column(&self) -> Option<usize> {
        self.inner.line_column.map(|x| x.1)
    }

    /// Attaches a value to the error.
    ///
    /// An attachment of the same type is replaced but keeps its position
    /// in the [`Display`](fmt::Display) output.  See [`ErrorAttachment`].
    pub fn with_attachment<T: ErrorAttachment>(mut self, value: T) -> Self {
        let type_id = TypeId::of::<T>();
        let value = Box::new(value);
        match self
            .inner
            .attachments
            .iter_mut()
            .find(|x| x.type_id == type_id)
        {
            Some(attachment) => attachment.value = value,
            None => self.inner.attachments.push(Attachment { type_id, value }),
        }
        self
    }

    /// Returns the attachment of the given type.
    pub fn attachment<T: ErrorAttachment>(&self) -> Option<&T> {
        let type_id = TypeId::of::<T>();
        let attachment = self
            .inner
            .attachments
            .iter()
            .find(|x| x.type_id == type_id)?;
        (&*attachment.value as &dyn Any).downcast_ref()
    }

    /// Returns the attachment of the given type mutably.
    pub fn attachment_mut<T: ErrorAttachment>(&mut self) -> Option<&mut T> {
        let type_id = TypeId::of::<T>();
        let attachment = self
            .inner
            .attachments
            .iter_mut()
            .find(|x| x.type_id == type_id)?;
        (&mut *attachment.value as &mut dyn Any).downcast_mut()
    }

    /// Iterates over the attachments in the order they were attached.
    pub fn attachments(&self) -> impl Iterator<Item = &dyn ErrorAttachment> {
        self.inner.attachments.iter().map(|x| &*x.value)
    }

    /// Returns `true` if the context of an event was attached.
    pub(crate) fn has_context(&self) -> bool {
        self.inner.has_context
    }

    /// Marks the context of an event as attached.
    pub(crate) fn set_has_context(&mut self) {
        self.inner.has_context = true;
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Error");
        s.field("kind", &self.inner.kind)
            .field("msg", &self.inner.msg);
        if let Some(offset) = self.inner.offset {
            s.field("offset", &offset);
        }
        if let Some((line, column)) = self.inner.line_column {
            s.field("line", &line).field("column", &column);
        }
        if !self.inner.attachments.is_empty() {
            s.field("attachments", &DebugAttachments(&self.inner.attachments));
        }
        s.field("source", &self.inner.source).finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.inner.kind, self.inner.msg)?;
        match (self.inner.line_column, self.inner.offset) {
            (Some((line, column)), _) => write!(f, " at line {} column {}", line, column)?,
            (None, Some(offset)) => write!(f, " at offset {}", offset)?,
            (None, None) => {}
        }
        for attachment in self.inner.attachments.iter() {
            attachment.value.fmt_context(f)?;
        }
        Ok(())
    }
}

struct DebugAttachments<'a>(&'a [Attachment]);

impl fmt::Debug for DebugAttachments<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|x| &x.value))
            .finish()
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::new(ErrorKind::Io, err.to_string()).with_source(err)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source.as_ref().map(|err| err.as_ref() as _)
    }
}
