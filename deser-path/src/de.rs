use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deser::de::{DeserializerState, MapSink, SeqSink, Sink, SinkHandle};
use deser::{Atom, Error};

use crate::{Path, PathSegment};

/// A path sink tracks the current path during deserialization.
pub struct PathSink<'a> {
    sink: SinkHandle<'a>,
    set_segment: Option<PathSegment>,
}

impl<'a> PathSink<'a> {
    /// Wraps a sink.
    pub fn wrap(sink: &'a mut dyn Sink) -> PathSink<'a> {
        PathSink::wrap_ref(SinkHandle::to(sink))
    }

    /// Wraps a sink ref.
    pub fn wrap_ref(sink: SinkHandle<'a>) -> PathSink<'a> {
        PathSink {
            sink,
            set_segment: None,
        }
    }

    fn set_segment(&mut self, state: &DeserializerState) {
        if let Some(segment) = self.set_segment.take() {
            *state.get_mut::<Path>().segments.last_mut().unwrap() = segment;
        }
    }
}

impl<'a> Sink for PathSink<'a> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.atom(atom, state)
    }

    fn map(&mut self, state: &DeserializerState) -> Result<Box<dyn MapSink + '_>, Error> {
        self.set_segment(state);
        state.get_mut::<Path>().segments.push(PathSegment::Index(0));
        Ok(Box::new(PathTrackingMapSink {
            sink: self.sink.map(state)?,
            captured_segment: Rc::default(),
        }))
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<Box<dyn SeqSink + '_>, Error> {
        self.set_segment(state);
        state.get_mut::<Path>().segments.push(PathSegment::Index(0));
        Ok(Box::new(PathTrackingSeqSink {
            sink: self.sink.seq(state)?,
            index: 0,
        }))
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.expecting()
    }
}

struct PathTrackingMapSink<'a> {
    sink: Box<dyn MapSink + 'a>,
    captured_segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> MapSink for PathTrackingMapSink<'a> {
    fn key(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::boxed(KeyCapturingSink {
            sink: self.sink.key()?,
            captured_segment: self.captured_segment.clone(),
        }))
    }

    fn value(&mut self) -> Result<SinkHandle, Error> {
        Ok(SinkHandle::boxed(PathSink {
            sink: self.sink.value()?,
            set_segment: self.captured_segment.take(),
        }))
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        state.get_mut::<Path>().segments.pop();
        self.sink.finish(state)
    }
}

struct PathTrackingSeqSink<'a> {
    sink: Box<dyn SeqSink + 'a>,
    index: usize,
}

impl<'a> SeqSink for PathTrackingSeqSink<'a> {
    fn item(&mut self) -> Result<SinkHandle, Error> {
        let sink_wrapper = PathSink {
            sink: self.sink.item()?,
            set_segment: Some(PathSegment::Index(self.index)),
        };
        Ok(SinkHandle::boxed(sink_wrapper))
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        state.get_mut::<Path>().segments.pop();
        self.sink.finish(state)
    }
}

struct KeyCapturingSink<'a> {
    sink: SinkHandle<'a>,
    captured_segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> Sink for KeyCapturingSink<'a> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        *self.captured_segment.borrow_mut() = match atom {
            Atom::Str(ref value) => Some(PathSegment::Key(value.to_string())),
            Atom::U64(value) => Some(PathSegment::Index(value as usize)),
            Atom::I64(value) => Some(PathSegment::Index(value as usize)),
            _ => None,
        };
        self.sink.atom(atom, state)?;
        Ok(())
    }

    fn map(&mut self, state: &DeserializerState) -> Result<Box<dyn MapSink + '_>, Error> {
        self.sink.map(state)
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<Box<dyn SeqSink + '_>, Error> {
        self.sink.seq(state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.expecting()
    }
}
