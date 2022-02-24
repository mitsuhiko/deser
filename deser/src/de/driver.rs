use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use crate::de::{Deserialize, DeserializerState, SinkHandle};
use crate::error::Error;
use crate::event::Event;
use crate::extensions::Extensions;

/// The driver allows emitting deserialization events into a [`Deserialize`].
///
/// This is a convenient way to safely drive a [`Sink`](crate::de::Sink) of a [`Deserialize`]
/// without using the runtime stack.  As rust lifetimes make what this type does
/// internally impossible with safe code, this is a safe abstractiont that
/// hides the unsafety internally.
pub struct DeserializeDriver<'a> {
    state: DeserializerState<'a>,
    current_sink: Option<SinkHandle<'static>>,
    sink_stack: ManuallyDrop<Vec<(SinkHandle<'static>, Layer)>>,
}

const STACK_CAPACITY: usize = 128;

enum Layer {
    Map(bool),
    Seq,
}

impl<'a> DeserializeDriver<'a> {
    /// Creates a new deserializer driver.
    pub fn new<T: Deserialize>(out: &'a mut Option<T>) -> DeserializeDriver<'a> {
        DeserializeDriver::from_sink(T::deserialize_into(out))
    }

    /// Creates a new deserializer driver from a sink.
    pub fn from_sink(sink: SinkHandle) -> DeserializeDriver<'a> {
        DeserializeDriver {
            state: DeserializerState {
                extensions: Extensions::default(),
                depth: 0,
                _marker: PhantomData,
            },
            sink_stack: ManuallyDrop::new(Vec::with_capacity(STACK_CAPACITY)),
            current_sink: Some(unsafe { extend_lifetime!(sink, SinkHandle<'_>) }),
        }
    }

    /// Returns a borrowed reference to the current deserializer state.
    pub fn state(&self) -> &DeserializerState {
        &self.state
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
        match self.sink_stack.last_mut() {
            Some((map_sink, Layer::Map(ref mut is_key))) => {
                let next_sink = if *is_key {
                    map_sink.next_key(&self.state)?
                } else {
                    map_sink.next_value(&self.state)?
                };
                *is_key = !*is_key;
                self.current_sink = Some(unsafe { extend_lifetime!(next_sink, SinkHandle<'_>) });
            }
            Some((seq_sink, Layer::Seq)) => {
                self.current_sink = Some(unsafe {
                    extend_lifetime!(seq_sink.next_value(&self.state)?, SinkHandle<'_>)
                });
            }
            None => {}
        }
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
                current_sink.atom(atom, &self.state)?;
                current_sink.finish(&self.state)?;
            }
            Event::MapStart => {
                let current_sink = current_sink!();
                current_sink.map(&self.state)?;
                self.state.depth += 1;
                self.sink_stack
                    .push((self.current_sink.take().unwrap(), Layer::Map(true)));
                return Ok(());
            }
            Event::MapEnd => match self.sink_stack.pop() {
                Some((mut map_sink, Layer::Map(_))) => {
                    map_sink.finish(&self.state)?;
                    self.state.depth -= 1;
                    self.current_sink = Some(map_sink);
                }
                _ => panic!("not inside a MapSink"),
            },
            Event::SeqStart => {
                let current_sink = current_sink!();
                current_sink.seq(&self.state)?;
                self.state.depth += 1;
                self.sink_stack
                    .push((self.current_sink.take().unwrap(), Layer::Seq));
                return Ok(());
            }
            Event::SeqEnd => match self.sink_stack.pop() {
                Some((mut seq_sink, Layer::Seq)) => {
                    seq_sink.finish(&self.state)?;
                    self.state.depth -= 1;
                    self.current_sink = Some(seq_sink);
                }
                _ => panic!("not inside a SeqSink"),
            },
        }

        Ok(())
    }
}

impl<'a> Drop for DeserializeDriver<'a> {
    fn drop(&mut self) {
        unsafe {
            while let Some(_item) = self.sink_stack.pop() {
                // drop in inverse order
            }
            ManuallyDrop::drop(&mut self.sink_stack);
        }
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
