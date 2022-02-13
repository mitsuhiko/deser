use std::mem::ManuallyDrop;

use crate::de::{Deserialize, DeserializerState, SinkHandle};
use crate::descriptors::Descriptor;
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
    current_sink: Option<SinkHandleWrapper>,
    sink_stack: ManuallyDrop<Vec<(SinkHandleWrapper, Layer)>>,
}

struct SinkHandleWrapper {
    sink: SinkHandle<'static>,
    used: bool,
}

impl SinkHandleWrapper {
    unsafe fn from<'a>(sink: SinkHandle<'a>) -> SinkHandleWrapper {
        SinkHandleWrapper {
            sink: extend_lifetime!(sink, SinkHandle<'_>),
            used: false,
        }
    }
}

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
                descriptor_stack: Vec::new(),
            },
            sink_stack: ManuallyDrop::new(Default::default()),
            current_sink: Some(unsafe { SinkHandleWrapper::from(sink) }),
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
    /// not expected will cause the driver to panic.  For instance trying to
    /// feed two events into a sink that was already used is guarded against.
    /// Likewise sending an unexpected `MapEnd` event or similar into the
    /// driver will cause a panic.
    pub fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        self._emit(event.into())
    }

    fn update_current_sink(&mut self) -> Result<(), Error> {
        match self.sink_stack.last_mut() {
            Some((map_sink, Layer::Map(ref mut is_key))) => {
                let next_sink = if *is_key {
                    map_sink.sink.next_key(&self.state)?
                } else {
                    map_sink.sink.next_value(&self.state)?
                };
                *is_key = !*is_key;
                self.current_sink = Some(unsafe { SinkHandleWrapper::from(next_sink) });
            }
            Some((seq_sink, Layer::Seq)) => {
                self.current_sink = Some(unsafe {
                    SinkHandleWrapper::from(seq_sink.sink.next_value(&self.state)?)
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
                let top = self.current_sink.as_mut().expect("no active sink");
                if top.used {
                    panic!("sink has already been used");
                } else {
                    &mut top.sink
                }
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
                let descriptor = current_sink.descriptor();
                self.state
                    .descriptor_stack
                    .push(unsafe { extend_lifetime!(descriptor, &dyn Descriptor) });
                self.sink_stack
                    .push((self.current_sink.take().unwrap(), Layer::Map(true)));
                return Ok(());
            }
            Event::MapEnd => match self.sink_stack.pop() {
                Some((mut map_sink, Layer::Map(_))) => {
                    map_sink.sink.finish(&self.state)?;
                    self.state.descriptor_stack.pop();
                    self.current_sink = Some(map_sink);
                }
                _ => panic!("not inside a MapSink"),
            },
            Event::SeqStart => {
                let current_sink = current_sink!();
                current_sink.seq(&self.state)?;
                let descriptor = current_sink.descriptor();
                self.state
                    .descriptor_stack
                    .push(unsafe { extend_lifetime!(descriptor, &dyn Descriptor) });
                self.sink_stack
                    .push((self.current_sink.take().unwrap(), Layer::Seq));
                return Ok(());
            }
            Event::SeqEnd => match self.sink_stack.pop() {
                Some((mut seq_sink, Layer::Seq)) => {
                    seq_sink.sink.finish(&self.state)?;
                    self.state.descriptor_stack.pop();
                    self.current_sink = Some(seq_sink);
                }
                _ => panic!("not inside a SeqSink"),
            },
        }

        self.current_sink.as_mut().unwrap().used = true;

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
