use std::marker::PhantomData;

use crate::State;
use crate::de::layer::{Layer, LayerEvent, Next};
use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::event::{Atom, ContainerShape, Event};

/// The driver allows emitting deserialization events into a [`Deserialize`].
///
/// This is a convenient way to safely drive a [`Sink`](crate::de::Sink) of a [`Deserialize`]
/// without using the runtime stack.  As rust lifetimes make what this type does
/// internally impossible with safe code, this is a safe abstractiont that
/// hides the unsafety internally.
///
/// # Events and Their Context
///
/// Events are emitted with [`emit`](Self::emit) or, if they borrow from
/// the data being deserialized, with [`emit_borrowed`](Self::emit_borrowed).
/// Information about the next event is placed into the [`State`] before it's
/// emitted: its byte range in the input with
/// [`State::set_input_range`] and data attached to it with
/// [`State::event_mut`].  Both are detached after the event was delivered.
///
/// ```
/// use deser::de::DeserializeDriver;
/// use deser::Event;
///
/// let mut out = None::<Vec<u32>>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// driver.state_mut().set_input_range(0, 1);
/// driver.emit(Event::seq_start()).unwrap();
/// driver.state_mut().set_input_range(1, 3);
/// driver.emit(42u64).unwrap();
/// driver.state_mut().set_input_range(3, 4);
/// driver.emit(Event::SeqEnd).unwrap();
/// ```
///
/// When an event fails, the error gets the context of the event attached
/// (see [`Error`] and [`State::add_error_context`]).
///
/// # Layers
///
/// [`Layer`]s sit between the format and the sinks and see every event
/// before it's delivered.  They are added with
/// [`push_layer`](Self::push_layer), see [`Layer`] for more information.
pub struct DeserializeDriver<'a, 'de: 'a> {
    core: DriverCore<'de>,
    layers: Vec<Box<dyn Layer>>,
    // the sinks borrow for 'a
    _marker: PhantomData<&'a mut ()>,
}

/// The state and the sinks of a driver.
pub(crate) struct DriverCore<'de> {
    pub(crate) state: State,
    // The sinks borrow from each other: every sink on the stack can borrow
    // from the sink below it.  The lifetimes of these borrows are erased
    // (to `'de` as the handles cannot outlive that) and it's the driver's
    // responsibility to never use a sink while one of the sinks it lent out
    // is still alive and to drop them in inverse order.
    //
    // `root` holds the sink the driver was created with while no container
    // is open.
    root: Option<SinkHandle<'de, 'de>>,
    sink_stack: Vec<(SinkHandle<'de, 'de>, Container)>,
}

const STACK_CAPACITY: usize = 128;

// an ongoing serialization can move between threads, for instance when it
// is suspended while waiting for IO.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<DeserializeDriver<'static, 'static>>();
};

#[derive(Copy, Clone)]
enum Container {
    /// A map, the flag is `true` if a key is expected next.
    Map(bool),
    Seq,
}

/// Erases the lifetime of a sink handle.
///
/// # Safety
///
/// The caller must ensure that the handle is dropped before the data it
/// borrows from.
unsafe fn erase_lifetime<'de>(handle: SinkHandle<'_, 'de>) -> SinkHandle<'de, 'de> {
    unsafe { std::mem::transmute::<SinkHandle<'_, 'de>, SinkHandle<'de, 'de>>(handle) }
}

