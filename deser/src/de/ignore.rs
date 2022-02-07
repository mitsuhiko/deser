use crate::de::{DeserializerState, MapSink, SeqSink, Sink, SinkHandle};
use crate::error::Error;
use crate::Atom;

pub struct Ignore;

impl Sink for Ignore {
    fn atom(&mut self, _atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<Box<dyn MapSink + '_>, crate::Error> {
        Ok(Box::new(Ignore))
    }

    fn seq(&mut self, _state: &DeserializerState) -> Result<Box<dyn SeqSink + '_>, crate::Error> {
        Ok(Box::new(Ignore))
    }
}

impl MapSink for Ignore {
    fn key(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::null())
    }

    fn value(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::null())
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }
}

impl SeqSink for Ignore {
    fn item(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::null())
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }
}
