use crate::State;
use crate::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ContainerShape, Event};
use crate::extensions::Snapshot;
use crate::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};

/// A recorded value that can be replayed into a sink later.
///
/// Some types cannot deserialize a value when it arrives because they first
/// need to see data that comes later.  An example are internally tagged enums
/// where the tag can come after the fields of the variant.  Such types record
/// values and replay them once they know where they should go.
///
/// A recording captures the events of a value together with their input
/// ranges (see [`State::input_range`]), the data attached to them (see
/// [`State::event`]) and the values of all replayable extensions in the
/// state (see [`State::set_replayable`]) at the time of each event.  When
/// replaying, these values are restored for every event.  This means that
/// information such as source locations or paths remains correct for replayed
/// values.  Map keys are also replayed as map keys, so format specific key
/// handling (like integer keys in JSON) continues to work.
///
/// ```
/// use deser::de::{DeserializeDriver, Recording};
/// use deser::{Deserialize, Event};
///
/// let mut recording = Recording::new();
/// {
///     let mut driver = DeserializeDriver::from_sink(recording.recorder());
///     driver.emit(Event::seq_start()).unwrap();
///     driver.emit(1u64).unwrap();
///     driver.emit(2u64).unwrap();
///     driver.emit(Event::SeqEnd).unwrap();
/// }
///
/// let mut out = None::<Vec<u32>>;
/// {
///     let mut driver_out = None::<()>;
///     let mut driver = DeserializeDriver::new(&mut driver_out);
///     recording
///         .replay(Deserialize::deserialize_into(&mut out), driver.state_mut())
///         .unwrap();
/// }
/// assert_eq!(out, Some(vec![1, 2]));
/// ```
///
/// Recordings are detached from the data they were recorded from: borrowed
/// atoms are recorded as owned (see [`Atom::to_static`]).  This means that
/// types which only accept borrowed data (like `&str`) cannot be
/// deserialized from a replayed recording.  Types which can hold owned data
/// (like `Cow<str>`) can.
///
/// # Raw Values
///
/// Recordings implement [`Deserialize`] and [`Serialize`].  This makes them
/// usable as raw values that capture any value without interpreting it, for
/// instance for the content of `#[deser(other)]` enum variants.  When
/// serialized the recorded events are emitted again, including the event
/// data attached to them (see [`State::event`]).  This means that format
/// specific information carried as event data (for instance CBOR tags)
/// survives a round trip through a recording.
///
/// ```
/// use deser::de::Recording;
/// use deser::Deserialize;
///
/// #[derive(Deserialize)]
/// pub struct Envelope {
///     kind: String,
///     payload: Recording,
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct Recording {
    events: Vec<RecordedEvent>,
    is_map_key: bool,
}

#[derive(Debug, Clone)]
struct RecordedEvent {
    event: Event<'static>,
    input_range: (usize, usize),
    snapshot: Snapshot,
}

// recordings are stored in sinks, they must not prevent them from moving
// between threads.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Recording>();
};

impl Recording {
    /// Creates an empty recording.
    pub fn new() -> Recording {
        Recording::default()
    }

