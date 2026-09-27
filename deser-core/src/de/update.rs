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

        fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
            self.$field.borrow_mut().recover(err, state)
        }

        fn expecting(&self) -> Cow<'_, str> {
            self.$field.borrow().expecting()
        }
    };
}

/// The part of replacing a value that depends on its type.
///
/// [`ReplaceSink`] is the same for all types (it exists once), it only
/// calls into this.
trait Replace<'de>: Send {
    /// Returns the sink of the new value.
    fn sink(&mut self) -> &mut (dyn Sink<'de> + '_);

    /// Returns the sink of the new value.
    fn sink_ref(&self) -> &(dyn Sink<'de> + '_);

    /// Replaces the value with the new value (if there is one).
    fn replace(&mut self);
}

/// Replaces a value of a type.
struct Replacer<'a, 'de, T> {
    out: &'a mut T,
    sink: OwnedSink<'de, T>,
}

impl<'a, 'de, T: Send> Replace<'de> for Replacer<'a, 'de, T> {
    fn sink(&mut self) -> &mut (dyn Sink<'de> + '_) {
        self.sink.borrow_mut()
    }

    fn sink_ref(&self) -> &(dyn Sink<'de> + '_) {
        self.sink.borrow()
    }

    fn replace(&mut self) {
        if let Some(value) = self.sink.take() {
            *self.out = value;
        }
    }
}

/// A sink that replaces a value.
///
/// The new value is deserialized into an owned sink and replaces the value
/// once it's complete.
struct ReplaceSink<'a, 'de> {
    inner: Box<dyn Replace<'de> + 'a>,
}

/// Creates a sink handle that replaces a value.
///
/// This is the default implementation of
/// [`Deserialize::deserialize_update`].
pub fn replace_handle<'a, 'de, T: Deserialize<'de>>(out: &'a mut T) -> SinkHandle<'a, 'de> {
    replace_with(out, OwnedSink::deserialize())
}

/// Creates a sink handle that replaces a value with a value that is
/// deserialized with a sink the function creates.
pub(crate) fn replace_handle_with<'a, 'de, T: Send + 'a>(
    out: &'a mut T,
    make: for<'x> fn(&'x mut Option<T>) -> SinkHandle<'x, 'de>,
) -> SinkHandle<'a, 'de> {
    replace_with(out, OwnedSink::with(make))
}

/// Creates a sink handle that updates a value and checks it once the update
/// is complete.
///
/// `update` creates the sink that updates the value (for instance
/// [`Deserialize::deserialize_update`] or
/// [`DeserializeAs::deserialize_update_as`](crate::adapters::DeserializeAs::deserialize_update_as)).
/// Once the update is complete, `check` is invoked with the updated value.
/// Errors it returns point at the start of the value in the input.  The
/// value is updated in place, if the check fails it's updated anyway (like
/// when an update fails, see [`Deserialize::deserialize_update`]).  This is
/// for adapters that check values to support updates:
///
/// ```
/// use deser::de::{DeserializeDriver, checked_update};
/// use deser::{Deserialize, Error, ErrorKind, Event};
///
/// #[derive(Deserialize)]
/// struct Range {
///     min: u32,
///     max: u32,
/// }
///
/// fn check(range: &Range) -> Result<(), Error> {
///     if range.min > range.max {
///         return Err(Error::new(ErrorKind::Unexpected, "min is larger than max"));
///     }
///     Ok(())
/// }
///
/// let mut range = Range { min: 1, max: 5 };
/// let mut driver = DeserializeDriver::from_sink(checked_update(
///     &mut range,
///     Range::deserialize_update,
///     check,
/// ));
/// driver.emit(Event::map_start()).unwrap();
/// driver.emit("min").unwrap();
/// driver.emit(10u64).unwrap();
/// let err = driver.emit(Event::MapEnd).unwrap_err();
/// assert_eq!(err.message(), "min is larger than max");
/// ```
pub fn checked_update<'a, 'de, T: Send + 'a>(
    value: &'a mut T,
    update: for<'x> fn(&'x mut T) -> SinkHandle<'x, 'de>,
    check: fn(&T) -> Result<(), Error>,
) -> SinkHandle<'a, 'de> {
    let ptr = NonNull::from(value);
    // SAFETY: the sink borrows the value for 'a, the pointer is only used
    // again once the sink was dropped (in `finish`).
    let sink = update(unsafe { &mut *ptr.as_ptr() });
    SinkHandle::boxed(CheckedUpdateSink {
        value: ptr,
        sink: Some(sink),
        check,
        start: None,
        _marker: PhantomData,
    })
}

