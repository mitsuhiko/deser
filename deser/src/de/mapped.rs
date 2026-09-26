use std::borrow::Cow;

use crate::State;
use crate::de::{OwnedSink, Sink, SinkHandle};
use crate::descriptors::Descriptor;
use crate::error::Error;
use crate::event::Atom;

/// A sink that deserializes a value into an owned sink and converts it.
///
/// All calls are forwarded to the owned sink, once it finished the value is
/// converted and placed in the output slot.
pub(crate) struct MappedSink<'a, 'de, T, U> {
    out: &'a mut Option<U>,
    sink: OwnedSink<'de, T>,
    convert: fn(T) -> Result<U, Error>,
}

impl<'a, 'de, T: 'a, U: 'a> MappedSink<'a, 'de, T, U> {
    /// Creates a handle to a mapped sink.
    pub(crate) fn handle(
        out: &'a mut Option<U>,
        sink: OwnedSink<'de, T>,
        convert: fn(T) -> Result<U, Error>,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(MappedSink { out, sink, convert })
    }
}

impl<'a, 'de, T, U> Sink<'de> for MappedSink<'a, 'de, T, U> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_value(state)
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().key_atom(atom, state)
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().value_atom(atom, state)
    }

    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().borrowed_key_atom(atom, state)
    }

    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().borrowed_value_atom(atom, state)
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
        if let Some(value) = self.sink.take() {
            *self.out = Some((self.convert)(value)?);
        }
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.sink.borrow().descriptor()
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.borrow().expecting()
    }
}
