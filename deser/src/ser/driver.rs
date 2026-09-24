use std::borrow::Cow;
use std::marker::PhantomData;

use crate::descriptors::NamedDescriptor;
use crate::error::Error;
use crate::extensions::Extensions;
use crate::ser::{Chunk, SerializerState};
use crate::{Atom, Descriptor, Event, Serialize};

use super::{MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};

/// The driver allows serializing a [`Serialize`] iteratively.
///
/// This is the only way to convert from a [`Serialize`] into an event
/// stream.  As a user one has to call [`next`](Self::next) until `None`
/// is returned, indicating the end of the event stream.
pub struct SerializeDriver<'a> {
    state: SerializerState<'static>,
    // Frames refer to data borrowed from the serializables of the frames
    // below them, which is why the lifetimes are erased to `'static`.
    stack: Vec<Frame>,
    _marker: PhantomData<&'a dyn Serialize>,
}

static STRUCT_KEY_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };

/// A single value that is currently being serialized.
struct Frame {
    // `phase` must be declared (and thus dropped) before `serializable` as
    // the emitters borrow from the serializable.
    phase: Phase,
    serializable: SerializeHandle<'static>,
}

enum Phase {
    /// The value needs to be serialized.
    Pending,
    /// The value was fully emitted, `finish` needs to be called.
    Finish,
    Seq(Box<dyn SeqEmitter>),
    Map(Box<dyn MapEmitter>, bool),
    Struct(Box<dyn StructEmitter>),
}

impl<'a> Drop for SerializeDriver<'a> {
    fn drop(&mut self) {
        // inner frames can borrow from outer frames, drop in inverse order.
        while let Some(_frame) = self.stack.pop() {}
    }
}

const STACK_CAPACITY: usize = 128;

type NextEvent<'a> = Option<(Event<'a>, &'a dyn Descriptor)>;

impl<'a> SerializeDriver<'a> {
    /// Creates a new driver which serializes the given value implementing [`Serialize`].
    pub fn new(serializable: &'a dyn Serialize) -> SerializeDriver<'a> {
        // SAFETY: the driver cannot outlive 'a
        let serializable = unsafe {
            std::mem::transmute::<SerializeHandle<'_>, SerializeHandle<'static>>(
                SerializeHandle::Borrowed(serializable),
            )
        };
        let mut stack = Vec::with_capacity(STACK_CAPACITY);
        stack.push(Frame {
            phase: Phase::Pending,
            serializable,
        });
        SerializeDriver {
            state: SerializerState {
                extensions: Extensions::default(),
                descriptor_stack: Vec::with_capacity(STACK_CAPACITY),
            },
            stack,
            _marker: PhantomData,
        }
    }

    /// Returns a borrowed reference to the current serializer state.
    pub fn state(&self) -> &SerializerState<'_> {
        &self.state
    }

    /// Produces the next serialization event.
    ///
    /// # Panics
    ///
    /// The driver will panic if the data fed from the serializer is malformed.
    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn next(
        &mut self,
    ) -> Result<Option<(Event<'_>, &dyn Descriptor, &SerializerState<'_>)>, Error> {
        Ok(self
            .advance()?
            .map(|(event, descriptor)| (event, descriptor, &self.state)))
    }

    /// Advances the driver.
    ///
    /// The returned event is bound to `'static` but it actually borrows from
    /// the frames on the stack.  It's only valid until the next call.
    fn advance(&mut self) -> Result<NextEvent<'static>, Error> {
        while let Some(frame) = self.stack.last_mut() {
            // The events produced borrow from the frames on the stack.  They
            // stay alive until the next call to `advance` as frames are only
            // popped at the start of the loop.
            let frame = unsafe { std::mem::transmute::<&mut Frame, &'static mut Frame>(frame) };
            match frame.phase {
                Phase::Pending => {
                    let serializable = &*frame.serializable;
                    let descriptor = serializable.descriptor();
                    let chunk = serializable.serialize(&mut self.state)?;
                    let chunk = unsafe { std::mem::transmute::<Chunk<'_>, Chunk<'static>>(chunk) };
                    let event = match chunk {
                        Chunk::Atom(atom) => {
                            frame.phase = Phase::Finish;
                            return Ok(Some((Event::Atom(atom), descriptor)));
                        }
                        Chunk::Struct(emitter) => {
                            frame.phase = Phase::Struct(emitter);
                            Event::MapStart
                        }
                        Chunk::Map(emitter) => {
                            frame.phase = Phase::Map(emitter, false);
                            Event::MapStart
                        }
                        Chunk::Seq(emitter) => {
                            frame.phase = Phase::Seq(emitter);
                            Event::SeqStart
                        }
                    };
                    self.state.descriptor_stack.push(descriptor);
                    return Ok(Some((event, descriptor)));
                }
                Phase::Finish => {
                    let frame = self.stack.pop().unwrap();
                    frame.serializable.finish(&mut self.state)?;
                    continue;
                }
                Phase::Seq(ref mut emitter) => {
                    if let Some(item) = emitter.next(&mut self.state)? {
                        self.push(item);
                        continue;
                    }
                }
                Phase::Map(ref mut emitter, ref mut is_value) => {
                    if *is_value {
                        *is_value = false;
                        let value = emitter.next_value(&mut self.state)?;
                        self.push(value);
                        continue;
                    } else if let Some(key) = emitter.next_key(&mut self.state)? {
                        *is_value = true;
                        self.push(key);
                        continue;
                    }
                }
                Phase::Struct(ref mut emitter) => {
                    if let Some((key, value)) = emitter.next(&mut self.state)? {
                        // the key is emitted directly as event, the value
                        // is serialized on the next iteration.
                        let key =
                            unsafe { std::mem::transmute::<Cow<'_, str>, Cow<'static, str>>(key) };
                        self.push(value);
                        return Ok(Some((Event::Atom(Atom::Str(key)), &STRUCT_KEY_DESCRIPTOR)));
                    }
                }
            }

            // if we make it here, a container was exhausted.  The emitter is
            // dropped and the serializable is finished on the next call.
            let event = match frame.phase {
                Phase::Seq(_) => Event::SeqEnd,
                _ => Event::MapEnd,
            };
            frame.phase = Phase::Finish;
            let descriptor = self.state.descriptor_stack.pop().unwrap();
            return Ok(Some((event, descriptor)));
        }

        Ok(None)
    }

    #[inline(always)]
    fn push(&mut self, serializable: SerializeHandle<'_>) {
        self.stack.push(Frame {
            phase: Phase::Pending,
            serializable: unsafe {
                std::mem::transmute::<SerializeHandle<'_>, SerializeHandle<'static>>(serializable)
            },
        });
    }
}

#[test]
fn test_seq_emitting() {
    let vec = vec![vec![1u64, 2], vec![3, 4]];

    let mut driver = SerializeDriver::new(&vec);
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

    let mut driver = SerializeDriver::new(&map);
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
