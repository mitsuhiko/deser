use crate::de::{DeserializeDriver, Sink, SinkHandle};
use crate::error::Error;
use crate::event::{Atom, Event};
use crate::extensions::Snapshot;
use crate::State;

/// A recorded value that can be replayed into a sink later.
///
/// Some types cannot deserialize a value when it arrives because they first
/// need to see data that comes later.  An example are internally tagged enums
/// where the tag can come after the fields of the variant.  Such types record
/// values and replay them once they know where they should go.
///
/// A recording captures the events of a value together with the values of
/// all replayable extensions in the state (see
/// [`State::set_replayable`]) at the time of each event.  When
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
///     driver.emit(Event::SeqStart).unwrap();
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
#[derive(Debug, Clone, Default)]
pub struct Recording {
    events: Vec<(Event<'static>, Snapshot)>,
    is_map_key: bool,
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
    pub fn recorder(&mut self) -> SinkHandle<'_> {
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
    /// impl Deserialize for NumberOrString {
    ///     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
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
    ///         for event in [deser::Event::SeqStart, 42u64.into(), "x".into(), deser::Event::SeqEnd] {
    ///             driver.emit(event).unwrap();
    ///         }
    ///     }
    ///     out.unwrap()
    /// };
    /// assert_eq!(values, [NumberOrString::Number(42), NumberOrString::String("x".into())]);
    /// ```
    pub fn capture<'a, F>(then: F) -> SinkHandle<'a>
    where
        F: FnOnce(Recording, &mut State) -> Result<(), Error> + 'a,
    {
        SinkHandle::boxed(CaptureSink {
            recording: Recording::new(),
            end: None,
            then: Some(Box::new(then)),
        })
    }

    /// Returns `true` if nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the recorded events.
    pub fn events(&self) -> impl Iterator<Item = &Event<'static>> {
        self.events.iter().map(|(event, _)| event)
    }

    /// Returns the value if the recording is a single string.
    ///
    /// This is useful to look at recorded map keys.
    pub fn as_str(&self) -> Option<&str> {
        match self.events.as_slice() {
            [(Event::Atom(Atom::Str(value)), _)] => Some(value),
            _ => None,
        }
    }

    /// Replays the recorded value into a sink.
    ///
    /// The state is the state of the ongoing deserialization.  The replayable
    /// extensions in it are restored to their current values after replaying.
    pub fn replay(&self, sink: SinkHandle<'_>, state: &mut State) -> Result<(), Error> {
        let live = state.extensions().snapshot();
        let rv = self.replay_events(sink, state);
        state.extensions_mut().restore(&live);
        rv
    }

    fn replay_events(&self, sink: SinkHandle<'_>, state: &mut State) -> Result<(), Error> {
        DeserializeDriver::nested(state, sink, self.is_map_key, |driver| {
            for (event, snapshot) in self.events.iter() {
                driver.state_mut().extensions_mut().restore(snapshot);
                driver.emit(event.as_borrowed())?;
            }
            Ok(())
        })
    }
}

fn record(recording: &mut Recording, is_root: bool, event: Event<'static>, state: &State) {
    if is_root && recording.events.is_empty() {
        recording.is_map_key = state.is_map_key();
    }
    recording
        .events
        .push((event, state.extensions().snapshot()));
}

type CaptureCallback<'a> = Box<dyn FnOnce(Recording, &mut State) -> Result<(), Error> + 'a>;

/// Records a value into an owned recording and invokes a callback with it.
struct CaptureSink<'a> {
    recording: Recording,
    end: Option<Event<'static>>,
    then: Option<CaptureCallback<'a>>,
}

impl<'a> CaptureSink<'a> {
    fn child(&mut self) -> SinkHandle<'_> {
        SinkHandle::boxed(Recorder {
            recording: &mut self.recording,
            end: None,
            is_root: false,
        })
    }
}

impl<'a> Sink for CaptureSink<'a> {
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
        record(&mut self.recording, true, Event::MapStart, state);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        record(&mut self.recording, true, Event::SeqStart, state);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        Ok(self.child())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
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

    fn child(&mut self) -> SinkHandle<'_> {
        SinkHandle::boxed(Recorder {
            recording: self.recording,
            end: None,
            is_root: false,
        })
    }
}

impl<'a> Sink for Recorder<'a> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.record(Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.record(Event::MapStart, state);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.record(Event::SeqStart, state);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        Ok(self.child())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        Ok(self.child())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some(end) = self.end.take() {
            self.record(end, state);
        }
        Ok(())
    }
}
