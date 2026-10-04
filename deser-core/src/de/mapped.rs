use crate::State;
use crate::de::{OwnedSink, Sink, SinkHandle};
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

impl<'a, 'de, T: Send + 'a, U: Send + 'a> MappedSink<'a, 'de, T, U> {
    /// Creates a handle to a mapped sink.
    pub(crate) fn handle(
        out: &'a mut Option<U>,
        sink: OwnedSink<'de, T>,
        convert: fn(T) -> Result<U, Error>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::arena(MappedSink { out, sink, convert }, state)
    }
}

/// Creates a handle to a sink that deserializes a value into an owned sink
/// and converts it.
///
/// The derive uses this for tuple structs which are deserialized as tuples.
#[cfg(feature = "derive")]
pub fn mapped<'a, 'de, T: Send + 'a, U: Send + 'a>(
    out: &'a mut Option<U>,
    sink: OwnedSink<'de, T>,
    convert: fn(T) -> Result<U, Error>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    MappedSink::handle(out, sink, convert, state)
}

impl<'a, 'de, T: Send, U: Send> Sink<'de> for MappedSink<'a, 'de, T, U> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().seq(state)
    }

    forward_to_owned!(sink);

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().finish(state)?;
        if let Some(value) = self.sink.take() {
            *self.out = Some((self.convert)(value)?);
        }
        Ok(())
    }
}
