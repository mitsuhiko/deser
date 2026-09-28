use crate::State;
use crate::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ContainerShape, Event};
use crate::extensions::Snapshot;
use crate::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::vec::Vec;

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
/// values.  Map keys are also replayed as map keys and atoms are recorded as
/// they are (lexical atoms remain lexical), so format specific handling (like
/// integer keys in JSON) continues to work.
///
/// ```
/// use deser::de::{DeserializeDriver, Recording};
/// use deser::{Deserialize, Event};
///
/// let mut recording = Recording::new();
/// {
///     let mut driver =
///         DeserializeDriver::from_fn(|state| recording.recorder(state));
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
///     let state = driver.state_mut();
///     recording
///         .replay(Deserialize::deserialize_into(&mut out, state), state)
///         .unwrap();
/// }
/// assert_eq!(out, Some(vec![1, 2]));
/// ```
///
/// Recordings are detached from the data they were recorded from: borrowed
/// atoms are recorded as owned (see [`Atom::to_static`]).  This means that
/// types which only accept borrowed data (like `&str`) cannot be
/// deserialized from a replayed recording.  Types which can hold owned data
/// (like `Cow<str>`) can.  (The buffering of the derive, for instance for
/// internally tagged enums, keeps borrowed data borrowed.)
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
#[derive(Clone, Default)]
pub struct Recording(RecordBuf<'static>);

/// A recording which keeps borrowed data borrowed.
///
/// Atoms that are delivered borrowed (see
/// [`Sink::borrowed_atom`]) are recorded as they are and replayed borrowed,
/// all others are recorded as owned.  This is what [`Recording`] uses
/// (with owned data only) and what the derive uses to buffer values, which
/// allows types that borrow to be deserialized from buffered values (for
/// instance the fields of internally tagged enums that come before the
/// tag).  Not public API.
#[derive(Clone, Default)]
pub struct RecordBuf<'de> {
    events: Events<'de>,
    is_map_key: bool,
}

/// The recorded events.
///
/// Most recordings are a single atom (like the keys that tagged enums
/// record until the variant is known), which is stored without an
/// allocation.
#[derive(Clone)]
enum Events<'de> {
    Inline(Option<RecordedEvent<'de>>),
    Heap(Vec<RecordedEvent<'de>>),
}

impl Default for Events<'_> {
    fn default() -> Self {
        Events::Inline(None)
    }
}

impl<'de> Events<'de> {
    #[inline]
    fn as_slice(&self) -> &[RecordedEvent<'de>] {
        match self {
            Events::Inline(None) => &[],
            Events::Inline(Some(event)) => core::slice::from_ref(event),
            Events::Heap(events) => events,
        }
    }

    #[inline]
    fn push(&mut self, event: RecordedEvent<'de>) {
        match self {
            Events::Inline(slot @ None) => *slot = Some(event),
            Events::Heap(events) => events.push(event),
            Events::Inline(Some(_)) => {
                self.reserve(CONTAINER_CAPACITY);
                if let Events::Heap(events) = self {
                    events.push(event);
                }
            }
        }
    }

    #[inline]
    fn clear(&mut self) {
        match self {
            Events::Inline(slot) => *slot = None,
            // the capacity is kept for the next recording
            Events::Heap(events) => events.clear(),
        }
    }

    /// Reserves space for more events, the events are stored on the heap
    /// from then on.
    fn reserve(&mut self, additional: usize) {
        match self {
            Events::Inline(slot) => {
                let mut events = Vec::with_capacity(additional + 1);
                events.extend(slot.take());
                *self = Events::Heap(events);
            }
            Events::Heap(events) => events.reserve(additional),
        }
    }
}

impl<'de> core::ops::Deref for Events<'de> {
    type Target = [RecordedEvent<'de>];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl core::fmt::Debug for Events<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_slice(), f)
    }
}

#[derive(Debug, Clone)]
struct RecordedEvent<'de> {
    event: Event<'de>,
    // the atom was delivered borrowed and is replayed borrowed
    borrowed: bool,
    input_range: (usize, usize),
    // `None` if the snapshot is empty, which it is unless there are
    // replayable extensions or event data.  This keeps recorded events small.
    snapshot: Option<Box<Snapshot>>,
}

