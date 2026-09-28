//! Error interface.
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::any::{Any, TypeId};
use core::fmt;

use crate::Position;

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
///
/// # Multiple Errors
///
/// An error can hold multiple errors, for instance if deserialization
/// continued after an error to report all problems of the input at once
/// (see [`State::set_collect_errors`](crate::State::set_collect_errors)).
/// The accessors ([`kind`](Self::kind), [`message`](Self::message), the
/// location and the attachments) refer to the first of them, all of them
/// are iterated with [`errors`](Self::errors).  The
/// [`Display`](fmt::Display) output mentions how many more errors there
/// are, with the alternate flag (`{:#}`) it lists all of them, one per
/// line:
///
/// ```
/// use deser::{Error, ErrorKind};
///
/// let err = Error::from_errors([
///     Error::new(ErrorKind::MissingField, "missing field `a`")
///         .with_offset(0),
///     Error::new(ErrorKind::Unexpected, "unexpected string")
///         .with_offset(9),
/// ])
/// .unwrap()
/// .resolve_position(b"{\n  \"b\": \"x\"}");
/// assert_eq!(err.error_count(), 2);
/// assert_eq!(err.kind(), ErrorKind::MissingField);
/// assert_eq!(
///     err.to_string(),
///     "MissingField: missing field `a` at line 1 column 1 \
///      (and 1 more error)"
/// );
/// assert_eq!(
///     format!("{:#}", err),
///     "MissingField: missing field `a` at line 1 column 1\n\
///      Unexpected: unexpected string at line 2 column 8"
/// );
/// ```
pub struct Error {
    // boxed so that results stay small.  Errors are rare but results are
    // passed around for every single value.
    inner: Box<ErrorInner>,
}

enum ErrorInner {
    Single(ErrorData),
    // at least two errors, all of them are single errors.  The first one
    // is the error the accessors refer to.
    Multiple(Vec<Error>),
}