    /// Returns a sink that records a value into this recording.
    ///
    /// A previously recorded value is discarded.
    pub fn recorder<'de>(&mut self) -> SinkHandle<'_, 'de> {
        self.events.clear();
        self.is_map_key = false;
        SinkHandle::boxed(Recorder {
            recording: self,
            end: None,
            is_root: true,
        })
    }

    /// Returns a sink that records a value and passes the recording to a
    /// callback once the value is complete.
    ///
    /// This is useful for types which need to see the complete value before
    /// they can deserialize it, like untagged enums which try to replay the
    /// value into different types.
    ///
    /// ```
    /// use deser::de::{Deserialize, Recording, SinkHandle};
    ///
    /// /// Deserializes either as number or as string.
    /// #[derive(Debug, PartialEq)]
    /// enum NumberOrString {
    ///     Number(u64),
    ///     String(String),
    /// }
    ///
    /// impl<'de> Deserialize<'de> for NumberOrString {
    ///     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
    ///         Recording::capture(move |recording, state| {
    ///             let mut number = None;
    ///             if recording.replay(u64::deserialize_into(&mut number), state).is_ok() {
    ///                 *out = number.map(NumberOrString::Number);
    ///             } else {
    ///                 let mut string = None;
    ///                 recording.replay(String::deserialize_into(&mut string), state)?;
    ///                 *out = string.map(NumberOrString::String);
    ///             }
    ///             Ok(())
    ///         })
    ///     }
    /// }
    ///
    /// let values: Vec<NumberOrString> = {
    ///     let mut out = None;
    ///     {
    ///         let mut driver = deser::de::DeserializeDriver::new(&mut out);
    ///         for event in [deser::Event::seq_start(), 42u64.into(), "x".into(), deser::Event::SeqEnd] {
    ///             driver.emit(event).unwrap();
    ///         }
    ///     }
    ///     out.unwrap()
    /// };
    /// assert_eq!(values, [NumberOrString::Number(42), NumberOrString::String("x".into())]);
    /// ```
    pub fn capture<'a, 'de, F>(then: F) -> SinkHandle<'a, 'de>
    where
        F: FnOnce(Recording, &mut State) -> Result<(), Error> + Send + 'a,
    {
        SinkHandle::boxed(CaptureSink {
            recording: Recording::new(),
            end: None,
            then: Some(Box::new(then)),
        })
    }

    /// Records a single atom, discarding a previously recorded value.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub(crate) fn set_atom(&mut self, atom: &Atom<'_>, state: &State) {
        self.events.clear();
        self.is_map_key = false;
        record(self, true, Event::Atom(atom.to_static()), state);
    }

    /// Returns `true` if nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the recorded events.
    pub fn events(&self) -> impl Iterator<Item = &Event<'static>> {
        self.events.iter().map(|recorded| &recorded.event)
    }

    /// Returns the value if the recording is a single string.
    ///
    /// This is useful to look at recorded map keys.
    pub fn as_str(&self) -> Option<&str> {
        match self.events.as_slice() {
            [
                RecordedEvent {
                    event: Event::Atom(atom),
                    ..
                },
            ] => atom.as_str(),
            _ => None,
        }
    }

    /// Replays the recorded value into a sink.
    ///
    /// The state is the state of the ongoing deserialization.  The replayable
    /// extensions in it are restored to their current values after replaying.
    pub fn replay<'de>(&self, sink: SinkHandle<'_, 'de>, state: &mut State) -> Result<(), Error> {
        let live = state.extensions().snapshot();
        let live_range = state.input_range;
        let rv = self.replay_events(sink, state);
        state.extensions_mut().restore(&live);
        state.input_range = live_range;
        rv
    }

    fn replay_events<'de>(
        &self,
        sink: SinkHandle<'_, 'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        DeserializeDriver::nested(state, sink, self.is_map_key, |driver| {
            for recorded in self.events.iter() {
                let state = driver.state_mut();
                state.input_range = recorded.input_range;
                state.extensions_mut().restore(&recorded.snapshot);
                driver.emit(recorded.event.as_borrowed())?;
            }
            Ok(())
        })
    }
}

fn record(recording: &mut Recording, is_root: bool, event: Event<'static>, state: &State) {
    if is_root && recording.events.is_empty() {
        recording.is_map_key = state.is_map_key();
    }
    recording.events.push(RecordedEvent {
        event,
        input_range: state.input_range,
        snapshot: state.extensions().snapshot(),
    });
}

type CaptureCallback<'a> = Box<dyn FnOnce(Recording, &mut State) -> Result<(), Error> + Send + 'a>;

/// Records a value into an owned recording and invokes a callback with it.
struct CaptureSink<'a> {
    recording: Recording,
    end: Option<Event<'static>>,
    then: Option<CaptureCallback<'a>>,
}

impl<'a> CaptureSink<'a> {
    fn child<'de>(&mut self) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(Recorder {
            recording: &mut self.recording,
            end: None,
            is_root: false,
        })
    }
}

impl<'a, 'de> Sink<'de> for CaptureSink<'a> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        record(
            &mut self.recording,
            true,
            Event::Atom(atom.to_static()),
            state,
        );
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let shape = state.container_shape();
        record(&mut self.recording, true, Event::MapStart(shape), state);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        let shape = state.container_shape();
        record(&mut self.recording, true, Event::SeqStart(shape), state);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some(end) = self.end.take() {
            record(&mut self.recording, true, end, state);
        }
        match self.then.take() {
            Some(then) => then(std::mem::take(&mut self.recording), state),
            None => Ok(()),
        }
    }
}

/// Records a single value into a recording.
struct Recorder<'a> {
    recording: &'a mut Recording,
    end: Option<Event<'static>>,
    is_root: bool,
}

impl<'a> Recorder<'a> {
    fn record(&mut self, event: Event<'static>, state: &State) {
        record(self.recording, self.is_root, event, state);
    }

