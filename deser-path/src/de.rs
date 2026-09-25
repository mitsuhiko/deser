use std::borrow::Cow;

use deser::de::{Sink, SinkHandle};
use deser::State;
use deser::{Atom, Descriptor, Error};

use crate::{Path, PathSegment};

enum Container {
    None,
    Map,
    Seq(usize),
}

/// A path sink tracks the current path during deserialization.
pub struct PathSink<'a, 'de> {
    sink: SinkHandle<'a, 'de>,
    container: Container,
    is_key: bool,
    entered_container: bool,
}

impl<'a, 'de> PathSink<'a, 'de> {
    /// Wraps a sink.
    pub fn wrap(sink: &'a mut dyn Sink<'de>) -> PathSink<'a, 'de> {
        PathSink::wrap_ref(SinkHandle::to(sink))
    }

    /// Wraps a sink ref.
    pub fn wrap_ref(sink: SinkHandle<'a, 'de>) -> PathSink<'a, 'de> {
        PathSink::new(sink, false)
    }

    fn new(sink: SinkHandle<'a, 'de>, is_key: bool) -> PathSink<'a, 'de> {
        PathSink {
            sink,
            container: Container::None,
            is_key,
            entered_container: false,
        }
    }

    /// Moves the path to the next index in sequences.
    fn advance_index(&mut self, state: &mut State) {
        if let Container::Seq(ref mut index) = self.container {
            if let Some(segment) = state.get_mut::<Path>().segments.last_mut() {
                *segment = PathSegment::Index(*index);
            }
            *index += 1;
        }
    }

    fn enter_container(&mut self, state: &mut State, container: Container) {
        state.set_replayable::<Path>();
        state.get_mut::<Path>().segments.push(PathSegment::Unknown);
        self.entered_container = true;
        self.container = container;
    }
}

/// Sets the segment of the current container to a key.
///
/// This reuses the allocation of the previous key if possible.
fn set_key(state: &mut State, atom: &Atom) {
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

impl<'a, 'de> Sink<'de> for PathSink<'a, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if self.is_key {
            set_key(state, &atom);
        }
        self.sink.atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        if self.is_key {
            set_key(state, &atom);
        }
        self.sink.borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.enter_container(state, Container::Map);
        self.sink.map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.enter_container(state, Container::Seq(0));
        self.sink.seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let sink = self.sink.next_key(state)?;
        Ok(SinkHandle::boxed(PathSink::new(sink, true)))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.advance_index(state);
        let sink = self.sink.next_value(state)?;
        Ok(SinkHandle::boxed(PathSink::new(sink, false)))
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        // this is what `next_key` followed by `atom` and `finish` on the
        // returned path sink does, without allocating the path sink.
        let mut sink = self.sink.next_key(state)?;
        set_key(state, &atom);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        // this is what `next_value` followed by `atom` and `finish` on the
        // returned path sink does, without allocating the path sink.
        self.advance_index(state);
        self.sink.value_atom(atom, state)
    }

    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let mut sink = self.sink.next_key(state)?;
        set_key(state, &atom);
        sink.borrowed_atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.advance_index(state);
        self.sink.borrowed_value_atom(atom, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = self.sink.finish(state);
        // leave the container this sink entered
        if self.entered_container {
            self.entered_container = false;
            state.get_mut::<Path>().pop();
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
