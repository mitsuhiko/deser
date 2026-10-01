//! The state shared between data formats and the types they process.
use alloc::vec::Vec;
use core::any::TypeId;
use core::fmt;

use crate::arena::{Arena, Buffer};
use crate::error::{Error, ErrorContext};
use crate::event::ContainerShape;
use crate::ext::RawFormatInfo;
use crate::extensions::{EventData, Extensions};

/// The input range of events without one.
pub(crate) const NO_RANGE: (usize, usize) = (usize::MAX, 0);

/// Gives access to the state of an ongoing serialization or deserialization.
///
/// The state acts as a communication channel between the data format and
/// the types that are serialized or deserialized.  It is used in both
/// directions: [`Deserialize`](crate::de::Deserialize) implementations and
/// [`Sink`](crate::de::Sink)s receive it during deserialization and
/// [`Serialize`](crate::ser::Serialize) implementations and emitters
/// receive it during serialization.  Formats get mutable access to it through
/// the drivers.  The state also holds the arena the sinks and emitters are
/// allocated in (see [`SinkHandle::arena`](crate::de::SinkHandle::arena)
/// and [`Chunk::seq`](crate::ser::Chunk::seq)).
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
/// Some extension values are well-known: the policy for keys that are
/// given more than once ([`DuplicateKeys`](crate::de::DuplicateKeys)),
/// the policy for keys that no field of a struct takes
/// ([`UnknownFields`](crate::de::UnknownFields)), how bytes are decoded
/// from strings ([`BytesFormat`](crate::BytesFormat)) and the source the
/// input ranges refer to ([`Source`](crate::Source)).  They are read and
/// set with their `of` and `set` functions.
///
/// Additionally formats can publish the byte range in the input of every
/// event (see [`input_range`](Self::input_range)) and extensions can
/// register types that add context to errors (see
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
    // `true` if the innermost open container is a multimap
    pub(crate) is_multimap: bool,
    // the key of the content of maps, empty if there is none (see
    // `ContentKey`)
    pub(crate) content_key: &'static str,
    // the byte range of the current event, `NO_RANGE` if there is none.
    // This is not an option so that it can be cleared with a single store.
    pub(crate) input_range: (usize, usize),
    // keyed by type as function pointers cannot be compared reliably
    error_context: Vec<(TypeId, AddContextFn)>,
    // `true` while errors are thrown away, see `discard_errors`.
    pub(crate) discards_errors: bool,
    // `true` if containers collect the errors of their items, see
    // `set_collect_errors`.
    collect_errors: bool,
    // the number of errors that can still be collected
    remaining_errors: usize,
    // `true` once an error was not collected because of the limit
    error_limit_reached: bool,
    // the arena the sinks of the deserialization are allocated in
    pub(crate) arena: Arena,
    // the format of the raw values that pass through as they are (see
    // `set_raw_format`)
    pub(crate) raw_format: Option<&'static RawFormatInfo>,
    // deserialization: the top-level value is wanted as raw value of the
    // format (see `set_raw_format`)
    pub(crate) raw_requested: Option<&'static RawFormatInfo>,
}

/// The function of an [`ErrorContext`].
type AddContextFn = fn(Error, &State) -> Error;

impl State {
    /// Creates an empty state.
    ///
    /// Drivers create their state, this is useful for code that processes
    /// events without a driver.
    #[allow(clippy::new_without_default)]
    pub fn new() -> State {
        State {
            extensions: Extensions::default(),
            depth: 0,
            container_shape: ContainerShape::new(),
            is_map_key: false,
            is_multimap: false,
            content_key: "",
            input_range: NO_RANGE,
            error_context: Vec::new(),
            discards_errors: false,
            collect_errors: false,
            remaining_errors: usize::MAX,
            error_limit_reached: false,
            arena: Arena::new(),
            raw_format: None,
            raw_requested: None,
        }
    }

