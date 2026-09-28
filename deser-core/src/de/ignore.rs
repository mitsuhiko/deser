use crate::Atom;
use crate::State;
use crate::de::{Sink, SinkHandle};
use crate::error::Error;

pub(super) struct Ignore;

impl<'de> Sink<'de> for Ignore {
    fn atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn __private_key_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn __private_value_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn borrowed_atom(&mut self, _atom: Atom<'de>, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn __private_borrowed_key_atom(
        &mut self,
        _atom: Atom<'de>,
        _state: &mut State,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn __private_borrowed_value_atom(
        &mut self,
        _atom: Atom<'de>,
        _state: &mut State,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::null())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::null())
    }
}
