use std::borrow::Cow;
use std::marker::PhantomData;
use std::ptr::NonNull;

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
    state: SerializerState,
    // Values and frames refer to data borrowed from the serializables and
    // emitters of the frames below them, which is why the lifetimes are
    // erased.  `next_value` and `needs_finish` borrow from the top frame.
    //
    // A value that was produced by an emitter (or the root value) that still
    // needs to be serialized.
    next_value: Option<Held>,
    // A value that was fully emitted and on which `finish` needs to be called.
    needs_finish: Option<Held>,
    stack: Vec<Frame>,
    _marker: PhantomData<&'a dyn Serialize>,
}

static STRUCT_KEY_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };

/// A compound value that is currently being serialized.
struct Frame {
    // `emitter` must be declared (and thus dropped) before `serializable` as
    // the emitters borrow from the serializable.
    emitter: Emitter,
    serializable: Held,
}

enum Emitter {
    Seq(Box<dyn SeqEmitter>),
    /// A map emitter, the flag is `true` if a value is expected next.
    Map(Box<dyn MapEmitter>, bool),
    Struct(Box<dyn StructEmitter>),
}

/// A serializable held by the driver.
///
/// This is like a [`SerializeHandle`] with an erased lifetime, but owned
/// values are held by raw pointer so that the handle can be moved while
/// events or emitters borrow from the value.
struct Held {
    ptr: NonNull<dyn Serialize>,
    owned: bool,
}

impl Held {
    /// Creates a held value from a handle.
    ///
    /// # Safety
    ///
    /// The held value must be dropped before the data the handle borrows.
    #[inline]
    unsafe fn new(handle: SerializeHandle<'_>) -> Held {
        let (ptr, owned) = match handle {
            SerializeHandle::Borrowed(value) => (NonNull::from(value), false),
            SerializeHandle::Owned(value) => (NonNull::new_unchecked(Box::into_raw(value)), true),
        };
        Held {
            ptr: std::mem::transmute::<NonNull<dyn Serialize + '_>, NonNull<dyn Serialize>>(ptr),
            owned,
        }
    }

    /// Returns the value with an unbounded lifetime.
    ///
    /// # Safety
    ///
    /// The returned reference must not be used after the held value was
    /// dropped.
    #[inline(always)]
    unsafe fn get<'x>(&self) -> &'x dyn Serialize {
        &*self.ptr.as_ptr()
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: owned values were created from a box
            unsafe { drop(Box::from_raw(self.ptr.as_ptr())) };
        }
    }
}

impl<'a> Drop for SerializeDriver<'a> {
    fn drop(&mut self) {
        // the pending values borrow from the top frame and inner frames can
        // borrow from outer frames, drop in inverse order.
        self.needs_finish = None;
        self.next_value = None;
        while let Some(_frame) = self.stack.pop() {}
    }
}

const STACK_CAPACITY: usize = 128;

type NextEvent<'a> = Option<(Event<'a>, &'static dyn Descriptor)>;