    /// Sets if maps and sequences collect the errors of their items.
    ///
    /// By default the first error ends the deserialization.  If errors are
    /// collected, the sinks of the containers that support it (derived
    /// structs and the standard collections) recover from the errors of
    /// their items (see [`Sink::recover`](crate::de::Sink::recover)) and
    /// deserialization continues to find the other errors.  The container
    /// fails once it's complete with all errors it collected (see
    /// [`Error::errors`]), including the fields that are missing.  This
    /// makes it possible to report all problems of the input at once:
    ///
    /// ```
    /// use deser::de::DeserializeDriver;
    /// use deser::{Deserialize, Event};
    ///
    /// #[derive(Deserialize, Debug)]
    /// struct Server {
    ///     host: String,
    ///     port: u16,
    /// }
    ///
    /// let mut out = None::<Vec<Server>>;
    /// let mut driver = DeserializeDriver::new(&mut out);
    /// driver.state_mut().set_collect_errors(true);
    /// let mut rv = Ok(());
    /// for event in [
    ///     Event::seq_start(),
    ///     Event::map_start(),
    ///     "host".into(),
    ///     42u64.into(),
    ///     "port".into(),
    ///     80u64.into(),
    ///     Event::MapEnd,
    ///     Event::map_start(),
    ///     "host".into(),
    ///     "b".into(),
    ///     "port".into(),
    ///     "http".into(),
    ///     Event::MapEnd,
    ///     Event::map_start(),
    ///     Event::MapEnd,
    ///     Event::SeqEnd,
    /// ] {
    ///     rv = rv.and_then(|()| driver.emit(event));
    /// }
    /// let err = rv.unwrap_err();
    /// let errors: Vec<_> = err.errors().map(|err| err.message()).collect();
    /// assert_eq!(
    ///     errors,
    ///     [
    ///         "unexpected unsigned integer, expected string",
    ///         "unexpected string, expected u16",
    ///         "missing field `host`",
    ///         "missing field `port`",
    ///     ]
    /// );
    /// ```
    ///
    /// Types can change this for the values in them, for instance to
    /// collect the errors of a part of the input.  The previous setting is
    /// returned.  While errors are thrown away (for instance while an
    /// untagged enum tries its variants) they are never collected.  See
    /// [`set_max_errors`](Self::set_max_errors) to limit the number of
    /// errors that are collected.
    pub fn set_collect_errors(&mut self, yes: bool) -> bool {
        core::mem::replace(&mut self.collect_errors, yes)
    }

    /// Returns `true` if the errors of items are collected.
    ///
    /// See [`set_collect_errors`](Self::set_collect_errors).
    pub fn collects_errors(&self) -> bool {
        self.collect_errors && !self.discards_errors
    }

    /// Limits the number of errors that are collected.
    ///
    /// Once the limit is reached, the next error ends the deserialization
    /// (together with the errors collected so far).  By default there is no
    /// limit.  This counts from the current number of collected errors.
    pub fn set_max_errors(&mut self, max: usize) {
        self.remaining_errors = max;
        self.error_limit_reached = false;
    }

    /// Returns `true` once the limit of errors was reached.
    ///
    /// The error that exceeded the limit (see
    /// [`set_max_errors`](Self::set_max_errors)) ends the deserialization.
    /// Sinks that keep the errors of their values instead of returning
    /// them should return errors once this is set.
    pub fn error_limit_reached(&self) -> bool {
        self.error_limit_reached
    }

    /// Takes a number of the errors that can still be collected.
    ///
    /// Returns `false` if errors are not collected or the limit is reached.
    pub(crate) fn take_error_slots(&mut self, count: usize) -> bool {
        if !self.collects_errors() {
            false
        } else if self.remaining_errors >= count {
            self.remaining_errors -= count;
            true
        } else {
            self.error_limit_reached = true;
            false
        }
    }

    /// Returns `true` while errors are thrown away.
    ///
    /// While an untagged enum tries its variants only whether a variant
    /// accepts the value matters.  The errors are created without a message
    /// then, and the driver does not attach context to them.  Sinks that
    /// keep the errors of their values (instead of returning them) should
    /// return them while this is set.
    pub fn discards_errors(&self) -> bool {
        self.discards_errors
    }

    /// Runs a function during which errors are thrown away.
    ///
    /// This is used while an untagged enum tries its variants: only whether
    /// a variant accepts the value matters, so the errors that are returned
    /// are created without a message (see
    /// [`discarded_error`](crate::error::discarded_error)) and the driver
    /// does not attach context to them.  Errors that are kept rather than
    /// returned (for instance collected unknown fields) are unaffected.
    pub(crate) fn discard_errors<R>(&mut self, f: impl FnOnce(&mut State) -> R) -> R {
        let outer = core::mem::replace(&mut self.discards_errors, true);
        let rv = f(self);
        self.discards_errors = outer;
        rv
    }

    /// Takes the scratch space of a format that was kept from the last
    /// deserialization (see
    /// [`__private_put_scratch`](Self::__private_put_scratch)).
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    pub fn __private_take_scratch(&mut self) -> Vec<u8> {
        self.arena.take_vec(Buffer::Scratch).unwrap_or_default()
    }

