//! Support for `#[deser(validate = ...)]`.
use std::borrow::Cow;

use crate::State;
use crate::de::{OwnedSink, Sink, SinkHandle};
use crate::error::{Error, conversion_error};
use crate::event::Atom;

/// A function that validates a value.
pub type Validator<T> = fn(&T) -> Result<(), Error>;

/// Creates the error for a value that failed validation.
#[cold]
pub fn invalid_value<E: std::fmt::Display>(err: E) -> Error {
    conversion_error(err)
}

/// Validates the value in a slot if there is one.
#[inline]
pub fn validate_slot<T>(slot: &Option<T>, validate: Validator<T>) -> Result<(), Error> {
    match slot {
        Some(value) => validate(value),
        None => Ok(()),
    }
}

/// A sink that validates the value once it's complete.
///
/// The value is deserialized into an owned sink, validated once it
/// finished and then moved into the output slot.  Errors point to the
/// start of the value.
struct ValidatedSink<'a, 'de, T> {
    out: &'a mut Option<T>,
    sink: OwnedSink<'de, T>,
    validate: Validator<T>,
    // the start of the value in the input
    start: Option<usize>,
}

impl<'a, 'de, T> ValidatedSink<'a, 'de, T> {
    fn begin(&mut self, state: &State) {
        self.start = state.input_range().map(|x| x.start);
    }

    /// Validates the value in the owned sink and moves it to the output.
    fn complete(&mut self) -> Result<(), Error> {
        if let Some(value) = self.sink.take() {
            if let Err(err) = (self.validate)(&value) {
                return Err(match (err.offset(), self.start) {
                    (None, Some(start)) => err.with_offset(start),
                    _ => err,
                });
            }
            *self.out = Some(value);
        }
        Ok(())
    }
}

/// Creates a sink handle that validates the value of an owned sink.
pub fn validated<'a, 'de, T: Send + 'a>(
    out: &'a mut Option<T>,
    sink: OwnedSink<'de, T>,
    validate: Validator<T>,
) -> SinkHandle<'a, 'de> {
    SinkHandle::boxed(ValidatedSink {
        out,
        sink,
        validate,
        start: None,
    })
}

/// Creates a sink handle that validates the value of the handle `make`
/// creates.
pub fn validated_with<'a, 'de, T: Send + 'a>(
    out: &'a mut Option<T>,
    make: for<'x> fn(&'x mut Option<T>) -> SinkHandle<'x, 'de>,
    validate: Validator<T>,
) -> SinkHandle<'a, 'de> {
    validated(out, OwnedSink::with(make), validate)
}

impl<'a, 'de, T: Send> Sink<'de> for ValidatedSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin(state);
        self.sink.borrow_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.begin(state);
        self.sink.borrow_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state);
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state);
        self.sink.borrow_mut().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink
            .borrow_mut()
            .__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink
            .borrow_mut()
            .__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.sink.borrow_mut().value_for_key(key, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        self.complete()
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.borrow().expecting()
    }
}
