use crate::de::{Deserialize, DeserializerState, SinkHandle};
use crate::error::Error;
use crate::event::Event;

/// The driver allows emitting deserialization events into a [`Deserialize`].
///
/// This is a convenient way to safely drive a [`Sink`](crate::de::Sink) of a [`Deserialize`]
/// without using the runtime stack.  As rust lifetimes make what this type does
/// internally impossible with safe code, this is a safe abstractiont that
/// hides the unsafety internally.
pub struct DeserializeDriver<'a> {
    state: DeserializerState<'a>,
    // The sinks borrow from each other: every sink on the stack (and the
    // current sink) can borrow from the sink below it.  The lifetimes are
    // erased to `'static` and it's the driver's responsibility to never
    // use a sink while one of the sinks it lent out is still alive and to
    // drop them in inverse order.
    current_sink: Option<SinkHandle<'static>>,
    sink_stack: Vec<(SinkHandle<'static>, Layer)>,
}

const STACK_CAPACITY: usize = 128;

enum Layer {
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
        DeserializeDriver::with_state(DeserializerState::new(None), sink)
    }

    /// Creates a driver that shares the extensions of another state.
    ///
    /// This is used to replay recorded events within an ongoing
    /// deserialization.
    pub(crate) fn nested(
        parent: &'a mut DeserializerState<'_>,
        sink: SinkHandle<'a>,
        is_map_key: bool,
    ) -> DeserializeDriver<'a> {
        let mut state = DeserializerState::new(Some(parent.extensions_mut()));
        state.is_map_key = is_map_key;
        DeserializeDriver::with_state(state, sink)
    }

    fn with_state(state: DeserializerState<'a>, sink: SinkHandle<'a>) -> DeserializeDriver<'a> {
        DeserializeDriver {
            state,
            sink_stack: Vec::with_capacity(STACK_CAPACITY),
            // SAFETY: the driver cannot outlive 'a
            current_sink: Some(unsafe { erase_lifetime(sink) }),
        }
    }

    /// Returns a borrowed reference to the current deserializer state.
    pub fn state(&self) -> &DeserializerState<'a> {
        &self.state
    }

    /// Returns a mutable reference to the current deserializer state.
    ///
    /// Formats use this to publish information for the event they emit
    /// next into the state.
    pub fn state_mut(&mut self) -> &mut DeserializerState<'a> {
        &mut self.state
    }

    /// Emits an event into the driver.
    ///
    /// # Panics
    ///
    /// The driver keeps an internal state and emitting events when they are
    /// not expected will cause the driver to panic.
    pub fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        self._emit(event.into())
    }

    fn update_current_sink(&mut self) -> Result<(), Error> {
        if self.sink_stack.is_empty() {
            return Ok(());
        }

        // The current sink was handed out by the sink on the top of the
        // stack and might borrow from it.  It has to be dropped before the
        // parent sink is used again.
        self.current_sink = None;

        let next_sink = match self.sink_stack.last_mut() {
            Some((map_sink, Layer::Map(ref mut is_key))) => {
                let next_sink = if *is_key {
                    map_sink.next_key(&mut self.state)?
                } else {
                    map_sink.next_value(&mut self.state)?
                };
                self.state.is_map_key = *is_key;
                *is_key = !*is_key;
                next_sink
            }
            Some((seq_sink, Layer::Seq)) => {
                self.state.is_map_key = false;
                seq_sink.next_value(&mut self.state)?
            }
            None => unreachable!(),
        };

        // SAFETY: the sink borrows from the sink on the top of the stack.  It
        // is dropped before that sink is used again or dropped.
        self.current_sink = Some(unsafe { erase_lifetime(next_sink) });
        Ok(())
    }

    fn _emit(&mut self, event: Event) -> Result<(), Error> {
        macro_rules! current_sink {
            () => {{
                self.update_current_sink()?;
                self.current_sink.as_mut().expect("no active sink")
            }};
        }

        match event {
            Event::Atom(atom) => {
                let current_sink = current_sink!();
                current_sink.atom(atom, &mut self.state)?;
                current_sink.finish(&mut self.state)?;
            }
            Event::MapStart | Event::SeqStart => {
                let current_sink = current_sink!();
                let layer = if let Event::MapStart = event {
                    current_sink.map(&mut self.state)?;
                    Layer::Map(true)
                } else {
                    current_sink.seq(&mut self.state)?;
                    Layer::Seq
                };
                self.state.descriptor_stack.push(current_sink.descriptor());
                self.sink_stack
                    .push((self.current_sink.take().unwrap(), layer));
            }
            Event::MapEnd | Event::SeqEnd => {
                let is_map = matches!(event, Event::MapEnd);
                match self.sink_stack.last() {
                    Some((_, Layer::Map(_))) if is_map => {}
                    Some((_, Layer::Seq)) if !is_map => {}
                    _ => panic!("not inside a {}", if is_map { "map" } else { "sequence" }),
                }
                // the last value sink borrows from the container sink, drop
                // it before finishing the container.
                self.current_sink = None;
                let (mut sink, _) = self.sink_stack.pop().unwrap();
                self.state.descriptor_stack.pop();
                sink.finish(&mut self.state)?;
                self.current_sink = Some(sink);
            }
        }

        Ok(())
    }
}

impl<'a> Drop for DeserializeDriver<'a> {
    fn drop(&mut self) {
        // sinks borrow from the sinks below them, drop them in inverse order
        self.current_sink = None;
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
