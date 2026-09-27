//! Collecting the errors of a value.
use std::borrow::Cow;
use std::fmt;
use std::ops::{Deref, DerefMut};

use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{Chunk, Describe, Serialize};
use deser_core::{Atom, ContainerShape, Error, State};

/// Collects the errors in a value.
///
/// Within the value, maps and sequences collect the errors of their items
/// (see [`State::set_collect_errors`]): if the value is invalid, the error
/// holds all of its errors rather than just the first one.  Together with
/// [`Validated`](crate::Validated) this reports all problems of a part of
/// the input:
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::{Collect, Validated};
///
/// #[derive(Deserialize)]
/// struct Address {
///     street: String,
///     zip: u32,
/// }
///
/// #[derive(Deserialize)]
/// struct Order {
///     id: u64,
///     shipping: Validated<Collect<Address>>,
/// }
///
/// let order: Order = deser_json::from_str(r#"{"id": 1, "shipping": {"zip": "x"}}"#).unwrap();
/// let err = order.shipping.error().unwrap();
/// let errors: Vec<_> = err.errors().map(|err| err.message()).collect();
/// assert_eq!(errors, ["unexpected string, expected u32", "missing field `street`"]);
/// ```
///
/// When serialized, the value is serialized.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Collect<T>(pub T);

impl<T> Collect<T> {
    /// Returns the value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Collect<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Collect<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for Collect<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Collect<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(CollectSink {
            out,
            sink: OwnedSink::deserialize(),
            outer: None,
        })
    }

    fn initial_value() -> Option<Self> {
        T::initial_value().map(Collect)
    }
}

struct CollectSink<'a, 'de, T> {
    out: &'a mut Option<Collect<T>>,
    sink: OwnedSink<'de, T>,
    // the setting outside of the value while it's deserialized
    outer: Option<bool>,
}

impl<'a, 'de, T> CollectSink<'a, 'de, T> {
    /// Starts collecting errors.
    fn begin(&mut self, state: &mut State) -> &mut (dyn Sink<'de> + '_) {
        if self.outer.is_none() {
            self.outer = Some(state.set_collect_errors(true));
        }
        self.sink.borrow_mut()
    }

    /// Restores the setting outside of the value once it's complete or
    /// failed.
    fn end<R>(&mut self, rv: Result<R, Error>, state: &mut State) -> Result<R, Error> {
        if let Some(outer) = self.outer.take() {
            state.set_collect_errors(outer);
        }
        rv
    }
}

impl<'a, 'de, T: Send> Sink<'de> for CollectSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let rv = self.begin(state).atom(atom, state);
        match rv {
            Ok(()) => Ok(()),
            err => self.end(err, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let rv = self.begin(state).borrowed_atom(atom, state);
        match rv {
            Ok(()) => Ok(()),
            err => self.end(err, state),
        }
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = self.begin(state).map(state);
        match rv {
            Ok(()) => Ok(()),
            err => self.end(err, state),
        }
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = self.begin(state).seq(state);
        match rv {
            Ok(()) => Ok(()),
            err => self.end(err, state),
        }
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

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        // if the value does not recover, the error passes through and the
        // value is complete
        match self.sink.borrow_mut().recover(err, state) {
            Ok(()) => Ok(()),
            err => self.end(err, state),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = self.sink.borrow_mut().finish(state);
        self.end(rv, state)?;
        *self.out = self.sink.take().map(Collect);
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.borrow().expecting()
    }
}

impl<T: Serialize> Serialize for Collect<T> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.0.serialize(state)
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.0.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.0.is_optional()
    }

    fn container_shape(&self) -> ContainerShape {
        self.0.container_shape()
    }

    fn describe(&self, d: &mut dyn Describe) {
        self.0.describe(d)
    }
}