impl<'a> SerializeDriver<'a> {
    /// Creates a new driver which serializes the given value implementing [`Serialize`].
    pub fn new(serializable: &'a dyn Serialize) -> SerializeDriver<'a> {
        SerializeDriver {
            state: SerializerState {
                extensions: Extensions::default(),
                descriptor_stack: Vec::with_capacity(STACK_CAPACITY),
            },
            // SAFETY: the driver cannot outlive 'a
            next_value: Some(unsafe { Held::new(SerializeHandle::Borrowed(serializable)) }),
            needs_finish: None,
            stack: Vec::with_capacity(STACK_CAPACITY),
            _marker: PhantomData,
        }
    }

    /// Returns a borrowed reference to the current serializer state.
    pub fn state(&self) -> &SerializerState {
        &self.state
    }

    /// Returns a mutable reference to the current serializer state.
    ///
    /// This can be used to place extension values into the state which the
    /// serializable values can then pick up.
    pub fn state_mut(&mut self) -> &mut SerializerState {
        &mut self.state
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
    ) -> Result<Option<(Event<'_>, &'static dyn Descriptor, &SerializerState)>, Error> {
        Ok(self
            .advance()?
            .map(|(event, descriptor)| (event, descriptor, &self.state)))
    }

    /// Advances the driver.
    ///
    /// The returned event is bound to `'static` but it actually borrows from
    /// the values and frames held by the driver.  It's only valid until the
    /// next call.
    fn advance(&mut self) -> Result<NextEvent<'static>, Error> {
        if let Some(held) = self.needs_finish.take() {
            // SAFETY: the value is alive until the end of this block
            unsafe { held.get() }.finish(&mut self.state)?;
        }

        let value = match self.next_value.take() {
            Some(value) => value,
            None => {
                let frame = match self.stack.last_mut() {
                    Some(frame) => frame,
                    None => return Ok(None),
                };
                // SAFETY: values produced by the emitter borrow from it.  The
                // frame stays on the stack until all of them are dropped.
                let emitter = unsafe { &mut *(&mut frame.emitter as *mut Emitter) };
                let next = match emitter {
                    Emitter::Seq(emitter) => emitter.next(&mut self.state)?,
                    Emitter::Map(emitter, is_value) => {
                        if *is_value {
                            *is_value = false;
                            Some(emitter.next_value(&mut self.state)?)
                        } else {
                            let key = emitter.next_key(&mut self.state)?;
                            *is_value = key.is_some();
                            key
                        }
                    }
                    Emitter::Struct(emitter) => match emitter.next(&mut self.state)? {
                        Some((key, value)) => {
                            // the key is emitted directly as event, the value
                            // is serialized on the next call.
                            // SAFETY: the value and key borrow from the emitter
                            // which stays alive until the next call.
                            self.next_value = Some(unsafe { Held::new(value) });
                            let key = unsafe {
                                std::mem::transmute::<Cow<'_, str>, Cow<'static, str>>(key)
                            };
                            return Ok(Some((Event::Atom(Atom::Str(key)), &STRUCT_KEY_DESCRIPTOR)));
                        }
                        None => None,
                    },
                };
                match next {
                    // SAFETY: the value borrows from the emitter on the top
                    // of the stack.
                    Some(value) => unsafe { Held::new(value) },
                    None => return Ok(Some(self.end_container())),
                }
            }
        };

        self.serialize_value(value)
    }

    /// Ends the container on the top of the stack.
    fn end_container(&mut self) -> (Event<'static>, &'static dyn Descriptor) {
        let Frame {
            emitter,
            serializable,
        } = self.stack.pop().unwrap();
        let event = match emitter {
            Emitter::Seq(_) => Event::SeqEnd,
            _ => Event::MapEnd,
        };
        // the emitter borrows from the serializable, drop it first.
        drop(emitter);
        self.needs_finish = Some(serializable);
        (event, self.state.descriptor_stack.pop().unwrap())
    }

    /// Serializes a value and returns its first event.
    #[inline]
    fn serialize_value(&mut self, value: Held) -> Result<NextEvent<'static>, Error> {
        // SAFETY: the value is held by the driver until the event and the
        // emitters derived from it are dropped.  Moving `value` only moves a
        // pointer to it.
        let serializable = unsafe { value.get() };
        let descriptor = serializable.descriptor();
        let chunk = serializable.serialize(&mut self.state)?;
        let (emitter, event) = match chunk {
            Chunk::Atom(atom) => {
                self.needs_finish = Some(value);
                return Ok(Some((Event::Atom(atom), descriptor)));
            }
            Chunk::Struct(emitter) => (Emitter::Struct(emitter), Event::MapStart),
            Chunk::Map(emitter) => (Emitter::Map(emitter, false), Event::MapStart),
            Chunk::Seq(emitter) => (Emitter::Seq(emitter), Event::SeqStart),
        };
        self.stack.push(Frame {
            emitter,
            serializable: value,
        });
        self.state.descriptor_stack.push(descriptor);
        Ok(Some((event, descriptor)))
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

#[test]
fn test_state_mut() {
    #[derive(Debug, Default)]
    struct Uppercase(bool);

    struct Name(&'static str);

    impl Serialize for Name {
        fn serialize(&self, state: &mut SerializerState) -> Result<Chunk<'_>, Error> {
            Ok(Chunk::Atom(Atom::Str(
                if state.get::<Uppercase>().is_some_and(|x| x.0) {
                    self.0.to_uppercase().into()
                } else {
                    self.0.into()
                },
            )))
        }
    }

    let names = vec![Name("foo"), Name("bar")];
    let mut driver = SerializeDriver::new(&names);
    driver.state_mut().get_mut::<Uppercase>().0 = true;
    let mut events = Vec::new();
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }

    assert_eq!(
        events,
        vec![Event::SeqStart, "FOO".into(), "BAR".into(), Event::SeqEnd],
    );
}
