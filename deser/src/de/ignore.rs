use crate::de::{Sink, SinkHandle};
use crate::error::Error;
use crate::Atom;
use crate::State;

pub struct Ignore;

impl Sink for Ignore {
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

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::null())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::null())
    }
}