impl RecordedEvent<'_> {
    /// Restores the replayable extensions and the event data of the event.
    fn restore(&self, state: &mut State) {
        match self.snapshot {
            Some(ref snapshot) => state.extensions_mut().restore(snapshot),
            // restoring an empty snapshot only clears the event data
            None => state.clear_event_data(),
        }
    }
}

// recordings are stored in sinks, they must not prevent them from moving
// between threads.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Recording>();
    assert_send::<RecordBuf<'static>>();
};

impl core::fmt::Debug for Recording {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Recording")
            .field("events", &self.0.events)
            .field("is_map_key", &self.0.is_map_key)
            .finish()
    }
}

impl core::fmt::Debug for RecordBuf<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RecordBuf")
            .field("events", &self.events)
            .field("is_map_key", &self.is_map_key)
            .finish()
    }
}

/// Where a recorder records to.
///
/// Recordings keep owned data only, record buffers keep borrowed data.
trait Target<'de>: Send {
    /// Records an event.
    fn push(&mut self, is_root: bool, event: Event<'static>, state: &State);

    /// Records an atom that was delivered borrowed.
    fn push_borrowed(&mut self, is_root: bool, atom: Atom<'de>, state: &State);

    fn is_empty(&self) -> bool;

    /// Reserves space for the events of a container.
    fn reserve(&mut self);
}

/// The number of events reserved for a recorded container.
const CONTAINER_CAPACITY: usize = 16;

impl<'de> Target<'de> for RecordBuf<'de> {
    fn push(&mut self, is_root: bool, event: Event<'static>, state: &State) {
        record(self, is_root, event, false, state);
    }

    fn push_borrowed(&mut self, is_root: bool, atom: Atom<'de>, state: &State) {
        record(self, is_root, Event::Atom(atom), true, state);
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn reserve(&mut self) {
        self.events.reserve(CONTAINER_CAPACITY);
    }
}

impl<'de> Target<'de> for Recording {
    fn push(&mut self, is_root: bool, event: Event<'static>, state: &State) {
        record(&mut self.0, is_root, event, false, state);
    }

    fn push_borrowed(&mut self, is_root: bool, atom: Atom<'de>, state: &State) {
        let event = Event::Atom(atom.to_static());
        record(&mut self.0, is_root, event, false, state);
    }

    fn is_empty(&self) -> bool {
        self.0.events.is_empty()
    }

    fn reserve(&mut self) {
        self.0.events.reserve(CONTAINER_CAPACITY);
    }
}

impl Recording {
    /// Creates an empty recording.
    pub fn new() -> Recording {
        Recording::default()
    }

    /// Returns a sink that records a value into this recording.
    ///
    /// A previously recorded value is discarded.
    pub fn recorder<'de>(&mut self, state: &mut State) -> SinkHandle<'_, 'de> {
        self.0.events.clear();
        self.0.is_map_key = false;
        SinkHandle::arena(
            Recorder {
                target: self,
                end: None,
                is_root: true,
            },
            state,
        )
    }

    /// Returns a sink that records a value and passes the recording to a
    /// callback once the value is complete.
    ///
    /// This is useful for types which need to see the complete value before
    /// they can deserialize it, like untagged enums which try to replay the
    /// value into different types.
    ///
    /// ```
    /// use deser::State;
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
    ///     fn deserialize_into<'out>(
    ///         out: &'out mut Option<Self>,
    ///         state: &mut State,
    ///     ) -> SinkHandle<'out, 'de> {
    ///         Recording::capture(move |recording, state| {
    ///             let mut number = None;
    ///             let sink = u64::deserialize_into(&mut number, state);
    ///             if recording.replay(sink, state).is_ok() {
    ///                 *out = number.map(NumberOrString::Number);
    ///             } else {
    ///                 let mut string = None;
    ///                 let sink = String::deserialize_into(&mut string, state);
    ///                 recording.replay(sink, state)?;
    ///                 *out = string.map(NumberOrString::String);
    ///             }
    ///             Ok(())
    ///         }, state)
    ///     }
    /// }
    ///
    /// let values: Vec<NumberOrString> = {
    ///     let mut out = None;
    ///     {
    ///         let mut driver = deser::de::DeserializeDriver::new(&mut out);
    ///         for event in [
    ///             deser::Event::seq_start(),
    ///             42u64.into(),
    ///             "x".into(),
    ///             deser::Event::SeqEnd,
    ///         ] {
    ///             driver.emit(event).unwrap();
    ///         }
    ///     }
    ///     out.unwrap()
    /// };
    /// assert_eq!(
    ///     values,
    ///     [NumberOrString::Number(42), NumberOrString::String("x".into())]
    /// );
    /// ```
    pub fn capture<'a, 'de, F>(then: F, state: &mut State) -> SinkHandle<'a, 'de>
    where
        F: FnOnce(Recording, &mut State) -> Result<(), Error> + Send + 'a,
    {
        SinkHandle::arena(
            CaptureSink {
                recording: Recording::new(),
                end: None,
                then: Some(FnCapture(Some(then))),
            },
            state,
        )
    }

    /// Returns `true` if nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the recorded events.
    pub fn events(&self) -> impl Iterator<Item = &Event<'static>> {
        self.0.events.iter().map(|recorded| &recorded.event)
    }

    /// Returns the value if the recording is a single string.
    ///
    /// This is useful to look at recorded map keys.
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// Replays the recorded value into a sink.
    ///
    /// The state is the state of the ongoing deserialization.  The replayable
    /// extensions in it are restored to their current values after replaying.
    pub fn replay<'de>(&self, sink: SinkHandle<'_, 'de>, state: &mut State) -> Result<(), Error> {
        self.0.replay(sink, state)
    }
}