    fn child<'de>(&mut self) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(Recorder {
            recording: self.recording,
            end: None,
            is_root: false,
        })
    }
}

impl<'a, 'de> Sink<'de> for Recorder<'a> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.record(Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.record(Event::MapStart(state.container_shape()), state);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.record(Event::SeqStart(state.container_shape()), state);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some(end) = self.end.take() {
            self.record(end, state);
        }
        Ok(())
    }
}

/// Recordings are compared by their events.
///
/// The event data and the replayable extensions captured with the events
/// are not compared.
impl PartialEq for Recording {
    fn eq(&self, other: &Self) -> bool {
        self.events.len() == other.events.len()
            && self
                .events
                .iter()
                .zip(other.events.iter())
                .all(|(a, b)| a.event == b.event)
    }
}

impl<'de> Deserialize<'de> for Recording {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        Recording::capture(move |recording, _state| {
            *out = Some(recording);
            Ok(())
        })
    }
}

/// Returns the number of events of the value the events start with.
fn value_len(events: &[RecordedEvent]) -> usize {
    let mut depth = 0usize;
    for (index, recorded) in events.iter().enumerate() {
        match recorded.event {
            Event::MapStart(_) | Event::SeqStart(_) => depth += 1,
            Event::MapEnd | Event::SeqEnd => depth = depth.saturating_sub(1),
            Event::Atom(_) => {}
        }
        if depth == 0 {
            return index + 1;
        }
    }
    events.len()
}

/// A recorded value that is serialized.
struct RecordedValue<'a>(&'a [RecordedEvent]);

impl<'a> RecordedValue<'a> {
    fn chunk(&self, state: &mut State) -> Result<Chunk<'a>, Error> {
        let events = self.0;
        let (first, snapshot) = match events.first() {
            Some(first) => (&first.event, &first.snapshot),
            None => {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "cannot serialize an empty recording",
                ));
            }
        };
        state
            .extensions_mut()
            .restore_event_data(snapshot.event_data());
        let inner = events.get(1..events.len().saturating_sub(1)).unwrap_or(&[]);
        Ok(match first {
            Event::Atom(atom) => Chunk::Atom(atom.as_borrowed()),
            Event::MapStart(_) => Chunk::Map(Box::new(RecordedEmitter {
                rest: inner,
                current: RecordedValue(&[]),
            })),
            Event::SeqStart(_) => Chunk::Seq(Box::new(RecordedEmitter {
                rest: inner,
                current: RecordedValue(&[]),
            })),
            Event::MapEnd | Event::SeqEnd => {
                return Err(Error::new(ErrorKind::Unexpected, "malformed recording"));
            }
        })
    }
}

impl<'a> RecordedValue<'a> {
    fn container_shape(&self) -> ContainerShape {
        match self.0.first().map(|x| &x.event) {
            Some(Event::MapStart(shape) | Event::SeqStart(shape)) => *shape,
            _ => ContainerShape::new(),
        }
    }
}

impl<'a> Serialize for RecordedValue<'a> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.chunk(state)
    }

    fn container_shape(&self) -> ContainerShape {
        RecordedValue::container_shape(self)
    }
}

/// Emits the values of a recorded map or sequence.
struct RecordedEmitter<'a> {
    rest: &'a [RecordedEvent],
    current: RecordedValue<'a>,
}

impl<'a> RecordedEmitter<'a> {
    fn next_value(&mut self) -> Option<SerializeHandle<'_>> {
        if self.rest.is_empty() {
            return None;
        }
        let len = value_len(self.rest);
        let (value, rest) = self.rest.split_at(len);
        self.current = RecordedValue(value);
        self.rest = rest;
        Some(SerializeHandle::to(&self.current))
    }
}

impl<'a> SeqEmitter for RecordedEmitter<'a> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.next_value())
    }
}

impl<'a> MapEmitter for RecordedEmitter<'a> {
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.next_value())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        RecordedEmitter::next_value(self)
            .ok_or_else(|| Error::new(ErrorKind::Unexpected, "malformed recording"))
    }
}

impl Serialize for Recording {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        RecordedValue(&self.events).chunk(state)
    }

    fn container_shape(&self) -> ContainerShape {
        RecordedValue(&self.events).container_shape()
    }

    fn is_optional(&self) -> bool {
        matches!(
            self.events.as_slice(),
            [RecordedEvent {
                event: Event::Atom(Atom::Null),
                ..
            }]
        )
    }
}
