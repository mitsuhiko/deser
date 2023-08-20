use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::Deref;

use crate::error::Error;
use crate::extensions::Extensions;
use crate::ser::Chunk;
use crate::{Descriptor, Event, Serialize};

use super::{MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};

/// Utility enum providing access to the current container state in the driver.
#[derive(Debug, Copy, Clone)]
pub enum ContainerState {
    Seq { first: bool },
    Map { first: bool, key_pos: bool },
}

/// The current state of the serializer.
///
/// During serializer the [`SerializerState`] acts as a communciation device between
/// the serializable types as the serializer.
pub struct SerializerState<'a> {
    extensions: Extensions,
    descriptor_stack: Vec<&'a dyn Descriptor>,
    container_stack: Vec<ContainerState>,
}

impl<'a> fmt::Debug for SerializerState<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Stack<'a>(&'a [&'a dyn Descriptor]);
        struct Entry<'a>(&'a dyn Descriptor);

        impl<'a> fmt::Debug for Entry<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Layer")
                    .field("type_name", &self.0.name())
                    .field("precision", &self.0.precision())
                    .field("unordered", &self.0.unordered())
                    .finish()
            }
        }

        impl<'a> fmt::Debug for Stack<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut l = f.debug_list();
                for item in self.0.iter() {
                    l.entry(&Entry(*item));
                }
                l.finish()
            }
        }

        f.debug_struct("SerializerState")
            .field("extensions", &self.extensions)
            .field("stack", &Stack(&self.descriptor_stack))
            .field("container_state", &self.container_stack.last())
            .finish()
    }
}

impl<'a> SerializerState<'a> {
    /// Returns an extension value.
    pub fn get<T: Default + fmt::Debug + 'static>(&self) -> Ref<'_, T> {
        self.extensions.get()
    }

    /// Returns a mutable extension value.
    pub fn get_mut<T: Default + fmt::Debug + 'static>(&self) -> RefMut<'_, T> {
        self.extensions.get_mut()
    }

    /// Returns the topmost descriptor.
    ///
    /// This descriptor always points to a container as the descriptor of a value itself
    /// will always be passed to the callback explicitly.
    pub fn top_descriptor(&self) -> Option<&dyn Descriptor> {
        self.descriptor_stack.last().copied()
    }

    /// Returns the current recursion depth.
    pub fn depth(&self) -> usize {
        self.descriptor_stack.len()
    }

    /// Returns the current container state.
    #[inline(always)]
    pub fn container_state(&self) -> Option<ContainerState> {
        self.container_stack.last().copied()
    }
}

/// The driver allows serializing a [`Serialize`] iteratively.
///
/// This is the only way to convert from a [`Serialize`] into an event
/// stream.  As a user one has to call [`next`](Self::next) until `None`
/// is returned, indicating the end of the event stream.
pub struct SerializeDriver<'a> {
    serializer_state: SerializerState<'static>,
    driver_state: DriverState,
    serializable_stack: ManuallyDrop<Vec<SerializableOnStack>>,
    emitter_stack: ManuallyDrop<Vec<Emitter>>,
    next_event: Option<(Event<'a>, &'a dyn Descriptor)>,
}

