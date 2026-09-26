//! The state shared between data formats and the types they process.
use std::fmt;
use std::sync::Arc;

use crate::error::Error;
use crate::event::ContainerShape;
use crate::extensions::Extensions;

/// The input range of events without one.
pub(crate) const NO_RANGE: (usize, usize) = (usize::MAX, 0);

/// Gives access to the state of an ongoing serialization or deserialization.
///
/// The state acts as a communication channel between the data format and
/// the types that are serialized or deserialized.  It is used in both
/// directions: [`Sink`](crate::de::Sink)s receive it during deserialization
/// and [`Serialize`](crate::ser::Serialize) implementations and emitters
/// receive it during serialization.  Formats get mutable access to it through
/// the drivers.
///
/// Besides some information about the current position (such as the
/// [`depth`](Self::depth)) it holds typed values that can be used by formats
/// and types to exchange information that is not part of the data model:
///
/// * Extension values ([`get`](Self::get) and [`get_mut`](Self::get_mut))
///   remain in the state until they are changed.  They are used for
///   information that spans many events such as the current path.
/// * Event data ([`event`](Self::event) and [`event_mut`](Self::event_mut))
///   is attached to a single event and detached by the drivers after the
///   event was delivered.  It is used for information about an individual
///   value, such as a tag.
///
/// Additionally formats can publish the byte range in the input of every
/// event (see [`input_range`](Self::input_range)) and extensions can
/// register functions that add context to errors (see
/// [`add_error_context`](Self::add_error_context)).
///
/// Extension values have to be [`Send`] and [`Sync`] so that the state is
/// too.  This means that the state never prevents an ongoing serialization
/// or deserialization from moving between threads.  They are `Sync` as
/// event data is recorded (see [`Recording`](crate::de::Recording)) and
/// recordings can be serialized.
pub struct State {
    extensions: Extensions,
    // the number of open containers
    pub(crate) depth: usize,
    // the shape of the container that is currently started
    pub(crate) container_shape: ContainerShape,
    pub(crate) is_map_key: bool,
    // the byte range of the current event, `NO_RANGE` if there is none.
    // This is not an option so that it can be cleared with a single store.
    pub(crate) input_range: (usize, usize),
    source: Option<Arc<str>>,
    error_context: Vec<ErrorContextFn>,
}

/// A function that adds context to an error, see [`State::add_error_context`].
pub type ErrorContextFn = fn(Error, &State) -> Error;

impl State {
    /// Creates a new state for a driver.
    pub(crate) fn new() -> State {
        State {
            extensions: Extensions::default(),
            depth: 0,
            container_shape: ContainerShape::new(),
            is_map_key: false,
            input_range: NO_RANGE,
            source: None,
            error_context: Vec::new(),
        }
    }

    /// Takes the state out, leaving an empty state that does not allocate.
    pub(crate) fn take(&mut self) -> State {
        std::mem::replace(
            self,
            State {
                extensions: Extensions::default(),
                depth: 0,
                container_shape: ContainerShape::new(),
                is_map_key: false,
                input_range: NO_RANGE,
                source: None,
                error_context: Vec::new(),
            },
        )
    }

