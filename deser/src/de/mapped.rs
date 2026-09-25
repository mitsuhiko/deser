use std::borrow::Cow;

use crate::de::{OwnedSink, Sink, SinkHandle};
use crate::descriptors::Descriptor;
use crate::error::Error;
use crate::event::Atom;
use crate::State;

/// A sink that deserializes a value into an owned sink and converts it.
///
/// All calls are forwarded to the owned sink, once it finished the value is
/// converted and placed in the output slot.
pub(crate) struct MappedSink<'a, T, U> {
    out: &'a mut Option<U>,
    sink: OwnedSink<T>,
    convert: fn(T) -> Result<U, Error>,
}

impl<'a, T: 'a, U: 'a> MappedSink<'a, T, U> {
    /// Creates a handle to a mapped sink.
    pub(crate) fn handle(
        out: &'a mut Option<U>,
        sink: OwnedSink<T>,
        convert: fn(T) -> Result<U, Error>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(MappedSink { out, sink, convert })
    }
}

impl<'a, T, U> Sink for MappedSink<'a, T, U> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.sink.borrow_mut().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.sink.borrow_mut().next_value(state)
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().key_atom(atom, state)
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_>>, Error> {
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
