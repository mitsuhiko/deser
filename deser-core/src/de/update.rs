//! Support for updating existing values (see
//! [`Deserialize::deserialize_update`]).
use std::borrow::Cow;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::State;
use crate::de::{Deserialize, OwnedSink, Sink, SinkHandle, is_null_atom};
use crate::error::Error;
use crate::event::Atom;

/// Forwards all calls of a sink to an owned sink.
macro_rules! forward_to_owned {
    ($field:ident) => {
        fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.$field.borrow_mut().next_key(state)
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.$field.borrow_mut().next_value(state)
        }

        fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.$field.borrow_mut().__private_key_atom(atom, state)
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.$field.borrow_mut().__private_value_atom(atom, state)
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.$field
                .borrow_mut()
                .__private_borrowed_key_atom(atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.$field
                .borrow_mut()
                .__private_borrowed_value_atom(atom, state)
        }

        fn value_for_key(
            &mut self,
            key: &str,
            state: &mut State,
        ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
            self.$field.borrow_mut().value_for_key(key, state)
        }

        fn expecting(&self) -> Cow<'_, str> {
            self.$field.borrow().expecting()
        }
    };
}

/// A function that validates a value.
pub type ValidateFn<T> = fn(&T) -> Result<(), Error>;

/// The part of replacing a value that depends on its type.
///
/// [`ReplaceSink`] is the same for all types (it exists once), it only
/// calls into this.
trait Replace<'de>: Send {
    /// Returns the sink of the new value.
    fn sink(&mut self) -> &mut (dyn Sink<'de> + '_);

    /// Returns the sink of the new value.
    fn sink_ref(&self) -> &(dyn Sink<'de> + '_);

    /// Returns `true` if the new value is validated.
    fn validates(&self) -> bool;

    /// Validates the new value (if there is one) and replaces the value.
    fn replace(&mut self) -> Result<(), Error>;
}

/// Replaces a value of a type.
struct Replacer<'a, 'de, T> {
    out: &'a mut T,
    sink: OwnedSink<'de, T>,
    validate: Option<ValidateFn<T>>,
}

impl<'a, 'de, T: Send> Replace<'de> for Replacer<'a, 'de, T> {
    fn sink(&mut self) -> &mut (dyn Sink<'de> + '_) {
        self.sink.borrow_mut()
    }

    fn sink_ref(&self) -> &(dyn Sink<'de> + '_) {
        self.sink.borrow()
    }

    fn validates(&self) -> bool {
        self.validate.is_some()
    }

    fn replace(&mut self) -> Result<(), Error> {
        if let Some(value) = self.sink.take() {
            if let Some(validate) = self.validate {
                validate(&value)?;
            }
            *self.out = value;
        }
        Ok(())
    }
}

/// A sink that replaces a value.
///
/// The new value is deserialized into an owned sink and replaces the value
/// once it's complete.  If a validator is given, the new value is only used
/// if it's valid, errors point at its start.
struct ReplaceSink<'a, 'de> {
    inner: Box<dyn Replace<'de> + 'a>,
    start: Option<usize>,
}

/// Creates a sink handle that replaces a value.
///
/// This is the default implementation of
/// [`Deserialize::deserialize_update`].
pub fn replace_handle<'a, 'de, T: Deserialize<'de>>(out: &'a mut T) -> SinkHandle<'a, 'de> {
    replace_with(out, OwnedSink::deserialize(), None)
}

/// Creates a sink handle that replaces a value with the value of an owned
/// sink, which is validated first if a validator is given.
pub fn replace_with<'a, 'de, T: Send + 'a>(
    out: &'a mut T,
    sink: OwnedSink<'de, T>,
    validate: Option<ValidateFn<T>>,
) -> SinkHandle<'a, 'de> {
    SinkHandle::boxed(ReplaceSink {
        inner: Box::new(Replacer {
            out,
            sink,
            validate,
        }),
        start: None,
    })
}

impl<'a, 'de> ReplaceSink<'a, 'de> {
    fn begin(&mut self, state: &State) -> &mut (dyn Sink<'de> + '_) {
        if self.inner.validates() {
            self.start = state.input_range().map(|x| x.start);
        }
        self.inner.sink()
    }
}

impl<'a, 'de> Sink<'de> for ReplaceSink<'a, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin(state).atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.begin(state).borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state).seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.inner.sink().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.inner.sink().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.sink().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.sink().__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.inner.sink().__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.inner.sink().__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.inner.sink().value_for_key(key, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.inner.sink_ref().expecting()
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.sink().finish(state)?;
        self.inner
            .replace()
            .map_err(|err| match (err.offset(), self.start) {
                (None, Some(start)) => err.with_offset(start),
                _ => err,
            })
    }
}

/// A sink that updates the value of an `Option` which is set.
///
/// The value is moved into an owned sink which updates it.  Null clears the
/// option, everything else updates the value.  The value is moved back
/// when the update finished, or failed.
struct OptionUpdateSink<'a, 'de, T> {
    out: &'a mut Option<T>,
    sink: OwnedSink<'de, T>,
}

/// Creates a sink handle that updates an `Option`.
///
/// If the option is set, the value in it is updated, otherwise a new value
/// is deserialized.  Null clears the option.
pub(crate) fn update_option<'a, 'de, T: Deserialize<'de>>(
    out: &'a mut Option<T>,
) -> SinkHandle<'a, 'de> {
    match out.take() {
        Some(value) => SinkHandle::boxed(OptionUpdateSink {
            out,
            sink: OwnedSink::update(value),
        }),
        None => replace_handle(out),
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for OptionUpdateSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if is_null_atom(&atom) {
            // the value is dropped, the option remains empty
            drop(self.sink.take());
            return Ok(());
        }
        self.sink.borrow_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        if is_null_atom(&atom) {
            drop(self.sink.take());
            return Ok(());
        }
        self.sink.borrow_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().seq(state)
    }

    forward_to_owned!(sink);

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        *self.out = self.sink.take();
        Ok(())
    }
}

impl<'a, 'de, T> Drop for OptionUpdateSink<'a, 'de, T> {
    fn drop(&mut self) {
        // if the update failed, the (partially updated) value is put back
        if let Some(value) = self.sink.take() {
            *self.out = Some(value);
        }
    }
}

/// A struct that is updated field by field.
///
/// The update sinks of derived structs with flattened fields keep the sinks
/// of the flattened fields (which borrow them) for the whole update while
/// they update the other fields, and look at the whole struct once they are
/// done (to validate it).  A mutable reference to the struct cannot be used
/// for this, so the fields are borrowed through a pointer.  The derive
/// borrows every field at most once at a time and only borrows the struct
/// as a whole once the borrows of all fields ended.
#[doc(hidden)]
pub struct UpdateTarget<'a, T> {
    ptr: NonNull<T>,
    _marker: PhantomData<&'a mut T>,
}

// SAFETY: this is a mutable reference to `T`.
unsafe impl<T: Send> Send for UpdateTarget<'_, T> {}

impl<'a, T> UpdateTarget<'a, T> {
    /// Creates the target for a value.
    #[inline]
    pub fn new(value: &'a mut T) -> UpdateTarget<'a, T> {
        UpdateTarget {
            ptr: NonNull::from(value),
            _marker: PhantomData,
        }
    }

    /// Returns the pointer to the value.
    ///
    /// Fields are borrowed with `&mut (*target.as_ptr()).field`, which does
    /// not borrow the other fields.
    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}