// We like to hold on to Cow<'_, str> in addition to a SerializeHandle
// so we can get away without an extra boxed allocation.
enum SerializableOnStack {
    Handle(SerializeHandle<'static>),
    StrCow(Cow<'static, str>),
}

impl Deref for SerializableOnStack {
    type Target = dyn Serialize;

    fn deref(&self) -> &Self::Target {
        match self {
            SerializableOnStack::Handle(handle) => &**handle,
            SerializableOnStack::StrCow(cow) => &*cow,
        }
    }
}

enum DriverState {
    SeqNextItem,
    MapNextKey,
    MapNextValue,
    StructNextPair,
    Serialize,
    PushMapState,
    PushStructState,
    PushSeqState,
    PopEmitter,
    FinishSerialize,
}

enum Emitter {
    Seq(Box<dyn SeqEmitter>),
    Map(Box<dyn MapEmitter>),
    Struct(Box<dyn StructEmitter>),
}

impl<'a> Drop for SerializeDriver<'a> {
    fn drop(&mut self) {
        self.next_event.take();
        while let Some(_emitter) = self.emitter_stack.pop() {
            // drop in inverse order
        }
        while let Some(_emitter) = self.serializable_stack.pop() {
            // drop in inverse order
        }
        unsafe {
            ManuallyDrop::drop(&mut self.serializable_stack);
            ManuallyDrop::drop(&mut self.emitter_stack);
        }
    }
}

const STACK_CAPACITY: usize = 128;

impl<'a> SerializeDriver<'a> {
    /// Creates a new driver which serializes the given value implementing [`Serialize`].
    pub fn new(serializable: &'a dyn Serialize) -> SerializeDriver<'a> {
        let serializable =
            unsafe { extend_lifetime!(SerializeHandle::Borrowed(serializable), SerializeHandle) };
        SerializeDriver {
            serializer_state: SerializerState {
                extensions: Extensions::default(),
                descriptor_stack: Vec::with_capacity(STACK_CAPACITY),
                container_stack: Vec::with_capacity(STACK_CAPACITY),
            },
            emitter_stack: ManuallyDrop::new(Vec::with_capacity(STACK_CAPACITY)),
            serializable_stack: ManuallyDrop::new({
                let mut vec = Vec::with_capacity(STACK_CAPACITY);
                vec.push(SerializableOnStack::Handle(serializable));
                vec
            }),
            driver_state: DriverState::Serialize,
            next_event: None,
        }
    }

    /// Returns a borrowed reference to the current serializer state.
    pub fn state(&self) -> &SerializerState {
        &self.serializer_state
    }

    /// Produces the next serialization event.
    ///
    /// Calling this method after it has once returned `Ok(None)` is undefined.
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
            .map(|(event, descriptor)| (event, descriptor, &self.serializer_state)))
    }

    fn advance(&mut self) -> Result<(), Error> {
        macro_rules! top_emitter {
            ($ty:ident) => {
                match self.emitter_stack.last_mut() {
                    Some(Emitter::$ty(emitter)) => emitter,
                    _ => panic!("incompatible emitter on stack"),
                }
            };
        }

        loop {
            match self.driver_state {
                DriverState::SeqNextItem => {
                    let emitter = top_emitter!(Seq);
                    match unsafe {
                        extend_lifetime!(
                            emitter.next(&self.serializer_state)?,
                            Option<SerializeHandle>
                        )
                    } {
                        Some(item_serializable) => {
                            // and serialize the current item
                            self.serializable_stack
                                .push(SerializableOnStack::Handle(item_serializable));
                            self.driver_state = DriverState::Serialize;
                        }
                        None => {
                            self.driver_state = DriverState::PopEmitter;
                        }
                    }
                }
                DriverState::MapNextKey => {
                    let emitter = top_emitter!(Map);
                    match unsafe {
                        extend_lifetime!(
                            emitter.next_key(&self.serializer_state)?,
                            Option<SerializeHandle>
                        )
                    } {
                        Some(key_serializable) => {
                            // and serialize the current key
                            self.serializable_stack
                                .push(SerializableOnStack::Handle(key_serializable));
                            self.driver_state = DriverState::Serialize;
                        }
                        None => {
                            self.driver_state = DriverState::PopEmitter;
                        }
                    }
                }
                DriverState::MapNextValue => {
                    let emitter = top_emitter!(Map);
                    let value_serializable = unsafe {
                        extend_lifetime!(
                            emitter.next_value(&self.serializer_state)?,
                            SerializeHandle
                        )
                    };
                    // and serialize the current value
                    self.serializable_stack
                        .push(SerializableOnStack::Handle(value_serializable));
                    self.driver_state = DriverState::Serialize;
                }
                DriverState::StructNextPair => {
                    let emitter = top_emitter!(Struct);
                    match unsafe {
                        extend_lifetime!(
                            emitter.next(&self.serializer_state)?,
                            Option<(Cow<'_, str>, SerializeHandle)>
                        )
                    } {
                        Some((key, value_serializable)) => {
                            // and serialize key and value
                            self.serializable_stack
                                .push(SerializableOnStack::Handle(value_serializable));
                            self.serializable_stack
                                .push(SerializableOnStack::StrCow(key));
                            self.driver_state = DriverState::Serialize;
                        }
                        None => {
                            self.driver_state = DriverState::PopEmitter;
                        }
                    }
                }
                DriverState::PushStructState => {
                    self.serializer_state
                        .container_stack
                        .push(ContainerState::Map {
                            first: true,
                            key_pos: true,
                        });
                    self.driver_state = DriverState::StructNextPair;
                }
                DriverState::PushMapState => {
                    self.serializer_state
                        .container_stack
                        .push(ContainerState::Map {
                            first: true,
                            key_pos: true,
                        });
                    self.driver_state = DriverState::MapNextKey;
                }
                DriverState::PushSeqState => {
                    self.serializer_state
                        .container_stack
                        .push(ContainerState::Seq { first: true });
                    self.driver_state = DriverState::SeqNextItem;
                }
                DriverState::Serialize => {
                    let serializable = self.serializable_stack.last().unwrap();
                    match unsafe {
                        extend_lifetime!(serializable.serialize(&self.serializer_state)?, Chunk)
                    } {
                        Chunk::Atom(atom) => {
                            self.next_event = Some((Event::Atom(atom), unsafe {
                                extend_lifetime!(serializable.descriptor(), &dyn Descriptor)
                            }));
                            self.driver_state = DriverState::FinishSerialize;
                            return Ok(());
                        }
                        Chunk::Struct(emitter) => {
                            let descriptor = unsafe {
                                extend_lifetime!(serializable.descriptor(), &dyn Descriptor)
                            };
                            self.next_event = Some((Event::MapStart, descriptor));
                            self.emitter_stack.push(Emitter::Struct(emitter));
                            self.driver_state = DriverState::PushStructState;
                            self.serializer_state.descriptor_stack.push(descriptor);
                            return Ok(());
                        }
                        Chunk::Map(emitter) => {
                            let descriptor = unsafe {
                                extend_lifetime!(serializable.descriptor(), &dyn Descriptor)
                            };
                            self.next_event = Some((Event::MapStart, descriptor));
                            self.emitter_stack.push(Emitter::Map(emitter));
                            self.driver_state = DriverState::PushMapState;
                            self.serializer_state.descriptor_stack.push(descriptor);
                            return Ok(());
                        }
                        Chunk::Seq(emitter) => {
                            let descriptor = unsafe {
                                extend_lifetime!(serializable.descriptor(), &dyn Descriptor)
                            };
                            self.next_event = Some((Event::SeqStart, descriptor));
                            self.emitter_stack.push(Emitter::Seq(emitter));
                            self.driver_state = DriverState::PushSeqState;
                            self.serializer_state.descriptor_stack.push(descriptor);
                            return Ok(());
                        }
                    }
                }
                DriverState::PopEmitter => {
                    let descriptor = self.serializer_state.descriptor_stack.pop().unwrap();
                    self.next_event = Some((
                        match self.emitter_stack.pop().unwrap() {
                            Emitter::Seq(_) => Event::SeqEnd,
                            Emitter::Map(_) | Emitter::Struct(_) => Event::MapEnd,
                        },
                        descriptor,
                    ));
                    self.serializer_state.container_stack.pop();
                    self.driver_state = DriverState::FinishSerialize;
                    return Ok(());
                }
                DriverState::FinishSerialize => {
                    let serializable = self.serializable_stack.pop().unwrap();
                    serializable.finish(&self.serializer_state)?;
                    match self.serializer_state.container_stack.last_mut() {
                        None => {
                            break;
                        }
                        Some(ContainerState::Seq { first }) => {
                            *first = false;
                            self.driver_state = DriverState::SeqNextItem;
                        }
                        Some(ContainerState::Map { first, key_pos }) => {
                            *first = false;
                            let key = !*key_pos;
                            *key_pos = key;

                            self.driver_state = match self.emitter_stack.last() {
                                Some(Emitter::Struct { .. }) if key => DriverState::StructNextPair,
                                Some(Emitter::Struct { .. }) if !key => DriverState::Serialize,
                                Some(Emitter::Map { .. }) if key => DriverState::MapNextKey,
                                Some(Emitter::Map { .. }) if !key => DriverState::MapNextValue,
                                _ => unreachable!("wrong emitter on stack"),
                            };
                        }
                    }
                }
            }
        }

        Ok(())
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
