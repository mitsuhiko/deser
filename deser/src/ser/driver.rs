use std::borrow::Cow;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::error::Error;
use crate::ser::layer::{EventFn, Layer, Next};
use crate::ser::{
    Begin, BeginKind, Chunk, ContainerShape, IndexedSeq, IndexedStruct, PlainSink, StructField,
};
use crate::{Atom, Event, Serialize, State};

use super::{MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};

/// The driver allows serializing a [`Serialize`] iteratively.
///
/// This is the only way to convert from a [`Serialize`] into an event
/// stream.  As a user one has to call [`next`](Self::next) until `None`
/// is returned, indicating the end of the event stream, or use
/// [`drive`](Self::drive).
///
/// When the serialization fails, the error gets the context of the
/// current value attached (see [`State::add_error_context`]).
///
/// # Layers
///
/// [`Layer`]s sit between the serialized values and the format and see
/// every event before the format receives it.  They are added with
/// [`push_layer`](Self::push_layer) and are only supported by
/// [`drive`](Self::drive).
pub struct SerializeDriver<'a> {
    state: State,
    layers: Vec<Box<dyn Layer>>,
    // Values and frames refer to data borrowed from the serializables and
    // emitters of the frames below them, which is why the lifetimes are
    // erased.  `next_value` and `needs_finish` borrow from the top frame.
    //
    // A value that was produced by an emitter (or the root value) that still
    // needs to be serialized.
    next_value: Option<Held>,
    // A value that was fully emitted.  It's held until the next call as the
    // last event can borrow from it.  If the flag is set, `finish` needs to
    // be called on it.
    needs_finish: Option<(Held, bool)>,
    stack: Vec<Frame>,
    // `true` if `next` returned an event.  Its event data is detached on the
    // next call.
    delivered: bool,
    _marker: PhantomData<&'a dyn Serialize>,
}

/// A compound value that is currently being serialized.
struct Frame {
    // `emitter` must be declared (and thus dropped) before `serializable` as
    // the emitters borrow from the serializable.
    emitter: Emitter,
    serializable: Held,
    needs_finish: bool,
}