impl<'de> RecordBuf<'de> {
    /// Creates an empty buffer.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub fn new() -> RecordBuf<'de> {
        RecordBuf::default()
    }

    /// Returns a sink that records a value into this buffer.
    ///
    /// A previously recorded value is discarded.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub fn recorder(&mut self, state: &mut State) -> SinkHandle<'_, 'de> {
        self.events.clear();
        self.is_map_key = false;
        SinkHandle::arena(
            Recorder {
                target: self,
                end: None,
                is_root: true,
            },
            state,
        )
    }

    /// Returns a sink that records a value and passes the buffer to a
    /// callback once the value is complete (see [`Recording::capture`]).
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub fn capture<'a, F>(then: F, state: &mut State) -> SinkHandle<'a, 'de>
    where
        F: FnOnce(RecordBuf<'de>, &mut State) -> Result<(), Error> + Send + 'a,
        'de: 'a,
    {
        RecordBuf::capture_with(FnCapture(Some(then)), state)
    }

    /// Returns a sink that captures a value and passes it on.
    ///
    /// Unlike [`capture`](Self::capture) values which are a single atom are
    /// passed on without recording them.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub(crate) fn capture_with<'a, C: Capture<'de, RecordBuf<'de>> + 'a>(
        then: C,
        state: &mut State,
    ) -> SinkHandle<'a, 'de>
    where
        'de: 'a,
    {
        SinkHandle::arena(
            CaptureSink {
                recording: RecordBuf::new(),
                end: None,
                then: Some(then),
            },
            state,
        )
    }

    /// Records a single atom, discarding a previously recorded value.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub(crate) fn set_atom(&mut self, atom: &Atom<'_>, state: &State) {
        self.events.clear();
        self.is_map_key = false;
        record(self, true, Event::Atom(atom.to_static()), false, state);
    }

    /// Returns the start of the input range of the first event.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub(crate) fn offset(&self) -> Option<usize> {
        self.events
            .first()
            .map(|x| x.input_range.0)
            .filter(|&x| x != crate::state::NO_RANGE.0)
    }

    /// Attaches the context of the first event to an error.
    ///
    /// This is for errors about the recorded value that are not returned
    /// while it's replayed.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub(crate) fn attach_context(&self, err: Error, state: &mut State) -> Error {
        let Some(first) = self.events.first() else {
            return state.attach_error_context(err);
        };
        let live = state.extensions().snapshot();
        let live_range = state.input_range;
        state.input_range = first.input_range;
        first.restore(state);
        let err = state.attach_error_context(err);
        state.extensions_mut().restore(&live);
        state.input_range = live_range;
        err
    }

    /// Returns `true` if nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the atom if the buffer is a single atom.
    #[cfg_attr(not(feature = "derive"), allow(dead_code))]
    pub(crate) fn single_atom(&self) -> Option<&Atom<'de>> {
        match self.events.as_slice() {
            [
                RecordedEvent {
                    event: Event::Atom(atom),
                    ..
                },
            ] => Some(atom),
            _ => None,
        }
    }

    /// Returns the value if the buffer is a single string.
    pub fn as_str(&self) -> Option<&str> {
        self.single_atom()?.as_str()
    }

    /// Replays the recorded value into a sink.
    ///
    /// Atoms that were delivered borrowed are replayed borrowed.
    pub fn replay<'a>(&self, sink: SinkHandle<'_, 'a>, state: &mut State) -> Result<(), Error>
    where
        'de: 'a,
    {
        let live = state.extensions().snapshot_if_any();
        let live_range = state.input_range;
        let rv = self.replay_events(sink, state);
        match live {
            Some(live) => state.extensions_mut().restore(&live),
            // restoring an empty snapshot only clears the event data
            None => state.clear_event_data(),
        }
        state.input_range = live_range;
        rv
    }

    fn replay_events<'a>(&self, sink: SinkHandle<'_, 'a>, state: &mut State) -> Result<(), Error>
    where
        'de: 'a,
    {
        DeserializeDriver::nested(state, sink, self.is_map_key, |driver| {
            for recorded in self.events.iter() {
                let state = driver.state_mut();
                state.input_range = recorded.input_range;
                recorded.restore(state);
                match recorded.event {
                    Event::Atom(ref atom) if recorded.borrowed => {
                        driver.emit_borrowed(Event::Atom(atom.clone()))?
                    }
                    ref event => driver.emit(event.as_borrowed())?,
                }
            }
            Ok(())
        })
    }
}

