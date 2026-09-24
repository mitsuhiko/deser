use std::borrow::Cow;

use deser::de::{DeserializerState, Sink, SinkHandle};
use deser::{Atom, Descriptor, Error};

use crate::{Path, PathSegment};

enum Container {
    None,
    Map,
    Seq(usize),
}

/// A path sink tracks the current path during deserialization.
pub struct PathSink<'a> {
    sink: SinkHandle<'a>,
    container: Container,
    is_key: bool,
    entered_container: bool,
}

impl<'a> PathSink<'a> {
    /// Wraps a sink.
    pub fn wrap(sink: &'a mut dyn Sink) -> PathSink<'a> {
        PathSink::wrap_ref(SinkHandle::to(sink))
    }

    /// Wraps a sink ref.
    pub fn wrap_ref(sink: SinkHandle<'a>) -> PathSink<'a> {
        PathSink::new(sink, false)
    }

    fn new(sink: SinkHandle<'a>, is_key: bool) -> PathSink<'a> {
        PathSink {
            sink,
            container: Container::None,
            is_key,
            entered_container: false,
        }
    }

    fn enter_container(&mut self, state: &mut DeserializerState, container: Container) {
        state.set_replayable::<Path>();
        state.get_mut::<Path>().segments.push(PathSegment::Unknown);
        self.entered_container = true;
        self.container = container;
    }
}

/// Sets the segment of the current container to a key.
///
/// This reuses the allocation of the previous key if possible.
fn set_key(state: &mut DeserializerState, atom: &Atom) {
    let segment = match state.get_mut::<Path>().segments.last_mut() {
        Some(segment) => segment,
        None => return,
    };
    match *atom {
        Atom::Str(ref key) => match segment {
            PathSegment::Key(ref mut buf) => {
                buf.clear();
                buf.push_str(key);
            }
            _ => *segment = PathSegment::Key(key.to_string()),
        },
        Atom::U64(value) => *segment = PathSegment::Index(value as usize),
        Atom::I64(value) => *segment = PathSegment::Index(value as usize),
        Atom::Ext(ref ext) => {
            // extension values (like annotated keys) use their fallback
            let fallback = ext.fallback();
            if !matches!(fallback, Atom::Ext(_)) {
                set_key(state, &fallback);
            }
        }
        _ => *segment = PathSegment::Unknown,
    }
}

impl<'a> Sink for PathSink<'a> {
    fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
        if self.is_key {
            set_key(state, &atom);
        }
        self.sink.atom(atom, state)
    }

    fn map(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
        self.enter_container(state, Container::Map);
        self.sink.map(state)
    }

    fn seq(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
        self.enter_container(state, Container::Seq(0));
        self.sink.seq(state)
    }

    fn next_key(&mut self, state: &mut DeserializerState) -> Result<SinkHandle<'_>, Error> {
        let sink = self.sink.next_key(state)?;
        Ok(SinkHandle::boxed(PathSink::new(sink, true)))
    }

    fn next_value(&mut self, state: &mut DeserializerState) -> Result<SinkHandle<'_>, Error> {
        if let Container::Seq(ref mut index) = self.container {
            if let Some(segment) = state.get_mut::<Path>().segments.last_mut() {
                *segment = PathSegment::Index(*index);
            }
            *index += 1;
        }
        let sink = self.sink.next_value(state)?;
        Ok(SinkHandle::boxed(PathSink::new(sink, false)))
    }

    fn finish(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
        let rv = self.sink.finish(state);
        // leave the container this sink entered
        if self.entered_container {
            self.entered_container = false;
            state.get_mut::<Path>().segments.pop();
        }
        rv
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.sink.descriptor()
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.expecting()
    }
}