#[derive(Debug)]
struct ErrorData {
    kind: ErrorKind,
    msg: Cow<'static, str>,
    source: Option<Box<dyn core::error::Error + Send + Sync>>,
    offset: Option<usize>,
    // line and column (1-based)
    line_column: Option<(usize, usize)>,
    // in the order they were attached, at most one per type
    attachments: Vec<Attachment>,
    // `true` once the driver attached the context of the current event.
    has_context: bool,
    // `true` once the error was collected (see `CollectedErrors`)
    collected: bool,
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
            inner: Box::new(ErrorInner::Single(ErrorData {
                kind,
                msg: msg.into(),
                source: None,
                offset: None,
                line_column: None,
                attachments: Vec::new(),
                has_context: false,
                collected: false,
            })),
        }
    }

    /// Combines errors into one.
    ///
    /// Errors that hold multiple errors are flattened (see
    /// [`push_error`](Self::push_error)).  Returns `None` if there are no
    /// errors.
    pub fn from_errors<I: IntoIterator<Item = Error>>(errors: I) -> Option<Error> {
        let mut errors = errors.into_iter();
        let mut rv = errors.next()?;
        for err in errors {
            rv.push_error(err);
        }
        Some(rv)
    }

    /// Adds an error to this error.
    ///
    /// If the error that is added holds multiple errors, they are added
    /// individually: errors do not nest (see [`errors`](Self::errors)).
    pub fn push_error(&mut self, err: Error) {
        let errors = self.make_multiple();
        match *err.inner {
            ErrorInner::Single(data) => errors.push(Error {
                inner: Box::new(ErrorInner::Single(data)),
            }),
            ErrorInner::Multiple(others) => errors.extend(others),
        }
    }

    /// Turns the error into one that holds multiple errors.
    fn make_multiple(&mut self) -> &mut Vec<Error> {
        if let ErrorInner::Single(_) = *self.inner {
            let first = core::mem::replace(&mut *self.inner, ErrorInner::Multiple(Vec::new()));
            if let ErrorInner::Multiple(ref mut errors) = *self.inner {
                errors.push(Error {
                    inner: Box::new(first),
                });
            }
        }
        match *self.inner {
            ErrorInner::Multiple(ref mut errors) => errors,
            ErrorInner::Single(_) => unreachable!(),
        }
    }

    /// Iterates over the errors this error holds.
    ///
    /// For an error that holds a single error, this is the error itself.
    /// The errors that are returned hold a single error each.
    pub fn errors(&self) -> impl Iterator<Item = &Error> {
        match *self.inner {
            ErrorInner::Single(_) => core::slice::from_ref(self).iter(),
            ErrorInner::Multiple(ref errors) => errors.iter(),
        }
    }

    /// Returns the number of errors this error holds.
    pub fn error_count(&self) -> usize {
        match *self.inner {
            ErrorInner::Single(_) => 1,
            ErrorInner::Multiple(ref errors) => errors.len(),
        }
    }

    /// Returns the data of the (first) error.
    fn data(&self) -> &ErrorData {
        match *self.inner {
            ErrorInner::Single(ref data) => data,
            ErrorInner::Multiple(ref errors) => errors[0].data(),
        }
    }

    /// Returns the data of the (first) error mutably.
    fn data_mut(&mut self) -> &mut ErrorData {
        match *self.inner {
            ErrorInner::Single(ref mut data) => data,
            ErrorInner::Multiple(ref mut errors) => errors[0].data_mut(),
        }
    }

    /// Applies a function to every error this error holds.
    pub(crate) fn map_each(mut self, mut f: impl FnMut(Error) -> Error) -> Error {
        if let ErrorInner::Multiple(ref mut errors) = *self.inner {
            for err in errors.iter_mut() {
                let taken = core::mem::replace(err, Error::new(ErrorKind::Unexpected, ""));
                *err = f(taken);
            }
            self
        } else {
            f(self)
        }
    }

    /// Returns the number of errors this error holds that were not
    /// collected yet.
    pub(crate) fn uncollected_count(&self) -> usize {
        self.errors().filter(|err| !err.data().collected).count()
    }

    /// Marks all errors this error holds as collected.
    pub(crate) fn mark_collected(mut self) -> Error {
        self = self.map_each(|mut err| {
            err.data_mut().collected = true;
            err
        });
        self
    }

    /// Attaches another error as source to this error.
    pub fn with_source<E: core::error::Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.data_mut().source = Some(Box::new(source));
        self
    }

    /// Returns the kind of the error.
    pub fn kind(&self) -> ErrorKind {
        self.data().kind
    }

    /// Returns the message of the error (without context).
    pub fn message(&self) -> &str {
        &self.data().msg
    }

    /// Sets the byte offset in the input the error refers to.
    ///
    /// A previously set line and column are discarded.
    pub fn with_offset(mut self, offset: usize) -> Self {
        let data = self.data_mut();
        data.offset = Some(offset);
        data.line_column = None;
        self
    }

    /// Sets the byte offset together with its line and column (1-based).
    pub fn with_position(mut self, offset: usize, line: usize, column: usize) -> Self {
        let data = self.data_mut();
        data.offset = Some(offset);
        data.line_column = Some((line, column));
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
    ///
    /// The positions of further errors (see [`errors`](Self::errors)) are
    /// resolved as well.
    pub fn resolve_position(self, source: &[u8]) -> Self {
        self.map_each(|mut err| {
            let data = err.data_mut();
            if let (Some(offset), None) = (data.offset, data.line_column) {
                let pos = Position::of(source, offset);
                data.line_column = Some((pos.line, pos.column));
            }
            err
        })
    }

    /// Moves the position of the error by the position of the input it
    /// refers to.
    ///
    /// This is used for errors of inputs which are part of a larger input,
    /// the base is the position of the start of the part.
    pub(crate) fn shift_position(self, base: Position) -> Self {
        self.map_each(|mut err| {
            let data = err.data_mut();
            if let Some(ref mut error_offset) = data.offset {
                *error_offset += base.offset;
            }
            if let Some((ref mut error_line, ref mut error_column)) = data.line_column {
                if *error_line == 1 {
                    *error_column += base.column - 1;
                }
                *error_line += base.line - 1;
            }
            err
        })
    }

    /// Returns the byte offset in the input the error refers to.
    pub fn offset(&self) -> Option<usize> {
        self.data().offset
    }

    /// Returns the line (1-based) the error refers to.
    pub fn line(&self) -> Option<usize> {
        self.data().line_column.map(|x| x.0)
    }

    /// Returns the column (1-based, in characters) the error refers to.
    pub fn column(&self) -> Option<usize> {
        self.data().line_column.map(|x| x.1)
    }

    /// Attaches a value to the error.
    ///
    /// An attachment of the same type is replaced but keeps its position
    /// in the [`Display`](fmt::Display) output.  See [`ErrorAttachment`].
    pub fn with_attachment<T: ErrorAttachment>(mut self, value: T) -> Self {
        let type_id = TypeId::of::<T>();
        let value = Box::new(value);
        let attachments = &mut self.data_mut().attachments;
        match attachments.iter_mut().find(|x| x.type_id == type_id) {
            Some(attachment) => attachment.value = value,
            None => attachments.push(Attachment { type_id, value }),
        }
        self
    }

    /// Returns the attachment of the given type.
    pub fn attachment<T: ErrorAttachment>(&self) -> Option<&T> {
        let type_id = TypeId::of::<T>();
        let attachment = self
            .data()
            .attachments
            .iter()
            .find(|x| x.type_id == type_id)?;
        (&*attachment.value as &dyn Any).downcast_ref()
    }

    /// Returns the attachment of the given type mutably.
    pub fn attachment_mut<T: ErrorAttachment>(&mut self) -> Option<&mut T> {
        let type_id = TypeId::of::<T>();
        let attachment = self
            .data_mut()
            .attachments
            .iter_mut()
            .find(|x| x.type_id == type_id)?;
        (&mut *attachment.value as &mut dyn Any).downcast_mut()
    }

    /// Iterates over the attachments in the order they were attached.
    pub fn attachments(&self) -> impl Iterator<Item = &dyn ErrorAttachment> {
        self.data().attachments.iter().map(|x| &*x.value)
    }

    /// Returns `true` if the context of an event was attached.
    pub(crate) fn has_context(&self) -> bool {
        self.data().has_context
    }

    /// Marks the context of an event as attached.
    pub(crate) fn set_has_context(&mut self) {
        self.data_mut().has_context = true;
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data = match *self.inner {
            ErrorInner::Single(ref data) => data,
            ErrorInner::Multiple(ref errors) => {
                return f.debug_tuple("Errors").field(errors).finish();
            }
        };
        let mut s = f.debug_struct("Error");
        s.field("kind", &data.kind).field("msg", &data.msg);
        if let Some(offset) = data.offset {
            s.field("offset", &offset);
        }
        if let Some((line, column)) = data.line_column {
            s.field("line", &line).field("column", &column);
        }
        if !data.attachments.is_empty() {
            s.field("attachments", &DebugAttachments(&data.attachments));
        }
        s.field("source", &data.source).finish()
    }
}

