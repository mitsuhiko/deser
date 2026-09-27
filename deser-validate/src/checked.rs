//! A value that is known to be valid.
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{Chunk, Describe, Serialize};
use deser_core::{Atom, ContainerShape, Error, State};

use crate::{Validator, Violation};

/// A value that was validated with `V`.
///
/// A `Checked` value can only be created with a value that is valid, so
/// holding one is proof that it is.  If the value is invalid,
/// deserialization fails (like with `#[deser(validate = ...)]`) and the
/// error has the [`Violation`] attached.  Errors point at the start of the
/// value.  The value is available through [`Deref`]:
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::{Checked, Len, Range};
///
/// #[derive(Deserialize, Debug)]
/// struct Server {
///     name: Checked<String, Len<1, 32>>,
///     port: Checked<u16, Range<1, 65535>>,
/// }
///
/// let server: Server = deser_json::from_str(r#"{"name": "web", "port": 80}"#).unwrap();
/// assert_eq!(server.name.len(), 3);
/// assert_eq!(*server.port, 80);
///
/// let err = deser_json::from_str::<Server>(r#"{"name": "", "port": 80}"#).unwrap_err();
/// assert_eq!(
///     err.to_string(),
///     "Unexpected: invalid value: length must be between 1 and 32 at line 1 column 10"
/// );
/// ```
///
/// When serialized, only the value is serialized.
pub struct Checked<T, V> {
    value: T,
    _validator: PhantomData<fn() -> V>,
}

impl<T, V: Validator<T>> Checked<T, V> {
    /// Validates a value.
    pub fn new(value: T) -> Result<Checked<T, V>, Violation> {
        V::validate(&value)?;
        Ok(Checked {
            value,
            _validator: PhantomData,
        })
    }
}

impl<T, V> Checked<T, V> {
    /// Returns the value.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T, V> Deref for Checked<T, V> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T, V> AsRef<T> for Checked<T, V> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}

impl<T: fmt::Debug, V> fmt::Debug for Checked<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.value, f)
    }
}

impl<T: fmt::Display, V> fmt::Display for Checked<T, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.value, f)
    }
}

impl<T: Clone, V> Clone for Checked<T, V> {
    fn clone(&self) -> Self {
        Checked {
            value: self.value.clone(),
            _validator: PhantomData,
        }
    }
}

impl<T: PartialEq, V> PartialEq for Checked<T, V> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq, V> Eq for Checked<T, V> {}

impl<'de, T: Deserialize<'de>, V: Validator<T>> Deserialize<'de> for Checked<T, V> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        CheckSink::<T, V, Self>::handle(out, OwnedSink::deserialize(), |value| Checked {
            value,
            _validator: PhantomData,
        })
    }

    /// Missing values are the missing values of `T` if they are valid.
    fn initial_value() -> Option<Self> {
        T::initial_value().and_then(|value| Checked::new(value).ok())
    }
}

/// A sink that validates a value once it's complete.
///
/// The value is deserialized into an owned sink, validated and converted
/// into the output.  Errors point at the start of the value.
pub(crate) struct CheckSink<'a, 'de, T, V, U> {
    out: &'a mut Option<U>,
    sink: OwnedSink<'de, T>,
    convert: fn(T) -> U,
    // the start of the value in the input
    start: Option<usize>,
    _validator: PhantomData<fn() -> V>,
}

impl<'a, 'de, T: Send + 'a, V: Validator<T> + 'a, U: Send + 'a> CheckSink<'a, 'de, T, V, U> {
    /// Creates a handle to the sink.
    pub(crate) fn handle(
        out: &'a mut Option<U>,
        sink: OwnedSink<'de, T>,
        convert: fn(T) -> U,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(CheckSink {
            out,
            sink,
            convert,
            start: None,
            _validator: PhantomData::<fn() -> V>,
        })
    }
}

impl<'a, 'de, T, V, U> CheckSink<'a, 'de, T, V, U> {
    fn begin(&mut self, state: &State) -> &mut (dyn Sink<'de> + '_) {
        self.start = state.input_range().map(|range| range.start);
        self.sink.borrow_mut()
    }
}

impl<'a, 'de, T: Send, V: Validator<T>, U: Send> Sink<'de> for CheckSink<'a, 'de, T, V, U> {
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
        self.sink.borrow_mut().recover(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        if let Some(value) = self.sink.take() {
            if let Err(violation) = V::validate(&value) {
                let err = violation.into_error();
                return Err(match self.start {
                    Some(start) => err.with_offset(start),
                    None => err,
                });
            }
            *self.out = Some((self.convert)(value));
        }
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.borrow().expecting()
    }
}

impl<T: Serialize, V> Serialize for Checked<T, V> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.value.serialize(state)
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.value.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.value.is_optional()
    }

    fn container_shape(&self) -> ContainerShape {
        self.value.container_shape()
    }

    fn describe(&self, d: &mut dyn Describe) {
        self.value.describe(d)
    }
}
