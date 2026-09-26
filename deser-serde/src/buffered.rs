//! Bridges serde by buffering the events of a value.
use std::iter::Peekable;

use deser::ser::{Chunk, SerializeHandle};
use deser::{Atom, ErrorKind, Event, State};

use crate::de::{Source, ValueDe, unexpected_end};
use crate::error::Error;
use crate::ser::{Emit, EventSerializer, Events};
use crate::sink::{Collector, Push};

struct Recorded<'de> {
    event: Event<'de>,
    offset: Option<usize>,
}

/// Collects the events of a value and deserializes it at the end.
#[derive(Default)]
pub(crate) struct Buffer<'de> {
    events: Vec<Recorded<'de>>,
}

impl<'de> Push<'de> for Buffer<'de> {
    fn push(&mut self, event: Event<'de>, state: &State) -> Result<(), deser::Error> {
        self.events.push(Recorded {
            event,
            offset: state.input_range().map(|range| range.start),
        });
        Ok(())
    }
}

impl<'de, T: serde::Deserialize<'de>> Collector<'de, T> for Buffer<'de> {
    fn begin(&mut self, event: Event<'de>, state: &State) -> Result<(), deser::Error> {
        self.push(event, state)
    }

    fn finish(&mut self) -> Result<T, deser::Error> {
        let mut src = BufferSource {
            events: std::mem::take(&mut self.events).into_iter().peekable(),
            offset: None,
        };
        T::deserialize(ValueDe::new(&mut src)).map_err(|err| {
            // the error refers to the event that was consumed last.  The
            // driver would attach the location of the end of the value.
            let err = err.into_deser();
            match (err.offset(), src.offset) {
                (None, Some(offset)) => err.with_offset(offset),
                _ => err,
            }
        })
    }
}

struct BufferSource<'de> {
    events: Peekable<std::vec::IntoIter<Recorded<'de>>>,
    offset: Option<usize>,
}

impl<'de> Source<'de> for BufferSource<'de> {
    fn next(&mut self) -> Result<Event<'de>, Error> {
        let recorded = self.events.next().ok_or_else(unexpected_end)?;
        self.offset = recorded.offset;
        Ok(recorded.event)
    }

    fn peek(&mut self) -> Result<&Event<'de>, Error> {
        match self.events.peek() {
            Some(recorded) => Ok(&recorded.event),
            None => Err(unexpected_end()),
        }
    }
}

/// The events of a serialized value.
///
/// Atoms are kept without allocating a vector as most values are atoms.
#[derive(Default)]
enum SerBuffer {
    #[default]
    Empty,
    Atom(Atom<'static>),
    Events(Vec<Event<'static>>),
}

impl Emit for SerBuffer {
    fn emit(&mut self, event: Event<'_>) -> Result<(), Error> {
        match self {
            SerBuffer::Empty => {
                *self = match event {
                    Event::Atom(atom) => SerBuffer::Atom(atom.to_static()),
                    event => SerBuffer::Events(vec![event.to_static()]),
                };
                Ok(())
            }
            SerBuffer::Events(events) => {
                events.push(event.to_static());
                Ok(())
            }
            SerBuffer::Atom(_) => Err(Error::new(
                ErrorKind::Unexpected,
                "serde serializer produced more than one value",
            )),
        }
    }
}

/// Serializes a serde value by buffering its events.
pub(crate) fn serialize<T: serde::Serialize + ?Sized>(
    value: &T,
) -> Result<Chunk<'static>, deser::Error> {
    let mut buffer = SerBuffer::Empty;
    value
        .serialize(EventSerializer::new(&mut buffer))
        .map_err(Error::into_deser)?;
    match buffer {
        SerBuffer::Empty => Err(deser::Error::new(
            ErrorKind::Unexpected,
            "serde serializer produced no value",
        )),
        SerBuffer::Atom(atom) => Ok(Chunk::Atom(atom)),
        SerBuffer::Events(events) => {
            Ok(Chunk::Forward(SerializeHandle::boxed(Events::new(events)?)))
        }
    }
}