    #[inline]
    pub(crate) fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    #[inline]
    pub(crate) fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    /// Returns an extension value.
    ///
    /// Returns `None` if the value was never set.
    #[inline]
    pub fn get<T: fmt::Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions.get()
    }

    /// Returns a mutable extension value.
    ///
    /// If the value was never set, it's initialized with the default value.
    #[inline]
    pub fn get_mut<T: Default + fmt::Debug + Send + Sync + 'static>(&mut self) -> &mut T {
        self.extensions.get_mut()
    }

    /// Marks an extension type as replayable.
    ///
    /// When a value is internally buffered during deserialization (for
    /// instance for internally tagged enums, see
    /// [`Recording`](crate::de::Recording)) the values of replayable
    /// extensions are captured for every event and restored when the event is
    /// replayed.  This is used for information that changes from event to
    /// event but remains in the state, such as the current path.  Event data
    /// is always captured, it does not need to be marked.
    pub fn set_replayable<T: Clone + Default + fmt::Debug + Send + Sync + 'static>(&mut self) {
        self.extensions.set_replayable::<T>();
    }

    /// Returns the data of a type attached to the current event.
    ///
    /// Returns `None` if no such data is attached to the event.
    ///
    /// Event data is attached to the next event and detached by the driver
    /// after that event was delivered:
    ///
    /// * During deserialization, formats attach data with
    ///   [`event_mut`](Self::event_mut) before they emit the event with the
    ///   [`DeserializeDriver`](crate::de::DeserializeDriver).  The sinks
    ///   that receive the event (including the
    ///   [`finish`](crate::de::Sink::finish) of a container on its end event)
    ///   can access it.
    /// * During serialization, [`Serialize`](crate::ser::Serialize)
    ///   implementations and emitters attach data while they produce a
    ///   value.  The format receives it together with the first event of the
    ///   value from the [`SerializeDriver`](crate::ser::SerializeDriver).
    ///
    /// Event data is captured by a [`Recording`](crate::de::Recording) and
    /// restored when the events are replayed.
    #[inline]
    pub fn event<T: fmt::Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions.event()
    }

    /// Returns the data of a type attached to the current event mutably.
    ///
    /// If no data of this type is attached to the current event yet, the
    /// default value is attached.  See [`event`](Self::event) for more
    /// information.
    ///
    /// Detached values are retained and reused for later events.  They are
    /// reset with [`clone_from`](Clone::clone_from) from the default value,
    /// which means that types which forward `clone_from` to their fields
    /// (unlike derived implementations of [`Clone`]) reuse the memory of
    /// collections such as [`Vec`].
    ///
    /// ```
    /// # use deser::State;
    /// #[derive(Debug, Default, Clone)]
    /// struct Tags(Vec<u64>);
    ///
    /// fn push_tag(state: &mut State, tag: u64) {
    ///     state.event_mut::<Tags>().0.push(tag);
    /// }
    /// ```
    #[inline]
    pub fn event_mut<T: Default + Clone + fmt::Debug + Send + Sync + 'static>(&mut self) -> &mut T {
        self.extensions.event_mut()
    }

    /// Returns `true` if any data is attached to the current event.
    ///
    /// This is a cheap check that formats can use to skip looking up event
    /// data for the vast majority of events that have none.
    #[inline(always)]
    pub fn has_event_data(&self) -> bool {
        self.extensions.has_event_data()
    }

    /// Detaches all data from the current event.
    ///
    /// The drivers call this after every event.
    #[inline(always)]
    pub fn clear_event_data(&mut self) {
        self.extensions.clear_event_data();
    }

    /// Returns the current recursion depth.
    ///
    /// This is the number of containers (maps and sequences) that are
    /// currently open.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the shape of the container that is started.
    ///
    /// During deserialization this is the shape of the
    /// [`MapStart`](crate::Event::MapStart) or
    /// [`SeqStart`](crate::Event::SeqStart) event and is intended to be
    /// called from [`Sink::map`](crate::de::Sink::map) and
    /// [`Sink::seq`](crate::de::Sink::seq).  At other times it's the shape of
    /// the container that was started last.
    pub fn container_shape(&self) -> ContainerShape {
        self.container_shape
    }

    /// Returns `true` if the value currently being processed is a map key.
    ///
    /// Many formats (such as JSON) can only represent string keys.  During
    /// deserialization sinks can use this to accept a stringified
    /// representation of their value when it's used as a key.  For instance
    /// the integer sinks will parse `"42"` as a number when in key position.
    ///
    /// During serialization this is `true` while a map key (including the
    /// keys of structs) is serialized and emitted.
    pub fn is_map_key(&self) -> bool {
        self.is_map_key
    }

    /// Returns the byte range in the input of the current event.
    ///
    /// This is only available if the format provides it (see
    /// [`set_input_range`](Self::set_input_range)).  The range refers to
    /// the [`source`](Self::source) and can be resolved into lines and
    /// columns for instance with the `deser-location` crate.
    #[inline]
    pub fn input_range(&self) -> Option<std::ops::Range<usize>> {
        let (start, end) = self.input_range;
        if start == NO_RANGE.0 {
            None
        } else {
            Some(start..end)
        }
    }

    /// Sets the byte range in the input of the next event.
    ///
    /// Formats call this before they emit an event into a
    /// [`DeserializeDriver`](crate::de::DeserializeDriver).  Like event
    /// data, the range is only attached to the next event: the driver
    /// detaches it after the event was delivered.
    ///
    /// ```
    /// use deser::de::DeserializeDriver;
    ///
    /// let mut out = None::<bool>;
    /// let mut driver = DeserializeDriver::new(&mut out);
    /// driver.state_mut().set_input_range(0, 4);
    /// driver.emit(true).unwrap();
    /// assert_eq!(driver.state().input_range(), None);
    /// ```
    #[inline(always)]
    pub fn set_input_range(&mut self, start: usize, end: usize) {
        self.input_range = (start, end);
    }

    /// Detaches the input range and the event data from the current event.
    #[inline(always)]
    pub(crate) fn clear_event(&mut self) {
        self.input_range.0 = NO_RANGE.0;
        self.extensions.clear_event_data();
    }

    /// Registers a function that adds context to errors.
    ///
    /// When an event fails (for instance because a sink rejects a value)
    /// the drivers invoke the registered functions with the error and the
    /// state as it was when the error happened.  This means that the
    /// context is also correct for errors in values which are replayed from
    /// a [`Recording`](crate::de::Recording).  The drivers only do this
    /// once for an error: the outer containers which the error passes
    /// through do not add their context.  Functions should not replace
    /// context that is already there.
    ///
    /// Registering the same function again has no effect.
    ///
    /// ```
    /// use deser::de::DeserializeDriver;
    /// use deser::{Error, ErrorAttachment, Event, State};
    ///
    /// #[derive(Debug)]
    /// struct Depth(usize);
    ///
    /// impl ErrorAttachment for Depth {}
    ///
    /// fn add_depth(err: Error, state: &State) -> Error {
    ///     match err.attachment::<Depth>() {
    ///         Some(_) => err,
    ///         None => err.with_attachment(Depth(state.depth())),
    ///     }
    /// }
    ///
    /// let mut out = None::<Vec<Vec<u32>>>;
    /// let mut driver = DeserializeDriver::new(&mut out);
    /// driver.state_mut().add_error_context(add_depth);
    /// driver.emit(Event::seq_start()).unwrap();
    /// driver.emit(Event::seq_start()).unwrap();
    /// let err = driver.emit(true).unwrap_err();
    /// assert_eq!(err.attachment::<Depth>().unwrap().0, 2);
    /// ```
    pub fn add_error_context(&mut self, f: ErrorContextFn) {
        if !self
            .error_context
            .iter()
            .any(|&other| other as usize == f as usize)
        {
            self.error_context.push(f);
        }
    }

    /// Attaches the context of the current event to an error.
    #[cold]
    #[inline(never)]
    pub(crate) fn attach_error_context(&self, mut err: Error) -> Error {
        if err.has_context() {
            return err;
        }
        err.set_has_context();
        if err.offset().is_none()
            && let Some(range) = self.input_range()
        {
            err = err.with_offset(range.start);
        }
        for f in self.error_context.iter() {
            err = f(err, self);
        }
        err
    }

    /// Returns the source the input ranges refer to.
    ///
    /// Input ranges are cheap to publish, but resolving them into lines and
    /// columns requires the source.  As this requires a copy of the input,
    /// formats only provide it when asked to (for instance with their
    /// `track_locations` option).
    #[inline]
    pub fn source(&self) -> Option<&Arc<str>> {
        self.source.as_ref()
    }

    /// Sets the source the input ranges refer to.
    ///
    /// See [`source`](Self::source).  Formats call this before emitting the
    /// first event.
    pub fn set_source<S: Into<Arc<str>>>(&mut self, source: S) {
        self.source = Some(source.into());
    }
}

// the state must never prevent an ongoing serialization or deserialization
// from moving between threads.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<State>();
};

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("extensions", &self.extensions)
            .field("depth", &self.depth)
            .field("is_map_key", &self.is_map_key)
            .field("input_range", &self.input_range())
            .field(
                "source_len",
                &self.source.as_ref().map(|source| source.len()),
            )
            .finish()
    }
}
