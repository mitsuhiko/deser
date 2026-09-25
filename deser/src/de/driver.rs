use std::marker::PhantomData;

use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::event::{Atom, Event};
use crate::State;

/// The driver allows emitting deserialization events into a [`Deserialize`].
///
/// This is a convenient way to safely drive a [`Sink`](crate::de::Sink) of a [`Deserialize`]
/// without using the runtime stack.  As rust lifetimes make what this type does
/// internally impossible with safe code, this is a safe abstractiont that
/// hides the unsafety internally.
pub struct DeserializeDriver<'a> {
    state: State,
    // The sinks borrow from each other: every sink on the stack can borrow
    // from the sink below it.  The lifetimes are erased to `'static` and
    // it's the driver's responsibility to never use a sink while one of the
    // sinks it lent out is still alive and to drop them in inverse order.
    //
    // `root` holds the sink the driver was created with while no container
    // is open.
    root: Option<SinkHandle<'static>>,
    sink_stack: Vec<(SinkHandle<'static>, Layer)>,
    // the sinks borrow for 'a
    _marker: PhantomData<&'a mut ()>,
}

const STACK_CAPACITY: usize = 128;

#[derive(Copy, Clone)]
enum Layer {
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
unsafe fn erase_lifetime(handle: SinkHandle<'_>) -> SinkHandle<'static> {
    std::mem::transmute::<SinkHandle<'_>, SinkHandle<'static>>(handle)
}

impl<'a> DeserializeDriver<'a> {
    /// Creates a new deserializer driver.
    pub fn new<T: Deserialize>(out: &'a mut Option<T>) -> DeserializeDriver<'a> {
        DeserializeDriver::from_sink(T::deserialize_into(out))
    }

    /// Creates a new deserializer driver from a sink.
    pub fn from_sink(sink: SinkHandle<'a>) -> DeserializeDriver<'a> {
        DeserializeDriver::with_state(State::new(), sink)
    }

    /// Runs a nested driver within an ongoing deserialization.
    ///
    /// The nested driver continues on the state of the ongoing
    /// deserialization: the extensions are shared and the containers opened
    /// by the nested driver are placed on top of the ones that are currently
    /// open.  This is used to replay recorded events so that replayed values
    /// observe the same state as values that were not buffered.
    pub(crate) fn nested<R>(
        state: &mut State,
        sink: SinkHandle<'_>,
        is_map_key: bool,
        f: impl FnOnce(&mut DeserializeDriver<'_>) -> R,
    ) -> R {
        let depth = state.descriptor_stack.len();
        let outer_is_map_key = state.is_map_key;
        let mut driver = DeserializeDriver::with_state(state.take(), sink);
        driver.state.is_map_key = is_map_key;
        let rv = f(&mut driver);
        *state = driver.state.take();
        drop(driver);
        // a failed replay can leave containers open
        state.descriptor_stack.truncate(depth);
        state.is_map_key = outer_is_map_key;
        rv
    }

    fn with_state(state: State, sink: SinkHandle<'a>) -> DeserializeDriver<'a> {
        DeserializeDriver {
            state,
            sink_stack: Vec::with_capacity(STACK_CAPACITY),
            // SAFETY: the driver cannot outlive 'a
            root: Some(unsafe { erase_lifetime(sink) }),
            _marker: PhantomData,
        }
    }

    /// Returns a borrowed reference to the current deserializer state.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Returns a mutable reference to the current deserializer state.
    ///
    /// Formats use this to publish information for the event they emit
    /// next into the state.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// Emits an event into the driver.
    ///
    /// # Panics
    ///
    /// The driver keeps an internal state and emitting events when they are
    /// not expected will cause the driver to panic.
    #[inline]
    pub fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        match event.into() {
            Event::Atom(atom) => self.emit_atom(atom),
            Event::MapStart => self.emit_start(true),
            Event::SeqStart => self.emit_start(false),
            Event::MapEnd => self.emit_end(true),
            Event::SeqEnd => self.emit_end(false),
        }
    }

    fn emit_atom(&mut self, atom: Atom) -> Result<(), Error> {
        match self.sink_stack.last_mut() {
            Some((sink, Layer::Map(ref mut is_key))) => {
                let key = *is_key;
                *is_key = !key;
                self.state.is_map_key = key;
                if key {
                    sink.key_atom(atom, &mut self.state)
                } else {
                    sink.value_atom(atom, &mut self.state)
                }
            }
            Some((sink, Layer::Seq)) => {
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

    fn emit_start(&mut self, is_map: bool) -> Result<(), Error> {
        let mut sink = match self.sink_stack.last_mut() {
            Some((parent, Layer::Map(ref mut is_key))) => {
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
            Some((parent, Layer::Seq)) => {
                self.state.is_map_key = false;
                let sink = parent.next_value(&mut self.state)?;
                // SAFETY: see above
                unsafe { erase_lifetime(sink) }
            }
            None => self.root.take().expect("no active sink"),
        };
        let layer = if is_map {
            sink.map(&mut self.state)?;
            Layer::Map(true)
        } else {
            sink.seq(&mut self.state)?;
            Layer::Seq
        };
        self.state.descriptor_stack.push(sink.descriptor());
        self.sink_stack.push((sink, layer));
        Ok(())
    }

    fn emit_end(&mut self, is_map: bool) -> Result<(), Error> {
        match self.sink_stack.last() {
            Some((_, Layer::Map(_))) if is_map => {}
            Some((_, Layer::Seq)) if !is_map => {}
            _ => panic!("not inside a {}", if is_map { "map" } else { "sequence" }),
        }
        let (mut sink, _) = self.sink_stack.pop().unwrap();
        // the container remains the current one while it's finished as sinks
        // can still produce values within it (for instance by replaying
        // recorded values).
        let rv = sink.finish(&mut self.state);
        self.state.descriptor_stack.pop();
        if self.sink_stack.is_empty() {
            // the root sink is retained until the driver is dropped
            self.root = Some(sink);
        }
        rv
    }
}

impl<'a> Drop for DeserializeDriver<'a> {
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
        driver.emit(Event::MapStart).unwrap();
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
