//! Reporting all problems of an input.
use std::fmt;
use std::sync::{Arc, Mutex};

use deser_core::de::DeserializeDriver;
use deser_core::{Error, ErrorKind, State};
use deser_path::{Path, PathLayer};

use crate::Violation;

/// A problem of the input.
///
/// This is a summary of an [`Error`] that can be cloned and stored: its
/// kind, message, location and path and the [`Violation`] if a validator
/// rejected the value.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    kind: ErrorKind,
    message: String,
    path: Option<String>,
    offset: Option<usize>,
    line_column: Option<(usize, usize)>,
    violation: Option<Violation>,
}

impl Issue {
    /// Creates the issue of an error.
    ///
    /// For errors that hold multiple errors (see [`Error::errors`]), this
    /// is the issue of the first one, see [`Report::from_error`] for all of
    /// them.
    pub fn from_error(err: &Error) -> Issue {
        Issue {
            kind: err.kind(),
            message: err.message().to_string(),
            path: err.attachment::<Path>().map(|path| path.to_string()),
            offset: err.offset(),
            line_column: err.line().zip(err.column()),
            violation: err.attachment::<Violation>().cloned(),
        }
    }

    /// Returns the kind of the error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the path of the value (for instance `servers[1].port`).
    ///
    /// The path of the root value is the empty string.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Returns the byte offset in the input.
    pub fn offset(&self) -> Option<usize> {
        self.offset
    }

    /// Returns the line (1-based).
    pub fn line(&self) -> Option<usize> {
        self.line_column.map(|x| x.0)
    }

    /// Returns the column (1-based, in characters).
    pub fn column(&self) -> Option<usize> {
        self.line_column.map(|x| x.1)
    }

    /// Returns the violation if a validator rejected the value.
    pub fn violation(&self) -> Option<&Violation> {
        self.violation.as_ref()
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.path {
            Some(ref path) if !path.is_empty() => write!(f, "{}: {}", path, self.message)?,
            _ => f.write_str(&self.message)?,
        }
        match (self.line_column, self.offset) {
            (Some((line, column)), _) => write!(f, " (at line {} column {})", line, column),
            (None, Some(offset)) => write!(f, " (at offset {})", offset),
            (None, None) => Ok(()),
        }
    }
}

/// All problems of an input.
///
/// A report is the result of a [`Validation`], it lists the issues in the
/// order they appear in the input.  Its display output lists them one per
/// line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    issues: Vec<Issue>,
}

impl Report {
    /// Creates an empty report.
    pub fn new() -> Report {
        Report::default()
    }

    /// Creates the report of the errors an error holds.
    pub fn from_error(err: &Error) -> Report {
        Report {
            issues: err.errors().map(Issue::from_error).collect(),
        }
    }

    /// Returns `true` if there are no issues.
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns the number of issues.
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// Returns the issues.
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// Iterates over the issues.
    pub fn iter(&self) -> std::slice::Iter<'_, Issue> {
        self.issues.iter()
    }

    /// Adds an issue.
    pub fn push(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    /// Resolves the offsets of the issues without lines and columns.
    ///
    /// The source is the input the offsets refer to.  Issues of values
    /// that kept their errors (see [`Validated`](crate::Validated)) only
    /// have lines and columns if the format provides the source (see
    /// [`deser_location`]).  This resolves them otherwise.
    pub fn resolve_positions(&mut self, source: &[u8]) {
        for issue in self.issues.iter_mut() {
            if let (Some(offset), None) = (issue.offset, issue.line_column) {
                let pos = deser_core::Position::of(source, offset);
                issue.line_column = Some((pos.line, pos.column));
            }
        }
    }
}

impl<'a> IntoIterator for &'a Report {
    type Item = &'a Issue;
    type IntoIter = std::slice::Iter<'a, Issue>;

    fn into_iter(self) -> Self::IntoIter {
        self.issues.iter()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, issue) in self.issues.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            fmt::Display::fmt(issue, f)?;
        }
        Ok(())
    }
}

impl std::error::Error for Report {}

/// Where the errors that values keep are reported to (in the state).
#[derive(Debug, Default)]
pub(crate) struct ReportHandle(Option<Arc<Mutex<Vec<Issue>>>>);