impl ErrorData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.msg)?;
        match (self.line_column, self.offset) {
            (Some((line, column)), _) => write!(f, " at line {} column {}", line, column)?,
            (None, Some(offset)) => write!(f, " at offset {}", offset)?,
            (None, None) => {}
        }
        for attachment in self.attachments.iter() {
            attachment.value.fmt_context(f)?;
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let errors = match *self.inner {
            ErrorInner::Single(ref data) => return data.fmt(f),
            ErrorInner::Multiple(ref errors) => errors,
        };
        errors[0].data().fmt(f)?;
        if f.alternate() {
            for err in &errors[1..] {
                writeln!(f)?;
                err.data().fmt(f)?;
            }
        } else if errors.len() == 2 {
            write!(f, " (and 1 more error)")?;
        } else {
            write!(f, " (and {} more errors)", errors.len() - 1)?;
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

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::new(ErrorKind::Io, err.to_string()).with_source(err)
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.data().source.as_ref().map(|err| err.as_ref() as _)
    }
}

/// Creates an error that is thrown away.
///
/// While errors are discarded (see `State::discard_errors`) the common
/// errors are created with this instead of building a message nobody reads.
#[cold]
#[inline(never)]
pub(crate) fn discarded_error(kind: ErrorKind) -> Error {
    Error::new(kind, "discarded error")
}

/// Creates the error for a value that failed to convert or validate.
#[cold]
pub(crate) fn conversion_error<E: fmt::Display>(err: E) -> Error {
    Error::new(ErrorKind::Unexpected, format!("invalid value: {}", err))
}

/// Creates the error for an unknown variant.
///
/// `tag` is the name that was given (if it can be a name), `type_name` the
/// name of the enum and `names` are the names of the variants.
#[cold]
pub fn unknown_variant(tag: Option<&str>, type_name: &str, names: &[&str]) -> Error {
    let mut msg = String::from("unknown variant");
    if let Some(tag) = tag {
        msg.push_str(" `");
        msg.push_str(tag);
        msg.push('`');
    }
    msg.push_str(" of ");
    msg.push_str(type_name);
    push_expected(&mut msg, names, "variants");
    Error::new(ErrorKind::Unexpected, msg)
}

/// Appends the expected names to an error message.
///
/// `what` is what the names are, for the message if there are none.
pub(crate) fn push_expected(msg: &mut String, names: &[&str], what: &str) {
    match names {
        [] => {
            msg.push_str(", there are no ");
            msg.push_str(what);
        }
        [name] => {
            msg.push_str(", expected `");
            msg.push_str(name);
            msg.push('`');
        }
        [first, second] => {
            msg.push_str(", expected `");
            msg.push_str(first);
            msg.push_str("` or `");
            msg.push_str(second);
            msg.push('`');
        }
        names => {
            msg.push_str(", expected one of ");
            for (idx, name) in names.iter().enumerate() {
                if idx > 0 {
                    msg.push_str(", ");
                }
                msg.push('`');
                msg.push_str(name);
                msg.push('`');
            }
        }
    }
}
