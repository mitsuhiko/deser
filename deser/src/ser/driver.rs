use std::borrow::Cow;
use std::mem::ManuallyDrop;

use crate::error::Error;
use crate::extensions::Extensions;
use crate::ser::{Chunk, SerializerState};
use crate::{Descriptor, Event, Serialize};

use super::{MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};

/// The driver allows serializing a [`Serialize`] iteratively.
///
/// This is the only way to convert from a [`Serialize`] into an event
/// stream.  As a user one has to call [`next`](Self::next) until `None`
/// is returned, indicating the end of the event stream.
pub struct Driver<'a> {
    serializable: Option<SerializeHandle<'a>>,
    state: SerializerState<'static>,
    state_stack: ManuallyDrop<Vec<DriverState>>,
    emitter_stack: ManuallyDrop<Vec<Emitter>>,
    next_event: Option<(Event<'a>, &'a dyn Descriptor)>,
}

enum DriverState {
    Initial,
    ProcessChunk {
        chunk: Chunk<'static>,
        serializable: SerializeHandle<'static>,
    },
    SeqEmitterAdvance {
        serializable: SerializeHandle<'static>,
    },
    MapEmitterNextKey {
        serializable: SerializeHandle<'static>,
    },
    MapEmitterNextValue {
        serializable: SerializeHandle<'static>,
    },
    StructEmitterAdvance {
        serializable: SerializeHandle<'static>,
    },
    Serialize {
        serializable: SerializeHandle<'static>,
    },
    PopEmitter,
    FinishSerialize {
        serializable: SerializeHandle<'static>,
    },
}

enum Emitter {
    Seq(Box<dyn SeqEmitter>),
    Map(Box<dyn MapEmitter>),
    Struct(Box<dyn StructEmitter>),
}

impl<'a> Drop for Driver<'a> {
    fn drop(&mut self) {
        self.next_event.take();
        while let Some(_emitter) = self.emitter_stack.pop() {
            // drop in inverse order
        }
        while let Some(_emitter) = self.state_stack.pop() {
            // drop in inverse order
        }
        unsafe {
            ManuallyDrop::drop(&mut self.state_stack);
            ManuallyDrop::drop(&mut self.emitter_stack);
        }
    }
}

impl<'a> Driver<'a> {
    /// Creates a new driver which serializes the given value implementing [`Serialize`].
    pub fn new(serializable: &'a dyn Serialize) -> Driver<'a> {
        Driver {
            serializable: Some(SerializeHandle::Borrowed(serializable)),
            state: SerializerState {
                extensions: Extensions::default(),
                descriptor_stack: Vec::new(),
            },
            emitter_stack: ManuallyDrop::new(Vec::new()),
            state_stack: ManuallyDrop::new(vec![DriverState::Initial]),
            next_event: None,
        }
    }

    /// Returns a borrowed reference to the current serializer state.
    pub fn state(&self) -> &SerializerState {
        &self.state
    }

    /// Produces the next serialization event.
    ///
    /// # Panics
    ///
    /// The driver will panic if the data fed from the serializer is malformed.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<(Event, &dyn Descriptor, &SerializerState)>, Error> {
        self.advance()?;
        Ok(self
            .next_event
            .take()
            .map(|(event, descriptor)| (event, descriptor, &self.state)))
    }

