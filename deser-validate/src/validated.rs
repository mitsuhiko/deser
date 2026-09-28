//! A value together with the errors it had.
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{Chunk, Describe, Serialize};
use deser_core::{Atom, ContainerShape, Error, ErrorKind, State};
use deser_location::Locations;

use crate::Validator;
use crate::report::ReportHandle;

/// A value that might be invalid, together with its errors.
///
/// Deserializing a `Validated` never fails: the errors of the value are
/// kept in it instead of failing the deserialization of the value it's in.
/// That covers the errors of the value itself (for instance a string where
/// a number is expected), of the values nested in it and the violation of
/// the validator `V` (none by default).  Within the value, all errors are
/// collected (see [`State::set_collect_errors`]): the error holds all
/// problems of the value, not just the first one.  Only the errors of the
/// data format and of layers (for instance
/// [`Limits`](deser_core::de::Limits)) still fail the deserialization, and
/// the error that exceeds the limit of errors (see
/// [`State::set_max_errors`]).
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::{Email, Validated};
///
/// #[derive(Deserialize)]
/// struct Signup {
///     email: Validated<String, Email>,
///     age: Validated<u8>,
/// }
///
/// let signup: Signup = deser_json::from_str(r#"{"email": "nope", "age": "x"}"#).unwrap();
/// assert!(!signup.email.is_valid());
/// assert_eq!(signup.email.unchecked_value().unwrap(), "nope");
/// assert_eq!(
///     signup.email.error().unwrap().to_string(),
///     "Unexpected: invalid value: must be an email address at offset 10"
/// );
/// assert_eq!(signup.age.value(), None);
/// ```
///
/// The errors have the context of the value attached, like errors that
/// are returned (for instance the path with a
/// [`PathLayer`](deser_path::PathLayer)).  If the format provides the
/// source (see [`deser_location`]), their lines and columns are resolved.
/// If a [`Validation`](crate::Validation) runs, they are reported to it
/// too.
///
/// While an untagged enum tries its variants, errors are not kept: a
/// variant with an invalid `Validated` value does not match.
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::Validated;
///
/// #[derive(Deserialize)]
/// struct Address {
///     street: String,
///     zip: u32,
/// }
///
/// #[derive(Deserialize)]
/// struct Order {
///     shipping: Validated<Address>,
/// }
///
/// let order: Order = deser_json::from_str(r#"{"shipping": {"zip": "x"}}"#).unwrap();
/// let err = order.shipping.error().unwrap();
/// let errors: Vec<_> = err.errors().map(|err| err.message()).collect();
/// assert_eq!(errors, ["unexpected string, expected u32", "missing field `street`"]);
/// ```
///
/// When serialized, the value is serialized (also if it's invalid), a
/// value that could not be deserialized is serialized as null.
pub struct Validated<T, V = ()> {
    value: Option<T>,
    error: Option<Error>,
    _validator: PhantomData<fn() -> V>,
}

impl<T, V: Validator<T>> Validated<T, V> {
    /// Validates a value.
    pub fn new(value: T) -> Validated<T, V> {
        let error = V::validate(&value).err().map(|x| x.into_error());
        Validated {
            value: Some(value),
            error,
            _validator: PhantomData,
        }
    }
}

impl<T, V> Validated<T, V> {
    /// Creates an invalid value from an error.
    pub fn from_error(error: Error) -> Validated<T, V> {
        Validated {
            value: None,
            error: Some(error),
            _validator: PhantomData,
        }
    }

    /// Returns `true` if the value is valid.
    pub fn is_valid(&self) -> bool {
        self.error.is_none()
    }

    /// Returns the value if it's valid.
    pub fn value(&self) -> Option<&T> {
        match self.error {
            None => self.value.as_ref(),
            Some(_) => None,
        }
    }

    /// Returns the value, also if the validator rejected it.
    ///
    /// Returns `None` if the value could not be deserialized.
    pub fn unchecked_value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Returns the error if the value is invalid.
    ///
    /// The error holds all errors of the value (see [`Error::errors`]).
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }

    /// Returns the value if it's valid and the error otherwise.
    pub fn into_result(self) -> Result<T, Error> {
        match (self.value, self.error) {
            (Some(value), None) => Ok(value),
            (_, Some(err)) => Err(err),
            (None, None) => Err(Error::new(ErrorKind::Unexpected, "missing value")),
        }
    }
}

impl<T: fmt::Debug, V> fmt::Debug for Validated<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.error {
            None => fmt::Debug::fmt(&self.value, f),
            Some(ref err) => f
                .debug_struct("Invalid")
                .field("value", &self.value)
                .field("error", &format_args!("{:#}", err))
                .finish(),
        }
    }
}

impl<'de, T: Deserialize<'de>, V: Validator<T>> Deserialize<'de> for Validated<T, V> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            ValidatedSink {
                out,
                sink: Some(OwnedSink::deserialize(state)),
                error: None,
                start: None,
                outer: None,
            },
            state,
        )
    }

    /// Missing values are the missing values of `T` (validated).
    fn initial_value() -> Option<Self> {
        T::initial_value().map(Validated::new)
    }
}

struct ValidatedSink<'a, 'de, T, V> {
    out: &'a mut Option<Validated<T, V>>,
    // `None` once the value failed, the rest of it is ignored then
    sink: Option<OwnedSink<'de, T>>,
    error: Option<Error>,
    // the start of the value in the input
    start: Option<usize>,
    // if errors are collected outside of the value while it's deserialized
    outer: Option<bool>,
}