fn record<'de>(
    buf: &mut RecordBuf<'de>,
    is_root: bool,
    event: Event<'de>,
    borrowed: bool,
    state: &State,
) {
    if is_root && buf.events.is_empty() {
        buf.is_map_key = state.is_map_key();
    }
    let recorded = RecordedEvent {
        event,
        borrowed,
        input_range: state.input_range,
        snapshot: state.extensions().snapshot_if_any().map(Box::new),
    };
    // the events of containers go to the vector directly
    match buf.events {
        Events::Heap(ref mut events) => events.push(recorded),
        ref mut events => events.push(recorded),
    }
}

/// Receives a value that was captured (see [`RecordBuf::capture_with`]).
pub(crate) trait Capture<'de, T>: Send {
    /// Receives a value that is a single atom.
    ///
    /// The atom is passed on as it is, without recording it.
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error>;

    /// Receives a value that is a single atom which borrows from the data.
    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.atom(atom, state)
    }

    /// Receives the recording of any other value.
    fn recorded(&mut self, recording: T, state: &mut State) -> Result<(), Error>;
}

/// Passes every captured value to a callback as recording.
struct FnCapture<F>(Option<F>);

impl<'de, T, F> Capture<'de, T> for FnCapture<F>
where
    T: Target<'de> + Default,
    F: FnOnce(T, &mut State) -> Result<(), Error> + Send,
{
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let mut recording = T::default();
        recording.push(true, Event::Atom(atom.to_static()), state);
        self.recorded(recording, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let mut recording = T::default();
        recording.push_borrowed(true, atom, state);
        self.recorded(recording, state)
    }

    fn recorded(&mut self, recording: T, state: &mut State) -> Result<(), Error> {
        match self.0.take() {
            Some(then) => then(recording, state),
            None => Ok(()),
        }
    }
}

/// Records a value and passes it on.
struct CaptureSink<T, C> {
    recording: T,
    end: Option<Event<'static>>,
    // `None` once the value was passed on
    then: Option<C>,
}

impl<'de, T, C> CaptureSink<T, C> {
    fn child(&mut self, state: &mut State) -> SinkHandle<'_, 'de>
    where
        T: Target<'de>,
    {
        SinkHandle::arena(
            Recorder {
                target: &mut self.recording,
                end: None,
                is_root: false,
            },
            state,
        )
    }
}