/// The sink of [`checked_update`].
struct CheckedUpdateSink<'a, 'de, T> {
    value: NonNull<T>,
    // borrows the value, `None` once the update is complete
    sink: Option<SinkHandle<'a, 'de>>,
    check: fn(&T) -> Result<(), Error>,
    // the start of the value in the input
    start: Option<usize>,
    _marker: PhantomData<&'a mut T>,
}

// SAFETY: the sink holds a mutable reference to the value (as pointer).
unsafe impl<T: Send> Send for CheckedUpdateSink<'_, '_, T> {}

impl<'a, 'de, T> CheckedUpdateSink<'a, 'de, T> {
    fn sink(&mut self) -> &mut SinkHandle<'a, 'de> {
        self.sink.as_mut().expect("update is complete")
    }

    fn begin(&mut self, state: &State) -> &mut SinkHandle<'a, 'de> {
        if self.start.is_none() {
            self.start = state.input_range().map(|x| x.start);
        }
        self.sink()
    }
}

impl<'a, 'de, T: Send> Sink<'de> for CheckedUpdateSink<'a, 'de, T> {
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
        self.sink().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink().__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink().__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink().__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.sink().value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.sink().recover(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = self.sink().finish(state);
        // the sink borrows the value, it's dropped before the value is used
        self.sink = None;
        rv?;
        // SAFETY: nothing borrows the value anymore
        let value = unsafe { self.value.as_ref() };
        (self.check)(value).map_err(|err| match (err.offset(), self.start) {
            (None, Some(start)) => err.with_offset(start),
            _ => err,
        })
    }

    fn expecting(&self) -> Cow<'_, str> {
        match self.sink {
            Some(ref sink) => sink.expecting(),
            None => Cow::Borrowed("compatible type"),
        }
    }
}

/// Creates a sink handle that replaces a value with the value of an owned
/// sink.
pub fn replace_with<'a, 'de, T: Send + 'a>(
    out: &'a mut T,
    sink: OwnedSink<'de, T>,
) -> SinkHandle<'a, 'de> {
    SinkHandle::boxed(ReplaceSink {
        inner: Box::new(Replacer { out, sink }),
    })
}

impl<'a, 'de> Sink<'de> for ReplaceSink<'a, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.sink().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.inner.sink().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.sink().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.sink().seq(state)
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

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.inner.sink().recover(err, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.inner.sink_ref().expecting()
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.sink().finish(state)?;
        self.inner.replace();
        Ok(())
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
#[cfg(feature = "derive")]
#[doc(hidden)]
pub struct UpdateTarget<'a, T> {
    ptr: std::ptr::NonNull<T>,
    _marker: std::marker::PhantomData<&'a mut T>,
}

// SAFETY: this is a mutable reference to `T`.
#[cfg(feature = "derive")]
unsafe impl<T: Send> Send for UpdateTarget<'_, T> {}

#[cfg(feature = "derive")]
impl<'a, T> UpdateTarget<'a, T> {
    /// Creates the target for a value.
    #[inline]
    pub fn new(value: &'a mut T) -> UpdateTarget<'a, T> {
        UpdateTarget {
            ptr: std::ptr::NonNull::from(value),
            _marker: std::marker::PhantomData,
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
