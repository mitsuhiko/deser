//! Support for collecting the errors of items (see
//! [`State::set_collect_errors`]).
use crate::State;
use crate::error::Error;

/// Collects the errors of a whole deserialization (a value of the
/// [`Context`](crate::Context)).
///
/// With this in the context, maps and sequences collect the errors of their
/// items and deserialization continues to find the other errors (see
/// [`State::set_collect_errors`]).  The limit of errors is optional:
///
/// ```
/// use deser::de::CollectErrors;
/// use deser::Context;
///
/// let config = deser_json::DeserializerConfig::builder()
///     .context(Context::with(CollectErrors::with_max_errors(10)))
///     .build();
/// let err = config.from_str::<Vec<u32>>(r#"["a", 1, true]"#).unwrap_err();
/// assert_eq!(err.errors().count(), 2);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollectErrors {
    pub(crate) max: Option<usize>,
}

impl CollectErrors {
    /// Collects errors without limit.
    pub const fn new() -> CollectErrors {
        CollectErrors { max: None }
    }

    /// Collects errors up to a limit (see [`set_max_errors`](Self::set_max_errors)).
    pub const fn with_max_errors(max: usize) -> CollectErrors {
        CollectErrors { max: Some(max) }
    }

    /// Limits the number of errors that are collected (see
    /// [`State::set_max_errors`]).
    pub const fn set_max_errors(&mut self, max: usize) {
        self.max = Some(max);
    }

    /// Returns the limit of errors (see [`set_max_errors`](Self::set_max_errors)).
    pub const fn max_errors(&self) -> Option<usize> {
        self.max
    }
}

/// The errors a map or sequence collected from its items.
///
/// This is a helper for sinks that collect the errors of their items (see
/// [`State::set_collect_errors`]).  In [`Sink::recover`](crate::de::Sink::recover)
/// the error is passed to [`collect`](Self::collect), which keeps it if
/// errors are collected.  Once the container is complete,
/// [`finish`](Self::finish) fails with all errors that were collected:
///
/// ```
/// use deser::de::{CollectedErrors, Deserialize, Sink, SinkHandle};
/// use deser::{Error, State};
///
/// struct Numbers(Vec<u32>);
///
/// struct NumbersSink<'a> {
///     out: &'a mut Option<Numbers>,
///     numbers: Vec<u32>,
///     current: Option<u32>,
///     errors: CollectedErrors,
/// }
///
/// impl<'a> NumbersSink<'a> {
///     fn flush(&mut self) {
///         self.numbers.extend(self.current.take());
///     }
/// }
///
/// impl<'de> Sink<'de> for NumbersSink<'_> {
///     fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
///         Ok(())
///     }
///
///     fn next_value(
///         &mut self,
///         state: &mut State,
///     ) -> Result<SinkHandle<'_, 'de>, Error> {
///         self.flush();
///         Ok(u32::deserialize_into(&mut self.current, state))
///     }
///
///     fn recover(
///         &mut self,
///         err: Error,
///         state: &mut State,
///     ) -> Result<(), Error> {
///         self.current = None;
///         self.errors.collect(err, state)
///     }
///
///     fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
///         self.errors.finish()?;
///         self.flush();
///         *self.out = Some(Numbers(std::mem::take(&mut self.numbers)));
///         Ok(())
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct CollectedErrors {
    // a single pointer as every container sink holds this, it's an error
    // that holds multiple errors once there are more
    errors: Option<Error>,
}

impl CollectedErrors {
    /// Creates an empty list of errors.
    #[inline]
    pub const fn new() -> CollectedErrors {
        CollectedErrors { errors: None }
    }

    /// Returns `true` if no errors were collected.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.errors.is_none()
    }

    fn add(&mut self, err: Error) {
        match self.errors {
            Some(ref mut errors) => errors.push_error(err),
            None => self.errors = Some(err),
        }
    }

    /// Collects the error of an item if errors are collected.
    ///
    /// If errors are not collected (or the limit of errors is reached, see
    /// [`State::set_max_errors`]), the error is returned together with the
    /// errors collected so far.  Errors which do not have the context of an
    /// event attached yet (see [`Error`]) get the context of the current
    /// event.
    #[cold]
    pub fn collect(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        // errors that other containers collected already (and failed with
        // once they were complete) do not count again
        let new = err.uncollected_count();
        if new == 0 && state.collects_errors() || state.take_error_slots(new) {
            self.add(state.error_in_context(err).mark_collected());
            Ok(())
        } else if self.errors.is_none() {
            Err(err)
        } else {
            // the new errors are not marked as collected: the containers the
            // error passes through do not collect it either
            self.add(state.error_in_context(err));
            Err(self.take().unwrap())
        }
    }

    /// Adds an error that is not the error of an item.
    ///
    /// This is for errors that are found once the container is complete,
    /// for instance missing fields.  They are always added and do not count
    /// towards the limit of errors.  Errors which do not have the context
    /// of an event attached yet get the context of the current event.
    #[cold]
    pub fn push(&mut self, err: Error, state: &State) {
        self.add(state.error_in_context(err).mark_collected());
    }

    /// Takes the collected errors as a single error.
    ///
    /// Returns `None` if there are none.
    pub fn take(&mut self) -> Option<Error> {
        self.errors.take()
    }

    /// Fails with the collected errors if there are any.
    #[inline(always)]
    pub fn finish(&mut self) -> Result<(), Error> {
        if self.errors.is_none() {
            Ok(())
        } else {
            Err(self.take_cold())
        }
    }

    #[cold]
    #[inline(never)]
    fn take_cold(&mut self) -> Error {
        self.errors.take().unwrap()
    }
}
