use crate::de::{Sink, SinkHandle};
use crate::error::Error;
use crate::Atom;
use crate::State;

pub struct Ignore;

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

    fn key_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn value_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn borrowed_atom(&mut self, _atom: Atom<'de>, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn borrowed_key_atom(&mut self, _atom: Atom<'de>, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn borrowed_value_atom(&mut self, _atom: Atom<'de>, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::null())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::null())
    }
}
