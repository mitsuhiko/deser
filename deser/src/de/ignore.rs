use crate::de::{DeserializerState, MapSink, SeqSink, Sink, SinkHandle};
use crate::error::Error;
use crate::Atom;

/// A [`Sink`] that ignores all values.
///
/// This can be used in places where a sink is required but no value
/// wants to be collected.  For instance it can be tricky to provide a
/// mutable reference to a sink from a function that doesn't have a way
/// to put a slot somewhere.
pub fn ignore() -> &'static mut dyn Sink {
    // invariant: the Ignore type is stateless and giving out a mutable
    // reference to it is safe.
    unsafe { extend_lifetime!(&mut Ignore, &mut Ignore) }
}

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
        Ok(SinkHandle::to(ignore()))
    }

    fn value(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::to(ignore()))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }
}

impl SeqSink for Ignore {
    fn item(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::to(ignore()))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }
}
