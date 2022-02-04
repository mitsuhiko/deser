use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deser::de::{DeserializerState, MapSink, SeqSink, Sink, SinkRef};
use deser::Error;

use crate::{Path, PathSegment};

/// A path sink tracks the current path during deserialization.
pub struct PathSink<'a> {
    sink: SinkRef<'a>,
    set_segment: Option<PathSegment>,
}

impl<'a> PathSink<'a> {
    /// Wraps a sink.
    pub fn wrap(sink: &'a mut dyn Sink) -> PathSink<'a> {
        PathSink::wrap_ref(SinkRef::Borrowed(sink))
    }

    /// Wraps a sink ref.
    pub fn wrap_ref(sink: SinkRef<'a>) -> PathSink<'a> {
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
    fn null(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.null(state)
    }

    fn bool(&mut self, value: bool, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.bool(value, state)
    }

    fn str(&mut self, value: &str, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.str(value, state)
    }

    fn bytes(&mut self, value: &[u8], state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.bytes(value, state)
    }

    fn u64(&mut self, value: u64, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.u64(value, state)
    }

    fn i64(&mut self, value: i64, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.i64(value, state)
    }

    fn f64(&mut self, value: f64, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        self.sink.f64(value, state)
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
    fn key(&mut self) -> Result<SinkRef, Error> {
        Ok(SinkRef::Owned(Box::new(KeyCapturingSink {
            sink: self.sink.key()?,
            captured_segment: self.captured_segment.clone(),
        })))
    }

    fn value(&mut self) -> Result<SinkRef, Error> {
        Ok(SinkRef::Owned(Box::new(PathSink {
            sink: self.sink.value()?,
            set_segment: self.captured_segment.take(),
        })))
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
    fn item(&mut self) -> Result<SinkRef, Error> {
        let sink_wrapper = PathSink {
            sink: self.sink.item()?,
            set_segment: Some(PathSegment::Index(self.index)),
        };
        Ok(SinkRef::Owned(Box::new(sink_wrapper)))
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        state.get_mut::<Path>().segments.pop();
        self.sink.finish(state)
    }
}

struct KeyCapturingSink<'a> {
    sink: SinkRef<'a>,
    captured_segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> Sink for KeyCapturingSink<'a> {
    fn null(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.null(state)
    }

    fn bool(&mut self, value: bool, state: &DeserializerState) -> Result<(), Error> {
        self.sink.bool(value, state)
    }

    fn str(&mut self, value: &str, state: &DeserializerState) -> Result<(), Error> {
        *self.captured_segment.borrow_mut() = Some(PathSegment::Key(value.to_string()));
        self.sink.str(value, state)
    }

    fn bytes(&mut self, value: &[u8], state: &DeserializerState) -> Result<(), Error> {
        self.sink.bytes(value, state)
    }

    fn u64(&mut self, value: u64, state: &DeserializerState) -> Result<(), Error> {
        *self.captured_segment.borrow_mut() = Some(PathSegment::Index(value as usize));
        self.sink.u64(value, state)
    }

    fn i64(&mut self, value: i64, state: &DeserializerState) -> Result<(), Error> {
        *self.captured_segment.borrow_mut() = Some(PathSegment::Index(value as usize));
        self.sink.i64(value, state)
    }

    fn f64(&mut self, value: f64, state: &DeserializerState) -> Result<(), Error> {
        self.sink.f64(value, state)
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
