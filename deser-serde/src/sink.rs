//! Sinks that collect the events of a value for serde.
use std::borrow::Cow;

use deser::de::{Sink, SinkHandle};
use deser::{Atom, Event, State};

use crate::de::{Single, ValueDe};
use crate::error::Error;

/// Receives the events of a value.
pub(crate) trait Push<'de> {
    fn push(&mut self, event: Event<'de>, state: &State) -> Result<(), deser::Error>;
}

/// Turns the events of a map or sequence into a serde value.
pub(crate) trait Collector<'de, T>: Push<'de> {
    /// Begins the value with its first event.
    fn begin(&mut self, event: Event<'de>, key: bool, state: &State) -> Result<(), deser::Error>;

    /// Returns the value after the last event.
    fn finish(&mut self) -> Result<T, deser::Error>;
}

/// Deserializes a serde value from a single atom.
///
/// Most serde values that are used with deser are atoms (like `Url` or
/// `IpAddr`), these are deserialized directly.
fn deserialize_atom<'de, T: serde::Deserialize<'de>>(
    atom: Atom<'de>,
    key: bool,
) -> Result<T, deser::Error> {
    let mut src = Single(Some(Event::Atom(atom)));
    T::deserialize(ValueDe::new(&mut src, key)).map_err(Error::into_deser)
}

/// The sink for a serde value.
pub(crate) struct RootSink<'a, T, C> {
    out: &'a mut Option<T>,
    collector: C,
    end: Option<Event<'static>>,
}

impl<'a, T, C> RootSink<'a, T, C> {
    pub(crate) fn new(out: &'a mut Option<T>, collector: C) -> RootSink<'a, T, C> {
        RootSink {
            out,
            collector,
            end: None,
        }
    }
}

impl<'a, 'de, T, C> Sink<'de> for RootSink<'a, T, C>
where
    T: serde::Deserialize<'de>,
    C: Collector<'de, T>,
{
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
        *self.out = Some(deserialize_atom(atom.to_static(), state.is_map_key())?);
        Ok(())
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), deser::Error> {
        *self.out = Some(deserialize_atom(atom, state.is_map_key())?);
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), deser::Error> {
        let event = Event::MapStart(state.container_shape());
        self.collector.begin(event, state.is_map_key(), state)?;
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), deser::Error> {
        let event = Event::SeqStart(state.container_shape());
        self.collector.begin(event, state.is_map_key(), state)?;
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, deser::Error> {
        Ok(ChildSink::handle(&mut self.collector))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, deser::Error> {
        Ok(ChildSink::handle(&mut self.collector))
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom.to_static()), state)
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom.to_static()), state)
    }

    fn borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom), state)
    }

    fn borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom), state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), deser::Error> {
        if let Some(end) = self.end.take() {
            self.collector.push(end, state)?;
            *self.out = Some(self.collector.finish()?);
        }
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("serde value")
    }
}

/// The sink for the values within a serde value.
struct ChildSink<'b, C: ?Sized> {
    collector: &'b mut C,
    end: Option<Event<'static>>,
}

impl<'b, C: ?Sized> ChildSink<'b, C> {
    fn handle<'de>(collector: &'b mut C) -> SinkHandle<'b, 'de>
    where
        C: Push<'de>,
    {
        SinkHandle::boxed(ChildSink {
            collector,
            end: None,
        })
    }
}

impl<'b, 'de, C: Push<'de> + ?Sized> Sink<'de> for ChildSink<'b, C> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom.to_static()), state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom), state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), deser::Error> {
        self.collector
            .push(Event::MapStart(state.container_shape()), state)?;
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), deser::Error> {
        self.collector
            .push(Event::SeqStart(state.container_shape()), state)?;
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, deser::Error> {
        Ok(ChildSink::handle(&mut *self.collector))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, deser::Error> {
        Ok(ChildSink::handle(&mut *self.collector))
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom.to_static()), state)
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom.to_static()), state)
    }

    fn borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom), state)
    }

    fn borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), deser::Error> {
        self.collector.push(Event::Atom(atom), state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), deser::Error> {
        match self.end.take() {
            Some(end) => self.collector.push(end, state),
            None => Ok(()),
        }
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("serde value")
    }
}