    fn advance(&mut self) -> Result<(), Error> {
        macro_rules! top_emitter {
            ($ty:ident) => {
                match self.emitter_stack.last_mut() {
                    Some(Emitter::$ty(emitter)) => emitter,
                    _ => unreachable!(),
                }
            };
        }

        while let Some(state) =
            unsafe { extend_lifetime!(self.state_stack.pop(), Option<DriverState>) }
        {
            match state {
                DriverState::Initial => {
                    let serializable = unsafe {
                        extend_lifetime!(self.serializable.take().unwrap(), SerializeHandle)
                    };
                    let chunk =
                        unsafe { extend_lifetime!(serializable.serialize(&self.state)?, Chunk) };
                    self.state_stack.push(DriverState::ProcessChunk {
                        chunk,
                        serializable,
                    })
                }
                DriverState::ProcessChunk {
                    chunk,
                    serializable,
                } => match chunk {
                    Chunk::Atom(atom) => {
                        self.next_event = Some((Event::Atom(atom), unsafe {
                            extend_lifetime!(serializable.descriptor(), &dyn Descriptor)
                        }));
                        self.state_stack
                            .push(DriverState::FinishSerialize { serializable });
                        return Ok(());
                    }
                    Chunk::Struct(emitter) => {
                        let descriptor =
                            unsafe { extend_lifetime!(serializable.descriptor(), &dyn Descriptor) };
                        self.next_event = Some((Event::MapStart, descriptor));
                        self.emitter_stack.push(Emitter::Struct(emitter));
                        self.state_stack
                            .push(DriverState::StructEmitterAdvance { serializable });
                        self.state.descriptor_stack.push(descriptor);
                        return Ok(());
                    }
                    Chunk::Map(emitter) => {
                        let descriptor =
                            unsafe { extend_lifetime!(serializable.descriptor(), &dyn Descriptor) };
                        self.next_event = Some((Event::MapStart, descriptor));
                        self.emitter_stack.push(Emitter::Map(emitter));
                        self.state_stack
                            .push(DriverState::MapEmitterNextKey { serializable });
                        self.state.descriptor_stack.push(descriptor);
                        return Ok(());
                    }
                    Chunk::Seq(emitter) => {
                        let descriptor =
                            unsafe { extend_lifetime!(serializable.descriptor(), &dyn Descriptor) };
                        self.next_event = Some((Event::SeqStart, descriptor));
                        self.emitter_stack.push(Emitter::Seq(emitter));
                        self.state_stack
                            .push(DriverState::SeqEmitterAdvance { serializable });
                        self.state.descriptor_stack.push(descriptor);
                        return Ok(());
                    }
                },
                DriverState::SeqEmitterAdvance { serializable } => {
                    let emitter = top_emitter!(Seq);
                    match unsafe {
                        extend_lifetime!(emitter.next(&self.state)?, Option<SerializeHandle>)
                    } {
                        Some(item_serializable) => {
                            // continue iteration
                            self.state_stack
                                .push(DriverState::SeqEmitterAdvance { serializable });
                            // and serialize the current item
                            self.state_stack.push(DriverState::Serialize {
                                serializable: item_serializable,
                            });
                        }
                        None => {
                            self.state_stack.push(DriverState::PopEmitter);
                        }
                    }
                }
                DriverState::MapEmitterNextKey { serializable } => {
                    let emitter = top_emitter!(Map);
                    match unsafe {
                        extend_lifetime!(emitter.next_key(&self.state)?, Option<SerializeHandle>)
                    } {
                        Some(key_serializable) => {
                            // continue with value
                            self.state_stack
                                .push(DriverState::MapEmitterNextValue { serializable });
                            // and serialize the current key
                            self.state_stack.push(DriverState::Serialize {
                                serializable: key_serializable,
                            });
                        }
                        None => {
                            self.state_stack.push(DriverState::PopEmitter);
                        }
                    }
                }
                DriverState::MapEmitterNextValue { serializable } => {
                    let emitter = top_emitter!(Map);
                    let value_serializable = unsafe {
                        extend_lifetime!(emitter.next_value(&self.state)?, SerializeHandle)
                    };
                    // continue with key again
                    self.state_stack
                        .push(DriverState::MapEmitterNextKey { serializable });
                    // and serialize the current value
                    self.state_stack.push(DriverState::Serialize {
                        serializable: value_serializable,
                    });
                }
                DriverState::StructEmitterAdvance { serializable } => {
                    let emitter = top_emitter!(Struct);
                    match unsafe {
                        extend_lifetime!(
                            emitter.next(&self.state)?,
                            Option<(Cow<'_, str>, SerializeHandle)>
                        )
                    } {
                        Some((key, value_serializable)) => {
                            // continue iteration
                            self.state_stack
                                .push(DriverState::StructEmitterAdvance { serializable });
                            // and serialize key and value
                            self.state_stack.push(DriverState::Serialize {
                                serializable: value_serializable,
                            });
                            self.state_stack.push(DriverState::Serialize {
                                serializable: SerializeHandle::boxed(key),
                            });
                        }
                        None => {
                            self.state_stack.push(DriverState::PopEmitter);
                        }
                    }
                }
                DriverState::Serialize { serializable } => {
                    let chunk =
                        unsafe { extend_lifetime!(serializable.serialize(&self.state)?, Chunk) };
                    self.state_stack.push(DriverState::ProcessChunk {
                        chunk,
                        serializable,
                    });
                }
                DriverState::PopEmitter => {
                    let descriptor = self.state.descriptor_stack.pop().unwrap();
                    self.next_event = Some((
                        match self.emitter_stack.pop().unwrap() {
                            Emitter::Seq(_) => Event::SeqEnd,
                            Emitter::Map(_) | Emitter::Struct(_) => Event::MapEnd,
                        },
                        descriptor,
                    ));
                    return Ok(());
                }
                DriverState::FinishSerialize { serializable } => {
                    serializable.finish(&self.state)?;
                }
            }
        }

        Ok(())
    }
}

#[test]
fn test_seq_emitting() {
    let vec = vec![vec![1u64, 2], vec![3, 4]];

    let mut driver = Driver::new(&vec);
    let mut events = Vec::new();
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }

    assert_eq!(
        events,
        vec![
            Event::SeqStart,
            Event::SeqStart,
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::SeqStart,
            3u64.into(),
            4u64.into(),
            Event::SeqEnd,
            Event::SeqEnd,
        ],
    );
}

#[test]
fn test_map_emitting() {
    let mut map = std::collections::BTreeMap::new();
    map.insert((1u32, 2u32), "first");
    map.insert((2, 3), "second");

    let mut driver = Driver::new(&map);
    let mut events = Vec::new();
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }

    assert_eq!(
        events,
        vec![
            Event::MapStart,
            Event::SeqStart,
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            "first".into(),
            Event::SeqStart,
            2u64.into(),
            3u64.into(),
            Event::SeqEnd,
            "second".into(),
            Event::MapEnd
        ]
    );
}
