use crate::de::{DeserializerState, Sink, SinkHandle};
use crate::error::Error;
use crate::Atom;

pub struct Ignore;

impl Sink for Ignore {
    fn atom(&mut self, _atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::null())
    }

    fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::null())
    }
}