enum Emitter {
    Seq(Box<dyn SeqEmitter>),
    /// A map emitter, the flag is `true` if a value is expected next.
    Map(Box<dyn MapEmitter>, bool),
    Struct(Box<dyn StructEmitter>),
    /// A sequence with the index of the next element.
    IndexedSeq(&'static dyn IndexedSeq, usize),
    /// A struct with the index of the next field.
    IndexedStruct(&'static dyn IndexedStruct, usize),
    /// A value that forwarded to another value (see [`Chunk::Forward`]).
    ///
    /// The frame holds the value while the forwarded value (which can
    /// borrow from it) is serialized.  It does not emit events and it's
    /// removed once it's on the top of the stack again.
    Forward,
}

/// A serializable held by the driver.
///
/// This is like a [`SerializeHandle`] with an erased lifetime, but owned
/// values are held by raw pointer so that the handle can be moved while
/// events or emitters borrow from the value.
pub(crate) struct Held {
    ptr: NonNull<dyn Serialize>,
    owned: bool,
}

// SAFETY: a held value is either a borrowed `&dyn Serialize` (which is
// `Send` as serializables are `Sync`) or an owned
// `Box<dyn Serialize + Send>`.
unsafe impl Send for Held {}

impl Held {
    /// Creates a held value from a handle.
    ///
    /// # Safety
    ///
    /// The held value must be dropped before the data the handle borrows.
    #[inline]
    pub(crate) unsafe fn new(handle: SerializeHandle<'_>) -> Held {
        unsafe {
            let (ptr, owned) = match handle {
                SerializeHandle::Borrowed(value) => (NonNull::from(value), false),
                SerializeHandle::Owned(value) => {
                    let value: Box<dyn Serialize + '_> = value;
                    (NonNull::new_unchecked(Box::into_raw(value)), true)
                }
            };
            Held {
                ptr: std::mem::transmute::<NonNull<dyn Serialize + '_>, NonNull<dyn Serialize>>(
                    ptr,
                ),
                owned,
            }
        }
    }

    /// Returns the value with an unbounded lifetime.
    ///
    /// # Safety
    ///
    /// The returned reference must not be used after the held value was
    /// dropped.
    #[inline(always)]
    pub(crate) unsafe fn get<'x>(&self) -> &'x dyn Serialize {
        unsafe { &*self.ptr.as_ptr() }
    }
}

impl Drop for Held {
    #[inline(always)]
    fn drop(&mut self) {
        #[cold]
        #[inline(never)]
        unsafe fn drop_owned(ptr: NonNull<dyn Serialize>) {
            unsafe {
                drop(Box::from_raw(ptr.as_ptr()));
            }
        }

        if self.owned {
            // SAFETY: owned values were created from a box
            unsafe { drop_owned(self.ptr) };
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

// an ongoing serialization can move between threads, for instance when it
// is suspended while waiting for IO.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<SerializeDriver<'static>>();
};

type NextEvent<'a> = Option<(Event<'a>, &'a dyn Serialize)>;

/// The callback of [`SerializeDriver::drive`] and
/// [`SerializeDriver::drive_described`].
trait Callback {
    /// `true` if the callback receives the values of the events.
    const DESCRIBED: bool;

    fn call(
        &mut self,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &mut State,
    ) -> Result<(), Error>;
}

/// A callback that does not receive values.
struct Plain<F>(F);

impl<F: FnMut(Event<'_>, &mut State) -> Result<(), Error>> Callback for Plain<F> {
    const DESCRIBED: bool = false;

    #[inline(always)]
    fn call(
        &mut self,
        event: Event<'_>,
        _value: &dyn Serialize,
        state: &mut State,
    ) -> Result<(), Error> {
        (self.0)(event, state)
    }
}

/// A callback that receives values.
struct Described<F>(F);

impl<F: FnMut(Event<'_>, &dyn Serialize, &mut State) -> Result<(), Error>> Callback
    for Described<F>
{
    const DESCRIBED: bool = true;

    #[inline(always)]
    fn call(
        &mut self,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &mut State,
    ) -> Result<(), Error> {
        (self.0)(event, value, state)
    }
}

/// The value of the keys of structs, it describes nothing.
static FIELD_KEY: () = ();

/// Delivers the events of plain values (see [`PlainSink`]).
struct PlainDelivery<'d, 'a, C> {
    driver: &'d mut SerializeDriver<'a>,
    f: &'d mut C,
}

impl<C: Callback> PlainSink for PlainDelivery<'_, '_, C> {
    #[inline]
    fn atom(&mut self, atom: Atom<'_>) -> Result<(), Error> {
        self.driver.state.is_map_key = false;
        self.driver.deliver(self.f, Event::Atom(atom), &FIELD_KEY)
    }

    #[inline]
    fn seq_start(&mut self, shape: ContainerShape) -> Result<(), Error> {
        self.driver.state.is_map_key = false;
        self.driver.state.depth += 1;
        self.driver
            .deliver(self.f, Event::SeqStart(shape), &FIELD_KEY)
    }

    #[inline]
    fn seq_end(&mut self) -> Result<(), Error> {
        self.driver.state.depth -= 1;
        self.driver.deliver(self.f, Event::SeqEnd, &FIELD_KEY)
    }
}

impl<'a> SerializeDriver<'a> {
    /// Creates a new driver which serializes the given value implementing [`Serialize`].
    pub fn new(serializable: &'a dyn Serialize) -> SerializeDriver<'a> {
        SerializeDriver {
            state: State::new(),
            layers: Vec::new(),
            // SAFETY: the driver cannot outlive 'a
            next_value: Some(unsafe { Held::new(SerializeHandle::Borrowed(serializable)) }),
            needs_finish: None,
            stack: Vec::with_capacity(STACK_CAPACITY),
            delivered: false,
            _marker: PhantomData,
        }
    }

    /// Returns a borrowed reference to the current serializer state.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Returns a mutable reference to the current serializer state.
    ///
    /// This can be used to place extension values into the state which the
    /// serializable values can then pick up.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// Adds a layer.
    ///
    /// Layers see the events in the order they were added: the layer that
    /// was added first sees the events produced by the values, the last one
    /// passes them on to the format.  See [`Layer`] for more information.
    pub fn push_layer<L: Layer + 'static>(&mut self, layer: L) {
        self.layers.push(Box::new(layer));
    }

    /// Produces the next serialization event.
    ///
    /// # Panics
    ///
    /// The driver will panic if the data fed from the serializer is
    /// malformed.  As layers can change the number of events, this method
    /// panics if layers were added.
    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn next(&mut self) -> Result<Option<(Event<'_>, &dyn Serialize, &mut State)>, Error> {
        assert!(
            self.layers.is_empty(),
            "layers are only supported by SerializeDriver::drive"
        );
        let rv = match self.advance() {
            Ok(rv) => rv,
            Err(err) => return Err(self.state.attach_error_context(err)),
        };
        self.delivered = rv.is_some();
        // The event and the value borrow from the values held by the driver
        // but never from the state (serializables cannot return chunks
        // borrowing from it), which is why the state can be handed out
        // mutably.
        Ok(rv.map(|(event, value)| (event, value, &mut self.state)))
    }

    /// Detaches the event data of an event returned by `next`.
    #[inline(always)]
    fn detach_delivered_event_data(&mut self) {
        if self.delivered {
            self.delivered = false;
            self.state.clear_event_data();
        }
    }

    /// Drives the serialization to the end and invokes a callback for every
    /// event.
    ///
    /// This produces the same events as calling [`next`](Self::next) until
    /// it returns `None` but it's faster.  The first error (either produced
    /// by a serializable or returned by the callback) aborts the
    /// serialization.
    ///
    /// ```
    /// # use deser::ser::SerializeDriver;
    /// # fn do_it() -> Result<(), deser::Error> {
    /// let serializable = vec!["foo", "bar", "baz"];
    /// let mut events = Vec::new();
    /// SerializeDriver::new(&serializable).drive(|event, _state| {
    ///     events.push(event.to_static());
    ///     Ok(())
    /// })?;
    /// assert_eq!(events.len(), 5);
    /// # Ok(()) } do_it().unwrap();
    /// ```
    #[inline]
    pub fn drive<F>(&mut self, f: F) -> Result<(), Error>
    where
        F: FnMut(Event<'_>, &mut State) -> Result<(), Error>,
    {
        match self.drive_impl(Plain(f)) {
            Ok(()) => Ok(()),
            Err(err) => Err(self.state.attach_error_context(err)),
        }
    }

    /// Like [`drive`](Self::drive) but the callback also receives the value
    /// of every event.
    ///
    /// The value is intended to be [described](crate::ser::Describe) by
    /// formats that reflect the Rust shape of values.  Formats that do not
    /// need it should use [`drive`](Self::drive) which is faster.
    ///
    /// ```
    /// # use deser::ser::{Describe, SerializeDriver};
    /// # fn do_it() -> Result<(), deser::Error> {
    /// struct IsSome(bool);
    ///
    /// impl Describe for IsSome {
    ///     fn some(&mut self) {
    ///         self.0 = true;
    ///     }
    /// }
    ///
    /// let mut some = Vec::new();
    /// SerializeDriver::new(&vec![Some(1), None]).drive_described(|_event, value, _state| {
    ///     let mut describer = IsSome(false);
    ///     value.describe(&mut describer);
    ///     some.push(describer.0);
    ///     Ok(())
    /// })?;
    /// assert_eq!(some, [false, true, false, false]);
    /// # Ok(()) } do_it().unwrap();
    /// ```
    #[inline]
    pub fn drive_described<F>(&mut self, f: F) -> Result<(), Error>
    where
        F: FnMut(Event<'_>, &dyn Serialize, &mut State) -> Result<(), Error>,
    {
        match self.drive_impl(Described(f)) {
            Ok(()) => Ok(()),
            Err(err) => Err(self.state.attach_error_context(err)),
        }
    }

    /// Delivers an event to the layers and the callback of
    /// [`drive`](Self::drive).
    #[inline(always)]
    fn deliver<C: Callback>(
        &mut self,
        f: &mut C,
        event: Event<'_>,
        value: &dyn Serialize,
    ) -> Result<(), Error> {
        // the value is only passed on if the callback wants it
        let value = if C::DESCRIBED { value } else { &FIELD_KEY };
        if self.layers.is_empty() {
            f.call(event, value, &mut self.state)?;
        } else {
            self.deliver_layered(
                &mut |event, value, state| f.call(event, value, state),
                event,
                value,
            )?;
        }
        self.state.clear_event_data();
        Ok(())
    }

    /// Passes an event through the layers.
    #[inline(never)]
    fn deliver_layered(
        &mut self,
        f: &mut EventFn<'_>,
        event: Event<'_>,
        value: &dyn Serialize,
    ) -> Result<(), Error> {
        Next::new(&mut self.layers, &mut self.state, f, value).emit(event)
    }

    #[inline(always)]
    fn drive_impl<C: Callback>(&mut self, mut f: C) -> Result<(), Error> {
        // `next` might have been used before.
        self.detach_delivered_event_data();
        if let Some((held, true)) = self.needs_finish.take() {
            // SAFETY: the value is alive until the end of this block
            unsafe { held.get() }.finish(&mut self.state)?;
        }
        if let Some(value) = self.next_value.take() {
            self.drive_value(value, false, &mut f)?;
        }

        while let Some(frame) = self.stack.last_mut() {
            // SAFETY: values produced by the emitter borrow from it.  The
            // frame stays on the stack until all of them are dropped.
            let emitter = unsafe { &mut *(&mut frame.emitter as *mut Emitter) };
            let value = match emitter {
                Emitter::Forward => {
                    self.finish_forward()?;
                    continue;
                }
                Emitter::IndexedStruct(fields, index) => {
                    let field = fields.field(*index, &mut self.state)?;
                    *index += 1;
                    match field {
                        StructField::Field(key, value) => {
                            self.state.is_map_key = true;
                            self.deliver(
                                &mut f,
                                Event::Atom(Atom::Str(Cow::Borrowed(key))),
                                &FIELD_KEY,
                            )?;
                            (value, false)
                        }
                        StructField::Skip => continue,
                        StructField::End => {
                            self.drive_end(&mut f)?;
                            continue;
                        }
                    }
                }
                Emitter::IndexedSeq(seq, index) => {
                    let element = seq.element(*index, &mut self.state)?;
                    *index += 1;
                    match element {
                        Some(value) => (value, false),
                        None => {
                            self.drive_end(&mut f)?;
                            continue;
                        }
                    }
                }
                Emitter::Struct(emitter) => match emitter.next(&mut self.state)? {
                    Some((key, value)) => {
                        self.state.is_map_key = true;
                        self.deliver(&mut f, Event::Atom(Atom::Str(key)), &FIELD_KEY)?;
                        (value, false)
                    }
                    None => {
                        self.drive_end(&mut f)?;
                        continue;
                    }
                },
                Emitter::Seq(emitter) => match emitter.next(&mut self.state)? {
                    Some(value) => (value, false),
                    None => {
                        self.drive_end(&mut f)?;
                        continue;
                    }
                },
                Emitter::Map(emitter, is_value) => {
                    if *is_value {
                        *is_value = false;
                        (emitter.next_value(&mut self.state)?, false)
                    } else {
                        match emitter.next_key(&mut self.state)? {
                            Some(key) => {
                                *is_value = true;
                                (key, true)
                            }
                            None => {
                                self.drive_end(&mut f)?;
                                continue;
                            }
                        }
                    }
                }
            };
            // SAFETY: the value borrows from the emitter on the top of the
            // stack.
            self.drive_value(unsafe { Held::new(value.0) }, value.1, &mut f)?;
        }

        Ok(())
    }

    /// Removes a forwarding frame from the top of the stack.
    ///
    /// This is invoked once the forwarded value was serialized.
    #[cold]
    fn finish_forward(&mut self) -> Result<(), Error> {
        let Frame {
            emitter,
            serializable,
            needs_finish,
        } = self.stack.pop().unwrap();
        debug_assert!(matches!(emitter, Emitter::Forward));
        if needs_finish {
            // SAFETY: the value is alive until the end of this block
            unsafe { serializable.get() }.finish(&mut self.state)?;
        }
        Ok(())
    }

    /// Places a value that forwarded to another value on the stack.
    ///
    /// Returns the forwarded value.
    #[cold]
    fn push_forward(
        &mut self,
        value: Held,
        needs_finish: bool,
        forwarded: SerializeHandle<'_>,
    ) -> Held {
        self.stack.push(Frame {
            emitter: Emitter::Forward,
            serializable: value,
            needs_finish,
        });
        // SAFETY: the forwarded value can borrow from the value which is
        // held by the frame.  It's dropped before the frame.
        unsafe { Held::new(forwarded) }
    }

    /// Serializes a forwarded value and emits its first event.
    #[inline(never)]
    fn drive_forwarded<C: Callback>(
        &mut self,
        value: Held,
        is_key: bool,
        f: &mut C,
    ) -> Result<(), Error> {
        self.drive_value(value, is_key, f)
    }

    /// Serializes a value and emits its first event.
    #[inline(always)]
    fn drive_value<C: Callback>(
        &mut self,
        value: Held,
        is_key: bool,
        f: &mut C,
    ) -> Result<(), Error> {
        // SAFETY: the value is held until the event and the emitters derived
        // from it are dropped.
        let serializable = unsafe { value.get() };
        self.state.is_map_key = is_key;
        let Begin {
            kind,
            shape,
            needs_finish,
        } = serializable.__private_begin(&mut self.state)?;
        let (emitter, event) = match kind {
            BeginKind::Chunk(Chunk::Atom(atom)) => {
                self.deliver(f, Event::Atom(atom), serializable)?;
                if needs_finish {
                    serializable.finish(&mut self.state)?;
                }
                return Ok(());
            }
            BeginKind::Chunk(Chunk::Struct(emitter)) => {
                (Emitter::Struct(emitter), Event::MapStart(shape))
            }
            BeginKind::Chunk(Chunk::Map(emitter)) => {
                (Emitter::Map(emitter, false), Event::MapStart(shape))
            }
            BeginKind::Chunk(Chunk::Seq(emitter)) => {
                (Emitter::Seq(emitter), Event::SeqStart(shape))
            }
            BeginKind::Struct(fields) => {
                (Emitter::IndexedStruct(fields, 0), Event::MapStart(shape))
            }
            // callbacks that describe values need to see every value
            BeginKind::Seq(seq) if !C::DESCRIBED => {
                return self.drive_indexed_seq(value, seq, shape, f);
            }
            BeginKind::Seq(seq) => (Emitter::IndexedSeq(seq, 0), Event::SeqStart(shape)),
            BeginKind::Chunk(Chunk::Forward(forwarded)) => {
                let forwarded = self.push_forward(value, needs_finish, forwarded);
                return self.drive_forwarded(forwarded, is_key, f);
            }
        };
        self.stack.push(Frame {
            emitter,
            serializable: value,
            needs_finish,
        });
        self.state.depth += 1;
        self.deliver(f, event, serializable)
    }

    /// Starts an indexed sequence.
    ///
    /// If its elements are plain, they are emitted right away and the
    /// sequence is ended.  Otherwise it's placed on the stack.
    #[inline(always)]
    fn drive_indexed_seq<C: Callback>(
        &mut self,
        value: Held,
        seq: &'static dyn IndexedSeq,
        shape: ContainerShape,
        f: &mut C,
    ) -> Result<(), Error> {
        // SAFETY: the value is held until the end of this function or by
        // the frame.
        let serializable = unsafe { value.get() };
        self.state.depth += 1;
        self.deliver(f, Event::SeqStart(shape), serializable)?;
        let emitted = seq.emit_plain(&mut PlainDelivery { driver: self, f })?;
        if emitted {
            self.state.depth -= 1;
            self.deliver(f, Event::SeqEnd, serializable)
        } else {
            self.stack.push(Frame {
                emitter: Emitter::IndexedSeq(seq, 0),
                serializable: value,
                // indexed sequences do not need `finish`
                needs_finish: false,
            });
            Ok(())
        }
    }

    /// Ends the container on the top of the stack and emits the end event.
    #[inline]
    fn drive_end<C: Callback>(&mut self, f: &mut C) -> Result<(), Error> {
        let event = self.end_container();
        // SAFETY: the value that started the container is held until the
        // next event.  This is only read if the callback uses it.
        let value = unsafe { self.finished_value() };
        self.deliver(f, event, value)?;
        if let Some((held, true)) = self.needs_finish.take() {
            // SAFETY: the value is alive until the end of this block
            unsafe { held.get() }.finish(&mut self.state)?;
        }
        Ok(())
    }

    /// Advances the driver.
    ///
    /// The returned event is bound to `'static` but it actually borrows from
    /// the values and frames held by the driver.  It's only valid until the
    /// next call.
    fn advance(&mut self) -> Result<NextEvent<'static>, Error> {
        self.detach_delivered_event_data();
        if let Some((held, true)) = self.needs_finish.take() {
            // SAFETY: the value is alive until the end of this block
            unsafe { held.get() }.finish(&mut self.state)?;
        }

        let mut is_key = false;
        let value = match self.next_value.take() {
            Some(value) => value,
            None => loop {
                let frame = match self.stack.last_mut() {
                    Some(frame) => frame,
                    None => return Ok(None),
                };
                // SAFETY: values produced by the emitter borrow from it.  The
                // frame stays on the stack until all of them are dropped.
                let emitter = unsafe { &mut *(&mut frame.emitter as *mut Emitter) };
                let next = match emitter {
                    Emitter::Forward => {
                        self.finish_forward()?;
                        continue;
                    }
                    Emitter::Seq(emitter) => emitter.next(&mut self.state)?,
                    Emitter::Map(emitter, is_value) => {
                        if *is_value {
                            *is_value = false;
                            Some(emitter.next_value(&mut self.state)?)
                        } else {
                            let key = emitter.next_key(&mut self.state)?;
                            *is_value = key.is_some();
                            is_key = true;
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
                            self.state.is_map_key = true;
                            return Ok(Some((Event::Atom(Atom::Str(key)), &FIELD_KEY)));
                        }
                        None => None,
                    },
                    Emitter::IndexedSeq(seq, index) => {
                        let rv = seq.element(*index, &mut self.state)?;
                        *index += 1;
                        rv
                    }
                    Emitter::IndexedStruct(fields, index) => loop {
                        let field = fields.field(*index, &mut self.state)?;
                        *index += 1;
                        match field {
                            StructField::Field(key, value) => {
                                // SAFETY: the value and key borrow from the
                                // serializable of the frame.
                                self.next_value = Some(unsafe { Held::new(value) });
                                let key = Cow::Borrowed(key);
                                self.state.is_map_key = true;
                                return Ok(Some((Event::Atom(Atom::Str(key)), &FIELD_KEY)));
                            }
                            StructField::Skip => continue,
                            StructField::End => break None,
                        }
                    },
                };
                match next {
                    // SAFETY: the value borrows from the emitter on the top
                    // of the stack.
                    Some(value) => break unsafe { Held::new(value) },
                    None => {
                        let event = self.end_container();
                        // SAFETY: the value is held until the next call
                        return Ok(Some((event, unsafe { self.finished_value() })));
                    }
                }
            },
        };

        self.serialize_value(value, is_key)
    }

    /// Ends the container on the top of the stack.
    fn end_container(&mut self) -> Event<'static> {
        let Frame {
            emitter,
            serializable,
            needs_finish,
        } = self.stack.pop().unwrap();
        let event = match emitter {
            Emitter::Seq(_) | Emitter::IndexedSeq(..) => Event::SeqEnd,
            _ => Event::MapEnd,
        };
        // the emitter borrows from the serializable, drop it first.
        drop(emitter);
        self.needs_finish = Some((serializable, needs_finish));
        self.state.depth -= 1;
        event
    }

    /// Returns the value of the container that was ended last.
    ///
    /// # Safety
    ///
    /// The value must not be used after the next event.
    #[inline(always)]
    unsafe fn finished_value(&self) -> &'static dyn Serialize {
        match self.needs_finish {
            // SAFETY: the caller guarantees the value is not used for longer
            Some((ref held, _)) => unsafe { held.get() },
            None => &FIELD_KEY,
        }
    }

    /// Serializes a value and returns its first event.
    #[inline]
    fn serialize_value(&mut self, value: Held, is_key: bool) -> Result<NextEvent<'static>, Error> {
        // SAFETY: the value is held by the driver until the event and the
        // emitters derived from it are dropped.  Moving `value` only moves a
        // pointer to it.
        let serializable = unsafe { value.get() };
        self.state.is_map_key = is_key;
        let Begin {
            kind,
            shape,
            needs_finish,
        } = serializable.__private_begin(&mut self.state)?;
        let (emitter, event) = match kind {
            BeginKind::Chunk(Chunk::Atom(atom)) => {
                self.needs_finish = Some((value, needs_finish));
                return Ok(Some((Event::Atom(atom), serializable)));
            }
            BeginKind::Chunk(Chunk::Struct(emitter)) => {
                (Emitter::Struct(emitter), Event::MapStart(shape))
            }
            BeginKind::Chunk(Chunk::Map(emitter)) => {
                (Emitter::Map(emitter, false), Event::MapStart(shape))
            }
            BeginKind::Chunk(Chunk::Seq(emitter)) => {
                (Emitter::Seq(emitter), Event::SeqStart(shape))
            }
            BeginKind::Struct(fields) => {
                (Emitter::IndexedStruct(fields, 0), Event::MapStart(shape))
            }
            BeginKind::Seq(seq) => (Emitter::IndexedSeq(seq, 0), Event::SeqStart(shape)),
            BeginKind::Chunk(Chunk::Forward(forwarded)) => {
                let forwarded = self.push_forward(value, needs_finish, forwarded);
                return self.serialize_forwarded(forwarded, is_key);
            }
        };
        self.stack.push(Frame {
            emitter,
            serializable: value,
            needs_finish,
        });
        self.state.depth += 1;
        Ok(Some((event, serializable)))
    }

    /// Serializes a forwarded value and returns its first event.
    #[inline(never)]
    fn serialize_forwarded(
        &mut self,
        value: Held,
        is_key: bool,
    ) -> Result<NextEvent<'static>, Error> {
        self.serialize_value(value, is_key)
    }
}

#[test]
fn test_seq_emitting() {
    let vec = vec![vec![1u64, 2], vec![3, 4]];

    let mut driver = SerializeDriver::new(&vec);
    let mut events = Vec::new();
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(crate::event::without_len(event.to_static()));
    }

    assert_eq!(
        events,
        vec![
            Event::seq_start(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            Event::seq_start(),
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
        events.push(crate::event::without_len(event.to_static()));
    }

    assert_eq!(
        events,
        vec![
            Event::MapStart(crate::ContainerShape::new().with_order(crate::Order::Sorted)),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd,
            "first".into(),
            Event::seq_start(),
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
        fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
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
        events.push(crate::event::without_len(event.to_static()));
    }

    assert_eq!(
        events,
        vec![
            Event::seq_start(),
            "FOO".into(),
            "BAR".into(),
            Event::SeqEnd
        ],
    );
}