impl<'a, 'de> DeserializeDriver<'a, 'de> {
    /// Creates a new deserializer driver.
    pub fn new<T: Deserialize<'de>>(out: &'a mut Option<T>) -> DeserializeDriver<'a, 'de> {
        DeserializeDriver::from_sink(T::deserialize_into(out))
    }

    /// Creates a new deserializer driver from a sink.
    pub fn from_sink(sink: SinkHandle<'a, 'de>) -> DeserializeDriver<'a, 'de> {
        DeserializeDriver::with_state(State::new(), sink)
    }

    /// Runs a nested driver within an ongoing deserialization.
    ///
    /// The nested driver continues on the state of the ongoing
    /// deserialization: the extensions are shared and the containers opened
    /// by the nested driver are placed on top of the ones that are currently
    /// open.  This is used to replay recorded events so that replayed values
    /// observe the same state as values that were not buffered.  The nested
    /// driver has no layers: the replayed events already passed the layers
    /// when they were recorded.
    pub(crate) fn nested<R>(
        state: &mut State,
        sink: SinkHandle<'_, 'de>,
        is_map_key: bool,
        f: impl FnOnce(&mut DeserializeDriver<'_, 'de>) -> R,
    ) -> R {
        let depth = state.depth;
        let outer_is_map_key = state.is_map_key;
        let mut driver = DeserializeDriver::with_state(state.take(), sink);
        driver.core.state.is_map_key = is_map_key;
        let rv = f(&mut driver);
        *state = driver.core.state.take();
        drop(driver);
        // a failed replay can leave containers open
        state.depth = depth;
        state.is_map_key = outer_is_map_key;
        rv
    }

    fn with_state(state: State, sink: SinkHandle<'a, 'de>) -> DeserializeDriver<'a, 'de> {
        DeserializeDriver {
            core: DriverCore {
                state,
                sink_stack: Vec::with_capacity(STACK_CAPACITY),
                // SAFETY: the driver cannot outlive 'a
                root: Some(unsafe { erase_lifetime(sink) }),
            },
            layers: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Returns a borrowed reference to the current deserializer state.
    pub fn state(&self) -> &State {
        &self.core.state
    }

    /// Returns a mutable reference to the current deserializer state.
    ///
    /// Formats use this to publish information for the event they emit
    /// next into the state.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.core.state
    }

    /// Adds a layer.
    ///
    /// Layers see the events in the order they were added: the layer that
    /// was added first sees the events emitted into the driver, the last
    /// one passes them on to the sinks.  See [`Layer`] for more
    /// information.
    pub fn push_layer<L: Layer + 'static>(&mut self, layer: L) {
        self.layers.push(Box::new(layer));
    }

    /// Wraps the sink the driver deserializes into.
    ///
    /// This allows placing a sink between the driver and the sink of a
    /// value, for instance to change how certain values are deserialized.
    /// Unlike [`Layer`]s such sinks see the sinks of the values and not just
    /// the events.  Sinks created by a wrapped sink are not wrapped
    /// automatically, the wrapper needs to wrap them in
    /// [`next_key`](crate::de::Sink::next_key) and
    /// [`next_value`](crate::de::Sink::next_value) if it wants to see
    /// them.
    ///
    /// # Panics
    ///
    /// Panics if events were already emitted.
    pub fn wrap_sink<F>(&mut self, f: F)
    where
        F: for<'x> FnOnce(SinkHandle<'x, 'de>) -> SinkHandle<'x, 'de>,
    {
        assert!(
            self.core.sink_stack.is_empty(),
            "sinks can only be wrapped before events are emitted"
        );
        let root = self.core.root.take().expect("no active sink");
        self.core.root = Some(f(root));
    }

    /// Emits an event into the driver.
    ///
    /// The data of the event is only valid for the call.  To emit data that
    /// can be borrowed use [`emit_borrowed`](Self::emit_borrowed).
    ///
    /// # Panics
    ///
    /// The driver keeps an internal state and emitting events when they are
    /// not expected will cause the driver to panic.
    #[inline]
    pub fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        match event.into() {
            Event::Atom(atom) => self.atom_event(atom),
            Event::MapStart(shape) => self.start_event(true, shape),
            Event::SeqStart(shape) => self.start_event(false, shape),
            Event::MapEnd => self.end_event(true),
            Event::SeqEnd => self.end_event(false),
        }
    }

    /// Emits an event that borrows from the data being deserialized.
    ///
    /// This is like [`emit`](Self::emit) but atoms are passed to
    /// [`Sink::borrowed_atom`](crate::de::Sink::borrowed_atom) which means
    /// that types like `&str` can borrow them:
    ///
    /// ```
    /// use deser::de::DeserializeDriver;
    ///
    /// let input = String::from("hello");
    /// let mut out = None::<&str>;
    /// {
    ///     let mut driver = DeserializeDriver::new(&mut out);
    ///     driver.emit_borrowed(input.as_str()).unwrap();
    /// }
    /// assert_eq!(out, Some("hello"));
    /// ```
    #[inline]
    pub fn emit_borrowed<E: Into<Event<'de>>>(&mut self, event: E) -> Result<(), Error> {
        match event.into() {
            Event::Atom(atom) => self.borrowed_atom_event(atom),
            Event::MapStart(shape) => self.start_event(true, shape),
            Event::SeqStart(shape) => self.start_event(false, shape),
            Event::MapEnd => self.end_event(true),
            Event::SeqEnd => self.end_event(false),
        }
    }

    // The following functions deliver an event emitted into the driver and
    // detach its context afterwards.  They are not inlined so that the code
    // emitting events stays small.

    #[inline(never)]
    fn atom_event(&mut self, atom: Atom) -> Result<(), Error> {
        if !self.layers.is_empty() {
            return self.emit_layered(LayerEvent::new(Event::Atom(atom)));
        }
        let rv = self.core.emit_atom(atom);
        self.core.finish_event(rv)
    }

    #[inline(never)]
    fn borrowed_atom_event(&mut self, atom: Atom<'de>) -> Result<(), Error> {
        if !self.layers.is_empty() {
            return self.emit_layered(LayerEvent::borrowed(Event::Atom(atom)));
        }
        let rv = self.core.emit_borrowed_atom(atom);
        self.core.finish_event(rv)
    }

    #[inline(never)]
    fn start_event(&mut self, is_map: bool, shape: ContainerShape) -> Result<(), Error> {
        if !self.layers.is_empty() {
            let event = if is_map {
                Event::MapStart(shape)
            } else {
                Event::SeqStart(shape)
            };
            return self.emit_layered(LayerEvent::new(event));
        }
        let rv = self.core.emit_start(is_map, shape);
        self.core.finish_event(rv)
    }

    #[inline(never)]
    fn end_event(&mut self, is_map: bool) -> Result<(), Error> {
        if !self.layers.is_empty() {
            let event = if is_map { Event::MapEnd } else { Event::SeqEnd };
            return self.emit_layered(LayerEvent::new(event));
        }
        let rv = self.core.emit_end(is_map);
        self.core.finish_event(rv)
    }

    /// Passes an event through the layers.
    ///
    /// This is marked as cold so that it does not affect the code emitting
    /// events when there are no layers.
    #[cold]
    #[inline(never)]
    fn emit_layered(&mut self, event: LayerEvent<'_, 'de>) -> Result<(), Error> {
        let rv = Next::new(&mut self.layers, &mut self.core).emit(event);
        self.core.finish_event(rv)
    }
}

