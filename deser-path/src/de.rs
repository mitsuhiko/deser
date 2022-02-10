use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deser::de::{DeserializerState, Sink, SinkHandle};
use deser::{Atom, Descriptor, Error};

use crate::{Path, PathSegment};

enum Container {
    None,
    Map(Rc<RefCell<Option<PathSegment>>>),
    Seq(usize),
}

/// A path sink tracks the current path during deserialization.
pub struct PathSink<'a> {
    sink: SinkHandle<'a>,
    container: Container,
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
            container: Container::None,
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
        if let Container::Map(ref capture) = self.container {
            *capture.borrow_mut() = match atom {
                Atom::Str(ref value) => Some(PathSegment::Key(value.to_string())),
                Atom::U64(value) => Some(PathSegment::Index(value as usize)),
                Atom::I64(value) => Some(PathSegment::Index(value as usize)),
                _ => None,
            };
        }
        self.sink.atom(atom, state)
    }

    fn map(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        state.get_mut::<Path>().segments.push(PathSegment::Unknown);
        self.container = Container::Map(Rc::default());
        self.sink.map(state)
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.set_segment(state);
        state.get_mut::<Path>().segments.push(PathSegment::Unknown);
        self.container = Container::Seq(0);
        self.sink.seq(state)
    }

    fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
        self.sink.next_key(state).map(|sink| {
            SinkHandle::boxed(PathSink {
                sink,
                container: match self.container {
                    Container::Map(ref capture) => Container::Map(capture.clone()),
                    _ => unreachable!(),
                },
                set_segment: None,
            })
        })
    }

    fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
        let set_segment = match self.container {
            Container::None => None,
            Container::Map(ref captured_key) => captured_key.borrow_mut().take(),
            Container::Seq(ref mut index) => {
                let old_index = *index;
                *index += 1;
                Some(PathSegment::Index(old_index))
            }
        };
        self.sink.next_value(state).map(|sink| {
            SinkHandle::boxed(PathSink {
                sink,
                container: Container::None,
                set_segment,
            })
        })
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.finish(state)
    }

    fn descriptor(&self) -> &dyn Descriptor {
        self.sink.descriptor()
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.expecting()
    }
}