impl<'a, 'de, T, V> ValidatedSink<'a, 'de, T, V> {
    /// Returns the sink of the value unless it failed.
    fn sink(&mut self) -> Option<&mut (dyn Sink<'de> + '_)> {
        self.sink.as_mut().map(|sink| sink.borrow_mut())
    }

    /// Returns the sink of the value at its start.
    ///
    /// The errors in the value are collected.
    fn begin(&mut self, state: &mut State) -> Option<&mut (dyn Sink<'de> + '_)> {
        if self.start.is_none() {
            self.start = state.input_range().map(|range| range.start);
        }
        if self.outer.is_none() {
            self.outer = Some(state.set_collect_errors(true));
        }
        self.sink()
    }

    /// Restores whether errors are collected once the value is complete or
    /// failed.
    fn end(&mut self, state: &mut State) {
        if let Some(outer) = self.outer.take() {
            state.set_collect_errors(outer);
        }
    }

    /// Keeps the error of the value.
    ///
    /// The rest of the value is ignored.  While errors are discarded and
    /// once the limit of errors is reached, the error is returned instead.
    fn fail(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        if state.discards_errors() || state.error_limit_reached() {
            self.end(state);
            return Err(err);
        }
        self.sink = None;
        self.error = Some(state.attach_error_context(err));
        Ok(())
    }

    /// Keeps the error of an operation on the value if it failed.
    fn check(&mut self, rv: Result<(), Error>, state: &mut State) -> Result<(), Error> {
        match rv {
            Ok(()) => Ok(()),
            Err(err) => self.fail(err, state),
        }
    }
}

impl<'a, 'de, T: Send, V: Validator<T>> Sink<'de> for ValidatedSink<'a, 'de, T, V> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let rv = match self.begin(state) {
            Some(sink) => sink.atom(atom, state),
            None => Ok(()),
        };
        self.check(rv, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let rv = match self.begin(state) {
            Some(sink) => sink.borrowed_atom(atom, state),
            None => Ok(()),
        };
        self.check(rv, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = match self.begin(state) {
            Some(sink) => sink.map(state),
            None => Ok(()),
        };
        self.check(rv, state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = match self.begin(state) {
            Some(sink) => sink.seq(state),
            None => Ok(()),
        };
        self.check(rv, state)
    }

    // The errors of the items are handled in `recover`.

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match self.sink() {
            Some(sink) => sink.next_key(state),
            None => Ok(SinkHandle::null()),
        }
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match self.sink() {
            Some(sink) => sink.next_value(state),
            None => Ok(SinkHandle::null()),
        }
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_key_atom(atom, state),
            None => Ok(()),
        }
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_value_atom(atom, state),
            None => Ok(()),
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_borrowed_key_atom(atom, state),
            None => Ok(()),
        }
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_borrowed_value_atom(atom, state),
            None => Ok(()),
        }
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        match self.sink() {
            Some(sink) => sink.value_for_key(key, state),
            None => Ok(None),
        }
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        // the value might recover itself (for instance if it collects
        // errors), otherwise it failed
        let rv = match self.sink() {
            Some(sink) => sink.recover(err, state),
            None => Ok(()),
        };
        self.check(rv, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = match self.sink() {
            Some(sink) => sink.finish(state),
            None => Ok(()),
        };
        self.check(rv, state)?;
        self.end(state);
        let value = self.sink.as_mut().and_then(|sink| sink.take());
        if let Some(ref value) = value
            && let Err(violation) = V::validate(value)
        {
            // the value is kept, it's invalid
            let err = violation.into_error();
            let err = match self.start {
                Some(start) => err.with_offset(start),
                None => err,
            };
            if state.discards_errors() {
                return Err(err);
            }
            self.error = Some(state.attach_error_context(err));
        }
        if value.is_none() && self.error.is_none() {
            return Ok(());
        }
        let error = self.error.take().map(|err| report_error(err, state));
        *self.out = Some(Validated {
            value,
            error,
            _validator: PhantomData,
        });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        match self.sink {
            Some(ref sink) => sink.borrow().expecting(),
            None => Cow::Borrowed("compatible type"),
        }
    }
}

/// Resolves the position of an error that is kept and reports it.
fn report_error(err: Error, state: &mut State) -> Error {
    let err = match Locations::source_map(state) {
        Some(source_map) => err.resolve_position(source_map.source().as_bytes()),
        None => err,
    };
    ReportHandle::report(&err, state);
    err
}

impl<T: Serialize, V> Serialize for Validated<T, V> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.value {
            Some(ref value) => value.serialize(state),
            None => Ok(Chunk::Atom(Atom::Null)),
        }
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        match self.value {
            Some(ref value) => value.finish(state),
            None => Ok(()),
        }
    }

    fn is_optional(&self) -> bool {
        match self.value {
            Some(ref value) => value.is_optional(),
            None => true,
        }
    }

    fn container_shape(&self) -> ContainerShape {
        match self.value {
            Some(ref value) => value.container_shape(),
            None => ContainerShape::new(),
        }
    }

    fn describe(&self, d: &mut dyn Describe) {
        if let Some(ref value) = self.value {
            value.describe(d)
        }
    }
}