impl<'de> DriverCore<'de> {
    /// Detaches the context of the event that was delivered.
    ///
    /// If the event failed, the context is attached to the error.
    #[inline(always)]
    fn finish_event(&mut self, rv: Result<(), Error>) -> Result<(), Error> {
        let rv = match rv {
            Ok(()) => Ok(()),
            Err(err) => Err(self.state.attach_error_context(err)),
        };
        self.state.clear_event();
        rv
    }

    /// Sets the position of the next event in the state.
    #[inline]
    pub(crate) fn update_position(&mut self, event: &Event<'_>) {
        self.state.is_map_key = match event {
            Event::MapEnd | Event::SeqEnd => false,
            _ => matches!(self.sink_stack.last(), Some((_, Container::Map(true)))),
        };
    }

    /// Delivers an event to the sinks.
    #[inline(always)]
    pub(crate) fn dispatch(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => self.emit_atom(atom),
            Event::MapStart(shape) => self.emit_start(true, shape),
            Event::SeqStart(shape) => self.emit_start(false, shape),
            Event::MapEnd => self.emit_end(true),
            Event::SeqEnd => self.emit_end(false),
        }
    }

    /// Delivers an event that borrows from the data to the sinks.
    #[inline(always)]
    pub(crate) fn dispatch_borrowed(&mut self, event: Event<'de>) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => self.emit_borrowed_atom(atom),
            Event::MapStart(shape) => self.emit_start(true, shape),
            Event::SeqStart(shape) => self.emit_start(false, shape),
            Event::MapEnd => self.emit_end(true),
            Event::SeqEnd => self.emit_end(false),
        }
    }

    #[inline(always)]
    fn emit_borrowed_atom(&mut self, atom: Atom<'de>) -> Result<(), Error> {
        match self.sink_stack.last_mut() {
            Some((sink, Container::Map(is_key))) => {
                let key = *is_key;
                *is_key = !key;
                self.state.is_map_key = key;
                if key {
                    sink.borrowed_key_atom(atom, &mut self.state)
                } else {
                    sink.borrowed_value_atom(atom, &mut self.state)
                }
            }
            Some((sink, Container::Seq)) => {
                self.state.is_map_key = false;
                sink.borrowed_value_atom(atom, &mut self.state)
            }
            None => {
                let sink = self.root.as_mut().expect("no active sink");
                sink.borrowed_atom(atom, &mut self.state)?;
                sink.finish(&mut self.state)
            }
        }
    }

    #[inline(always)]
    fn emit_atom(&mut self, atom: Atom) -> Result<(), Error> {
        match self.sink_stack.last_mut() {
            Some((sink, Container::Map(is_key))) => {
                let key = *is_key;
                *is_key = !key;
                self.state.is_map_key = key;
                if key {
                    sink.key_atom(atom, &mut self.state)
                } else {
                    sink.value_atom(atom, &mut self.state)
                }
            }
            Some((sink, Container::Seq)) => {
                self.state.is_map_key = false;
                sink.value_atom(atom, &mut self.state)
            }
            None => {
                let sink = self.root.as_mut().expect("no active sink");
                sink.atom(atom, &mut self.state)?;
                sink.finish(&mut self.state)
            }
        }
    }

    #[inline(always)]
    fn emit_start(&mut self, is_map: bool, shape: ContainerShape) -> Result<(), Error> {
        let mut sink = match self.sink_stack.last_mut() {
            Some((parent, Container::Map(is_key))) => {
                let key = *is_key;
                *is_key = !key;
                self.state.is_map_key = key;
                let sink = if key {
                    parent.next_key(&mut self.state)?
                } else {
                    parent.next_value(&mut self.state)?
                };
                // SAFETY: the sink borrows from the sink on the top of the
                // stack.  It's placed above it on the stack and dropped
                // before it.
                unsafe { erase_lifetime(sink) }
            }
            Some((parent, Container::Seq)) => {
                self.state.is_map_key = false;
                let sink = parent.next_value(&mut self.state)?;
                // SAFETY: see above
                unsafe { erase_lifetime(sink) }
            }
            None => self.root.take().expect("no active sink"),
        };
        self.state.container_shape = shape;
        let container = if is_map {
            sink.map(&mut self.state)?;
            Container::Map(true)
        } else {
            sink.seq(&mut self.state)?;
            Container::Seq
        };
        self.state.depth += 1;
        self.sink_stack.push((sink, container));
        Ok(())
    }

    #[inline(always)]
    fn emit_end(&mut self, is_map: bool) -> Result<(), Error> {
        match self.sink_stack.last() {
            Some((_, Container::Map(_))) if is_map => {}
            Some((_, Container::Seq)) if !is_map => {}
            _ => panic!("not inside a {}", if is_map { "map" } else { "sequence" }),
        }
        let (mut sink, _) = self.sink_stack.pop().unwrap();
        // the container remains the current one while it's finished as sinks
        // can still produce values within it (for instance by replaying
        // recorded values).
        let rv = sink.finish(&mut self.state);
        self.state.depth -= 1;
        if self.sink_stack.is_empty() {
            // the root sink is retained until the driver is dropped
            self.root = Some(sink);
        }
        rv
    }
}

impl<'de> Drop for DriverCore<'de> {
    fn drop(&mut self) {
        // sinks borrow from the sinks below them, drop them in inverse order
        while let Some(_item) = self.sink_stack.pop() {}
    }
}

#[test]
fn test_driver() {
    let mut out: Option<std::collections::BTreeMap<u32, String>> = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::map_start()).unwrap();
        driver.emit(1u64).unwrap();
        driver.emit("Hello").unwrap();
        driver.emit(2u64).unwrap();
        driver.emit("World").unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }

    let map = out.unwrap();
    assert_eq!(map[&1], "Hello");
    assert_eq!(map[&2], "World");
}