    /// Keeps the scratch space of a format for the next deserialization.
    ///
    /// Formats that unescape strings into a buffer would otherwise grow it
    /// again for every document.  The buffer is cleared.
    ///
    /// Internal fast path, not public API (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    pub fn __private_put_scratch(&mut self, buffer: Vec<u8>) {
        self.arena.put_vec(Buffer::Scratch, buffer);
    }

    /// Declares the format of the raw values that pass through as they are.
    ///
    /// Formats with raw values (see [`Raw`](crate::ext::Raw)) call this with
    /// the description of their format:
    ///
    /// * Deserializers call it before they emit the first event.  Sinks
    ///   then request values that deserialize into raw values of the format
    ///   (see [`Error::is_raw_request`]) and the format passes on their
    ///   input as [`RawInput`](crate::ext::RawInput) rather than their
    ///   events.  The top-level value is requested before the
    ///   deserialization starts: this returns `true` if the top-level value
    ///   is wanted as raw value.  Only the first call can return `true`.
    /// * Serializers call it before the first value and ignore the result.
    ///   Raw values of the format are then emitted as
    ///   [`RawInput`](crate::ext::RawInput), which the serializer writes as
    ///   it is.  Raw values of other formats are serialized as the values
    ///   they hold.
    #[inline(always)]
    pub fn set_raw_format(&mut self, format: &'static RawFormatInfo) -> bool {
        self.raw_format = Some(format);
        self.raw_requested
            .take()
            .is_some_and(|requested| core::ptr::eq(requested, format))
    }

    /// Requests the next value as raw value of a format.
    ///
    /// Sinks call this while they handle the event before the value that
    /// deserializes into a [`Raw`](crate::ext::Raw) value (for instance the
    /// key of the field) and return the result from the event.  If the
    /// format passes on the input of values of the format (see
    /// [`set_raw_format`](Self::set_raw_format)), the result is the request
    /// (see [`Error::is_raw_request`]), otherwise the value is deserialized
    /// from its events.  The drivers pass the request on to the format,
    /// which emits the next value as [`RawInput`](crate::ext::RawInput).
    /// Sequences request their first item when they start and every next
    /// one after an item.  As the request is the result of an event,
    /// formats do not check for it for every value.
    ///
    /// Internal protocol, not public API yet (see `lib.rs`).
    #[doc(hidden)]
    #[inline]
    pub fn __private_request_raw(&mut self, format: &'static RawFormatInfo) -> Result<(), Error> {
        match self.raw_format {
            Some(own) if core::ptr::eq(own, format) => Err(Error::raw_request()),
            _ => Ok(()),
        }
    }

    /// Returns `true` if the serializer writes raw values of the format as
    /// they are (see [`set_raw_format`](Self::set_raw_format)).
    #[inline]
    pub(crate) fn accepts_raw(&self, format: &'static RawFormatInfo) -> bool {
        self.raw_format
            .is_some_and(|own| core::ptr::eq(own, format))
    }

    /// Takes the state out, leaving an empty state that does not allocate.
    pub(crate) fn take(&mut self) -> State {
        core::mem::replace(self, State::new())
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
    #[inline(always)]
    pub fn event<T: fmt::Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions.event()
    }

    /// Returns the data of a type attached to the current event mutably.
    ///
    /// If no data of this type is attached to the current event yet, the
    /// default value is attached.  See [`event`](Self::event) for more
    /// information.
    ///
    /// Event data has to be [`Send`] and [`Sync`] so that it can be captured
    /// (see [`capture_event_data`](Self::capture_event_data)) without
    /// preventing the captured data from being shared between threads.
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

    /// Takes the data of a type from the current event.
    ///
    /// Returns `None` if no such data is attached to the event.  Unlike
    /// resetting the data through [`event_mut`](Self::event_mut) this
    /// detaches it, so the data is not captured with the event by the sinks
    /// it's passed on to (such as the ones of a
    /// [`Recording`](crate::de::Recording)).  This is what types which
    /// consume event data (like a wrapper which captures a tag) use.
    ///
    /// ```
    /// # use deser::State;
    /// #[derive(Debug, Default, Clone)]
    /// struct Tag(String);
    ///
    /// fn take_tag(state: &mut State) -> Option<String> {
    ///     state.take_event::<Tag>().map(|tag| tag.0)
    /// }
    /// ```
    pub fn take_event<T: Default + fmt::Debug + Send + Sync + 'static>(&mut self) -> Option<T> {
        self.extensions.take_event()
    }

    /// Captures the data attached to the current event.
    ///
    /// The captured data can be attached to another event later with
    /// [`attach_event_data`](Self::attach_event_data).  This allows types
    /// which hold values outside of a serialization or deserialization to
    /// retain data like tags (see [`EventData`]).
    pub fn capture_event_data(&self) -> EventData {
        self.extensions.capture_event_data()
    }

    /// Attaches captured data to the current event.
    ///
    /// Data of the same types that is already attached to the event is
    /// replaced, other data is retained.  See
    /// [`event`](Self::event) for when event data is attached and detached.
    pub fn attach_event_data(&mut self, data: &EventData) {
        self.extensions.attach_event_data(data);
    }

    /// Detaches all data from the current event.
    ///
    /// The drivers call this after every event.
    #[inline(always)]
    pub(crate) fn clear_event_data(&mut self) {
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
    /// Formats which can only represent string keys (such as JSON) emit
    /// them as [`Atom::Lexical`](crate::Atom::Lexical), which the sinks of
    /// the keys parse, so sinks rarely need this.
    ///
    /// During serialization this is `true` while a map key (including the
    /// keys of structs) is serialized and emitted.
    pub fn is_map_key(&self) -> bool {
        self.is_map_key
    }

    /// Returns `true` if the innermost open container is a multimap.
    ///
    /// A multimap is a map whose keys can be given more than once (see
    /// [`ContainerShape::with_multimap`]).  This is the case while its
    /// keys and values are deserialized and while its sink is finished
    /// (in [`Sink::finish`](crate::de::Sink::finish)), which includes the
    /// keys and values that flattened fields take.  Within a nested map or
    /// sequence it's the flag of that container.
    ///
    /// Sinks that collect the values of repeated keys (derived structs and
    /// maps) check this.
    #[inline]
    pub fn is_multimap(&self) -> bool {
        self.is_multimap
    }

    /// Returns the byte range in the input of the current event.
    ///
    /// This is only available if the format provides it (see
    /// [`set_input_range`](Self::set_input_range)).  The range refers to
    /// the [`Source`](crate::Source) and can be resolved into lines and columns for
    /// instance with the `deser-location` crate.
    #[inline]
    pub fn input_range(&self) -> Option<core::ops::Range<usize>> {
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

    /// Registers a type that adds context to errors.
    ///
    /// When an event fails (for instance because a sink rejects a value)
    /// the drivers invoke [`ErrorContext::add_context`] of the registered
    /// types in the order they were registered with the error and the
    /// state as it was when the error happened.  This means that the
    /// context is also correct for errors in values which are replayed from
    /// a [`Recording`](crate::de::Recording).  The drivers only do this
    /// once for an error: the outer containers which the error passes
    /// through do not add their context.
    ///
    /// Registering the same type again has no effect, so this can be
    /// called for every event.
    ///
    /// ```
    /// use deser::de::DeserializeDriver;
    /// use deser::{Error, ErrorAttachment, ErrorContext, Event, State};
    ///
    /// #[derive(Debug)]
    /// struct Depth(usize);
    ///
    /// impl ErrorAttachment for Depth {}
    ///
    /// impl ErrorContext for Depth {
    ///     fn add_context(err: Error, state: &State) -> Error {
    ///         match err.attachment::<Depth>() {
    ///             Some(_) => err,
    ///             None => err.with_attachment(Depth(state.depth())),
    ///         }
    ///     }
    /// }
    ///
    /// let mut out = None::<Vec<Vec<u32>>>;
    /// let mut driver = DeserializeDriver::new(&mut out);
    /// driver.state_mut().add_error_context::<Depth>();
    /// driver.emit(Event::seq_start()).unwrap();
    /// driver.emit(Event::seq_start()).unwrap();
    /// let err = driver.emit(true).unwrap_err();
    /// assert_eq!(err.attachment::<Depth>().unwrap().0, 2);
    /// ```
    pub fn add_error_context<T: ErrorContext>(&mut self) {
        let key = TypeId::of::<T>();
        if !self.error_context.iter().any(|&(other, _)| other == key) {
            self.error_context.push((key, T::add_context));
        }
    }

    /// Attaches the context of the current event to an error.
    ///
    /// The drivers do this for the errors of the events that fail (see
    /// [`add_error_context`](Self::add_error_context)): the start of the
    /// input range of the event is attached as offset (unless the error
    /// has one) and the registered types add their context.  Errors that
    /// already have the context of an event attached are returned
    /// unchanged.  This is for sinks that handle the errors of their values
    /// themselves instead of returning them, so that they have the same
    /// context as the errors the driver sees.
    #[cold]
    #[inline(never)]
    pub fn attach_error_context(&self, mut err: Error) -> Error {
        if err.has_context() {
            return err;
        }
        err.set_has_context();
        if err.offset().is_none()
            && let Some(range) = self.input_range()
        {
            err = err.with_offset(range.start);
        }
        for (_, f) in self.error_context.iter() {
            err = f(err, self);
        }
        err
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
            .field("is_multimap", &self.is_multimap)
            .field("input_range", &self.input_range())
            .finish()
    }
}