impl ReportHandle {
    /// Reports the errors of an error if a validation runs.
    pub(crate) fn report(err: &Error, state: &State) {
        if let Some(ReportHandle(Some(issues))) = state.get::<ReportHandle>() {
            let mut issues = issues.lock().unwrap_or_else(|err| err.into_inner());
            issues.extend(err.errors().map(Issue::from_error));
        }
    }
}

/// Finds all problems of an input.
///
/// A validation is set up on a deserialization (with the `setup` function
/// that formats accept, see
/// [`Deserializer::deserialize_with`](deser_core::de::Deserializer::deserialize_with)).
/// It collects errors (see [`State::set_collect_errors`]) and tracks paths
/// (see [`PathLayer`]).  Once the deserialization is done,
/// [`finish`](Self::finish) returns the value together with a [`Report`]
/// of all problems: the errors that failed the deserialization and the
/// errors that [`Validated`](crate::Validated) values kept.
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::{Email, Range, Validated, Validation};
///
/// #[derive(Deserialize)]
/// struct Signup {
///     email: Validated<String, Email>,
///     age: Validated<u8, Range<13, 130>>,
///     name: String,
/// }
///
/// let input = r#"{"email": "nope", "age": 7}"#;
/// let validation = Validation::new();
/// let rv = deser_json::Deserializer::from_str(input)
///     .deserialize_with::<Signup, _>(|driver| validation.setup(driver));
/// let outcome = validation.finish(rv);
/// assert!(outcome.value.is_none());
/// assert_eq!(
///     outcome.report.to_string(),
///     "email: invalid value: must be an email address (at offset 10)\n\
///      age: invalid value: must be between 13 and 130 (at offset 25)\n\
///      missing field `name` (at line 1 column 27)"
/// );
/// ```
#[derive(Debug, Default)]
pub struct Validation {
    issues: Arc<Mutex<Vec<Issue>>>,
    max_errors: Option<usize>,
}

/// The result of a [`Validation`].
#[derive(Debug)]
pub struct Outcome<T> {
    /// The value, if it could be deserialized.
    ///
    /// The value can hold invalid values (see
    /// [`Validated`](crate::Validated)), the report lists them.
    pub value: Option<T>,
    /// The problems of the input.
    pub report: Report,
}

impl<T> Outcome<T> {
    /// Returns `true` if the input has no problems.
    pub fn is_valid(&self) -> bool {
        self.value.is_some() && self.report.is_empty()
    }

    /// Returns the value if the input has no problems, the report
    /// otherwise.
    pub fn into_result(self) -> Result<T, Report> {
        match self.value {
            Some(value) if self.report.is_empty() => Ok(value),
            _ => Err(self.report),
        }
    }
}

impl Validation {
    /// Creates a validation.
    pub fn new() -> Validation {
        Validation::default()
    }

    /// Limits the number of errors that are collected.
    ///
    /// See [`State::set_max_errors`].  This limits the errors that fail
    /// the deserialization, not the errors that values keep.
    pub fn set_max_errors(&mut self, max: usize) {
        self.max_errors = Some(max);
    }

    /// Returns the limit of errors (see [`set_max_errors`](Self::set_max_errors)).
    pub fn max_errors(&self) -> Option<usize> {
        self.max_errors
    }

    /// Sets up a deserialization.
    pub fn setup(&self, driver: &mut DeserializeDriver<'_, '_>) {
        driver.push_layer(PathLayer::new());
        let state = driver.state_mut();
        state.set_collect_errors(true);
        if let Some(max) = self.max_errors {
            state.set_max_errors(max);
        }
        state.get_mut::<ReportHandle>().0 = Some(self.issues.clone());
    }

    /// Finishes the validation with the result of the deserialization.
    pub fn finish<T>(&self, rv: Result<T, Error>) -> Outcome<T> {
        let issues = {
            let mut issues = self.issues.lock().unwrap_or_else(|err| err.into_inner());
            std::mem::take(&mut *issues)
        };
        let mut report = Report { issues };
        let value = match rv {
            Ok(value) => Some(value),
            Err(err) => {
                report.issues.extend(err.errors().map(Issue::from_error));
                None
            }
        };
        // in the order of the input, issues without a location last
        report
            .issues
            .sort_by_key(|issue| issue.offset.unwrap_or(usize::MAX));
        Outcome { value, report }
    }
}