impl<'de, T: Target<'de> + Default, C: Capture<'de, T>> Sink<'de> for CaptureSink<T, C> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.then.take() {
            Some(mut then) => then.atom(atom, state),
            None => Ok(()),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match self.then.take() {
            Some(mut then) => then.borrowed_atom(atom, state),
            None => Ok(()),
        }
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let shape = state.container_shape();
        // containers have a few events at least, skip the smallest sizes
        self.recording.reserve();
        self.recording.push(true, Event::MapStart(shape), state);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        let shape = state.container_shape();
        self.recording.reserve();
        self.recording.push(true, Event::SeqStart(shape), state);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child(state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child(state))
    }

    // atoms in the container are recorded without creating a sink for them

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.recording
            .push(false, Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.recording
            .push(false, Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.recording.push_borrowed(false, atom, state);
        Ok(())
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.recording.push_borrowed(false, atom, state);
        Ok(())
    }

    /// Takes all keys if the recording is flattened into a struct.
    ///
    /// The keys are recorded as a map.
    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        match self.end {
            Some(Event::MapEnd) => {}
            None if self.recording.is_empty() => {
                self.recording
                    .push(true, Event::MapStart(ContainerShape::new()), state);
                self.end = Some(Event::MapEnd);
            }
            _ => return Ok(None),
        }
        self.recording
            .push(false, Event::Atom(Atom::Str(key.to_owned().into())), state);
        Ok(Some(self.child(state)))
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        // an atom was already passed on
        let Some(mut then) = self.then.take() else {
            return Ok(());
        };
        if self.recording.is_empty() && self.end.is_none() {
            // flattened into a struct but no key was given
            self.recording
                .push(true, Event::MapStart(ContainerShape::new()), state);
            self.end = Some(Event::MapEnd);
        }
        if let Some(end) = self.end.take() {
            self.recording.push(true, end, state);
        }
        then.recorded(core::mem::take(&mut self.recording), state)
    }
}

/// Records a single value into a recording.
struct Recorder<'a, T> {
    target: &'a mut T,
    end: Option<Event<'static>>,
    is_root: bool,
}

impl<'a, T> Recorder<'a, T> {
    fn child<'de>(&mut self, state: &mut State) -> SinkHandle<'_, 'de>
    where
        T: Target<'de>,
    {
        SinkHandle::arena(
            Recorder {
                target: &mut *self.target,
                end: None,
                is_root: false,
            },
            state,
        )
    }
}

impl<'a, 'de, T: Target<'de>> Sink<'de> for Recorder<'a, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.target
            .push(self.is_root, Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.target.push_borrowed(self.is_root, atom, state);
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.target.push(
            self.is_root,
            Event::MapStart(state.container_shape()),
            state,
        );
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.target.push(
            self.is_root,
            Event::SeqStart(state.container_shape()),
            state,
        );
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child(state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(self.child(state))
    }

    // atoms in the container are recorded without creating a sink for them

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.target
            .push(false, Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.target
            .push(false, Event::Atom(atom.to_static()), state);
        Ok(())
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.target.push_borrowed(false, atom, state);
        Ok(())
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.target.push_borrowed(false, atom, state);
        Ok(())
    }
    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some(end) = self.end.take() {
            self.target.push(self.is_root, end, state);
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
        self.0.events.len() == other.0.events.len()
            && self
                .0
                .events
                .iter()
                .zip(other.0.events.iter())
                .all(|(a, b)| a.event == b.event)
    }
}

impl<'de> Deserialize<'de> for Recording {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        Recording::capture(
            move |recording, _state| {
                *out = Some(recording);
                Ok(())
            },
            state,
        )
    }
}

/// Returns the number of events of the value the events start with.
fn value_len(events: &[RecordedEvent<'static>]) -> usize {
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
struct RecordedValue<'a>(&'a [RecordedEvent<'static>]);

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
        match snapshot {
            Some(snapshot) => state
                .extensions_mut()
                .restore_event_data(snapshot.event_data()),
            None => state.clear_event_data(),
        }
        let inner = events.get(1..events.len().saturating_sub(1)).unwrap_or(&[]);
        Ok(match first {
            Event::Atom(atom) => Chunk::Atom(atom.as_borrowed()),
            Event::MapStart(_) => Chunk::map(
                RecordedEmitter {
                    rest: inner,
                    current: RecordedValue(&[]),
                },
                state,
            ),
            Event::SeqStart(_) => Chunk::seq(
                RecordedEmitter {
                    rest: inner,
                    current: RecordedValue(&[]),
                },
                state,
            ),
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
    rest: &'a [RecordedEvent<'static>],
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
        RecordedValue(self.0.events.as_slice()).chunk(state)
    }

    fn container_shape(&self) -> ContainerShape {
        RecordedValue(self.0.events.as_slice()).container_shape()
    }

    fn is_optional(&self) -> bool {
        matches!(
            self.0.events.as_slice(),
            [RecordedEvent {
                event: Event::Atom(Atom::Null),
                ..
            }]
        )
    }
}
