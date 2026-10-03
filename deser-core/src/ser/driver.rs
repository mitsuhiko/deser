use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::arena::{Alloc, Buffer};
use crate::error::Error;
use crate::ser::layer::{EventFn, Layer, Next};
use crate::ser::{
    Begin, BeginKind, Boxed, ContainerShape, Emit, Erased, FIELDS_END, HandleInner, IndexedSeq,
    IndexedStruct, PLAIN_BUDGET, PlainSink, Serialize, SerializeRef, StructField,
};
use crate::{Atom, Event, State};
use crate::{Context, Text};

use super::{MapEmitter, SeqEmitter, SerializeHandle, StructEmitter};

/// The driver allows serializing a [`Serialize`] iteratively.
///
/// This is the only way to convert from a [`Serialize`] into an event
/// stream.  There are several ways to receive the events:
///
/// * [`drive`](Self::drive) invokes a callback for every event and
///   [`drive_sink`](Self::drive_sink) delivers them to an [`EventSink`].
///   Both serialize the value at once.
/// * [`drive_until`](Self::drive_until) delivers the events to an
///   [`EventSink`] which can pause the driver, for instance to write the
///   output of large values in pieces.
/// * [`next`](Self::next) returns one event at a time.
///
/// Event sinks can also receive the values of the events to
/// [describe](crate::ser::Describe) them (see [`EventSink::DESCRIBED`]).
///
/// When the serialization fails, the error gets the context of the
/// current value attached (see [`State::add_error_context`]).
///
/// # Layers
///
/// [`Layer`]s sit between the serialized values and the format and see
/// every event before the format receives it.  They are added with
/// [`push_layer`](Self::push_layer) and are not supported by
/// [`next`](Self::next).
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
    _marker: PhantomData<SerializeRef<'a>>,
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
    Seq(Boxed<dyn SeqEmitter>),
    /// A map emitter, the flag is `true` if a value is expected next.
    Map(Boxed<dyn MapEmitter>, bool),
    Struct(Boxed<dyn StructEmitter>),
    /// A sequence with the index of the next element.
    IndexedSeq(&'static dyn IndexedSeq, usize),
    /// A struct with the index of the next field.
    IndexedStruct(&'static dyn IndexedStruct, usize),
    /// A value that forwarded to another value (see [`Emit::Forward`]).
    ///
    /// The frame holds the value while the forwarded value (which can
    /// borrow from it) is serialized.  It does not emit events and it's
    /// removed once it's on the top of the stack again.
    Forward,
}

impl Emitter {
    /// Drops the emitter, it's popped from the arena right away if it's on
    /// the top.
    #[inline(always)]
    fn release(self, state: &mut State) {
        match self {
            Emitter::Seq(emitter) => Boxed::release(emitter, state),
            Emitter::Map(emitter, _) => Boxed::release(emitter, state),
            Emitter::Struct(emitter) => Boxed::release(emitter, state),
            _ => {}
        }
    }
}

/// A serializable held by the driver.
///
/// This is like a [`SerializeHandle`] with an erased lifetime, but owned
/// values are held by raw pointer so that the handle can be moved while
/// events or emitters borrow from the value.
pub(crate) struct Held {
    ptr: NonNull<dyn Erased>,
    /// Where the value is allocated if it's owned, `None` if it's borrowed.
    owned: Option<Alloc>,
}

// SAFETY: a held value is either a borrowed `SerializeRef` (which is
// `Send` as serializables are `Sync`) or an owned
// `Boxed<dyn Erased + Send>`.
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
            let (ptr, owned) = match handle.0 {
                HandleInner::Borrowed(value) => (NonNull::from(value.as_dyn()), None),
                HandleInner::Owned(value) => {
                    let (ptr, alloc) = Boxed::into_raw(value);
                    let ptr: NonNull<dyn Erased + '_> = ptr;
                    (ptr, Some(alloc))
                }
            };
            Held {
                ptr: core::mem::transmute::<NonNull<dyn Erased + '_>, NonNull<dyn Erased>>(ptr),
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
    pub(crate) unsafe fn get<'x>(&self) -> SerializeRef<'x> {
        SerializeRef::from_dyn(unsafe { &*self.ptr.as_ptr() })
    }
}

impl Drop for Held {
    #[inline(always)]
    fn drop(&mut self) {
        #[cold]
        #[inline(never)]
        unsafe fn drop_owned(ptr: NonNull<dyn Erased>, alloc: Alloc) {
            unsafe {
                drop(Boxed::from_raw(ptr, alloc));
            }
        }

        if let Some(alloc) = self.owned {
            // SAFETY: owned values were created from a box of the allocation
            unsafe { drop_owned(self.ptr, alloc) };
        }
    }
}

impl<'a> Drop for SerializeDriver<'a> {
    fn drop(&mut self) {
        // the pending values borrow from the top frame and inner frames can
        // borrow from outer frames, drop in inverse order.
        self.needs_finish = None;
        self.next_value = None;
        while let Some(frame) = self.stack.pop() {
            // the emitter borrows from the serializable, drop it first
            frame.emitter.release(&mut self.state);
        }
        let stack = core::mem::take(&mut self.stack);
        self.state.arena.put_vec(Buffer::SerializeStack, stack);
    }
}

const STACK_CAPACITY: usize = 128;

// an ongoing serialization can move between threads, for instance when it
// is suspended while waiting for IO.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<SerializeDriver<'static>>();
};

type NextEvent<'a> = Option<(Event<'a>, SerializeRef<'a>)>;

/// The callback of [`SerializeDriver::drive`] and
/// [`SerializeDriver::drive_until`].
trait Callback {
    /// `true` if the callback receives the values of the events.
    const DESCRIBED: bool;

    /// `true` if the callback can pause the driver (see
    /// [`SerializeDriver::drive_until`]).
    ///
    /// Plain values are only emitted at once if they fit into the budget
    /// (see `PLAIN_BUDGET`), large plain sequences are emitted in pieces
    /// so that the driver can pause in between.
    const PAUSABLE: bool = false;

    /// `true` if the fast paths for plain values are used.
    ///
    /// Callbacks that describe values need to see every value.
    const FAST: bool = !Self::DESCRIBED;

    /// `true` if large plain sequences are emitted at once.
    const UNBOUNDED: bool = Self::FAST && !Self::PAUSABLE;

    fn call(
        &mut self,
        event: Event<'_>,
        value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error>;

    /// Returns `true` if the driver should pause before the next value.
    #[inline(always)]
    fn pause(&mut self) -> bool {
        false
    }
}

/// A callback that does not receive values.
struct Plain<F>(F);

impl<F: FnMut(Event<'_>, &mut State) -> Result<(), Error>> Callback for Plain<F> {
    const DESCRIBED: bool = false;

    #[inline(always)]
    fn call(
        &mut self,
        event: Event<'_>,
        _value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error> {
        (self.0)(event, state)
    }
}

/// Receives the events of [`SerializeDriver::drive_sink`] and
/// [`SerializeDriver::drive_until`].
///
/// Unlike the callback of [`drive`](SerializeDriver::drive) an event
/// sink can:
///
/// * pause the driver (with [`drive_until`](SerializeDriver::drive_until)):
///   before the next value is serialized the driver asks the sink with
///   [`pause`](Self::pause) if it should stop.  This is used to write the
///   output of large values in pieces, for instance to wait until the
///   output that was produced so far was written to a socket.
/// * receive the values of the events (see [`DESCRIBED`](Self::DESCRIBED))
///   to [describe](crate::ser::Describe) them, which formats that reflect
///   the Rust shape of values need.
///
/// ```
/// # use deser::ser::{Describe, EventSink, SerializeDriver, SerializeRef};
/// # use deser::{Error, Event, State};
/// /// Records if the values are `Some`.
/// struct IsSome(Vec<bool>);
///
/// struct Describer(bool);
///
/// impl Describe for Describer {
///     fn some(&mut self) {
///         self.0 = true;
///     }
/// }
///
/// impl EventSink for IsSome {
///     const DESCRIBED: bool = true;
///
///     fn event(
///         &mut self,
///         _event: Event<'_>,
///         value: SerializeRef<'_>,
///         _state: &mut State,
///     ) -> Result<(), Error> {
///         let mut describer = Describer(false);
///         value.describe(&mut describer);
///         self.0.push(describer.0);
///         Ok(())
///     }
/// }
///
/// # fn do_it() -> Result<(), deser::Error> {
/// let mut sink = IsSome(Vec::new());
/// let value = vec![Some(1), None];
/// SerializeDriver::new(&value).drive_sink(&mut sink)?;
/// assert_eq!(sink.0, [false, true, false, false]);
/// # Ok(()) } do_it().unwrap();
/// ```
pub trait EventSink {
    /// `true` if the sink receives the values of the events.
    ///
    /// Otherwise the value passed to [`event`](Self::event) describes
    /// nothing.  Values are only passed on if they are wanted as the
    /// driver serializes values faster if it does not need to hand out
    /// every one of them.
    const DESCRIBED: bool = false;

    /// Receives an event.
    ///
    /// Formats can mark the implementation as `#[inline(always)]` which
    /// makes the compiler specialize it for every kind of event the driver
    /// delivers (which is not possible with the callback of
    /// [`drive`](SerializeDriver::drive)).
    fn event(
        &mut self,
        event: Event<'_>,
        value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error>;

    /// Returns `true` if the driver should pause.
    ///
    /// This is invoked between values: before the values in maps and
    /// sequences (not the keys of structs) and between the pieces of large
    /// values that only hold atoms.  After the first value of a call to
    /// [`drive_until`](SerializeDriver::drive_until) returns `true`, the
    /// call returns.  The default implementation never pauses.
    ///
    /// [`drive_sink`](SerializeDriver::drive_sink) does not invoke this.
    fn pause(&mut self) -> bool {
        false
    }
}

/// A callback that delivers to an event sink and does not pause.
struct Sink<'s, S>(&'s mut S);

impl<S: EventSink> Callback for Sink<'_, S> {
    const DESCRIBED: bool = S::DESCRIBED;

    #[inline(always)]
    fn call(
        &mut self,
        event: Event<'_>,
        value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.0.event(event, value, state)
    }
}

/// A callback that delivers to an event sink which can pause.
struct Pausable<'s, S>(&'s mut S);

impl<S: EventSink> Callback for Pausable<'_, S> {
    const DESCRIBED: bool = S::DESCRIBED;
    const PAUSABLE: bool = true;

    #[inline(always)]
    fn call(
        &mut self,
        event: Event<'_>,
        value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.0.event(event, value, state)
    }

    #[inline(always)]
    fn pause(&mut self) -> bool {
        self.0.pause()
    }
}

/// The value of the keys of structs, it describes nothing.
static FIELD_KEY: () = ();

/// Returns the value of the keys of structs.
#[inline(always)]
fn field_key() -> SerializeRef<'static> {
    SerializeRef::new(&FIELD_KEY)
}

/// Serializes a plain value into an `Emit`, for when every value is driven
/// on its own.
#[inline(never)]
fn serialize_plain<'x>(plain: SerializeRef<'x>, state: &mut State) -> Result<BeginKind<'x>, Error> {
    Ok(BeginKind::Emit(plain.serialize(state)?))
}

/// Delivers the events of plain values (see [`PlainSink`]).
///
/// The first event of a value is a key if `is_map_key` is set in the
/// state, it's reset after every such event.
struct PlainDelivery<'d, 'a, C> {
    driver: &'d mut SerializeDriver<'a>,
    f: &'d mut C,
}

impl<C: Callback> PlainDelivery<'_, '_, C> {
    /// Delivers the first event of a value.
    #[inline(always)]
    fn begin(&mut self, event: Event<'_>) -> Result<(), Error> {
        self.driver.deliver(self.f, event, field_key())?;
        self.driver.state.is_map_key = false;
        Ok(())
    }
}

impl<C: Callback> PlainSink for PlainDelivery<'_, '_, C> {
    #[inline]
    fn atom(&mut self, atom: Atom<'_>) -> Result<(), Error> {
        self.begin(Event::Atom(atom))
    }

    #[inline]
    fn seq_start(&mut self, shape: ContainerShape) -> Result<(), Error> {
        self.driver.state.depth += 1;
        self.begin(Event::SeqStart(shape))
    }

    #[inline]
    fn seq_end(&mut self) -> Result<(), Error> {
        self.driver.state.depth -= 1;
        self.driver.deliver(self.f, Event::SeqEnd, field_key())
    }

    #[inline]
    fn map_start(&mut self, shape: ContainerShape) -> Result<(), Error> {
        self.driver.state.depth += 1;
        self.begin(Event::MapStart(shape))
    }

    #[inline]
    fn map_end(&mut self) -> Result<(), Error> {
        self.driver.state.depth -= 1;
        self.driver.deliver(self.f, Event::MapEnd, field_key())
    }

    #[inline]
    fn key(&mut self) {
        self.driver.state.is_map_key = true;
    }

    #[inline]
    fn field(&mut self, name: &str) -> Result<(), Error> {
        self.driver.state.is_map_key = true;
        self.begin(Event::Atom(Atom::Str(Text::borrowed(name))))
    }
}

impl<'a> SerializeDriver<'a> {
    /// Creates a new driver which serializes the given value implementing [`Serialize`].
    #[inline]
    pub fn new<T: Serialize>(value: &'a T) -> SerializeDriver<'a> {
        SerializeDriver::from_ref(SerializeRef::new(value))
    }

    /// Creates a new driver which serializes the value of a reference.
    ///
    /// Unlike [`new`](Self::new) this is not generic, the reference can be
    /// to a value with an adapter (see [`SerializeRef::serialize_as`]).
    pub fn from_ref(serializable: SerializeRef<'a>) -> SerializeDriver<'a> {
        let mut state = State::new();
        // the stack of the last driver is reused
        let stack = state
            .arena
            .take_vec(Buffer::SerializeStack)
            .unwrap_or_else(|| Vec::with_capacity(STACK_CAPACITY));
        SerializeDriver {
            state,
            layers: Vec::new(),
            // SAFETY: the driver cannot outlive 'a
            next_value: Some(unsafe { Held::new(SerializeHandle::from(serializable)) }),
            needs_finish: None,
            stack,
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

    /// Sets the context of the serialization.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`]).  This replaces the context of the
    /// driver.  Formats add the values of their own context for the types
    /// it has no value for (see
    /// [`set_default_context`](Self::set_default_context)).
    pub fn set_context(&mut self, context: Context) {
        self.state.set_context(context);
    }

    /// Adds the values of a context that the context of the driver has no
    /// value for.
    ///
    /// Formats use this for the context they were given (for instance the
    /// one of their configuration).  A context that was set on the driver
    /// before (for instance in the setup callback of `serialize_with`)
    /// takes precedence: its values are kept and the values of the given
    /// context are only added for the types it has no value for.
    #[inline(never)]
    pub fn set_default_context(&mut self, context: Context) {
        let mut merged = self.state.context().clone();
        if merged.fill_from(&context) {
            self.set_context(merged);
        }
    }

    /// Returns the context of the serialization.
    pub fn context(&self) -> &Context {
        self.state.context()
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
    pub fn next(&mut self) -> Result<Option<(Event<'_>, SerializeRef<'_>, &mut State)>, Error> {
        assert!(
            self.layers.is_empty(),
            "layers are only supported by SerializeDriver::drive"
        );
        let rv = match self.advance() {
            Ok(rv) => rv,
            Err(err) => return Err(self.state.error_in_context(err)),
        };
        self.delivered = rv.is_some();
        // The event and the value borrow from the values held by the driver
        // but never from the state (serializables cannot return `Emit`s
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
    /// serialization.  To receive the values of the events or to pause the
    /// serialization, use an [`EventSink`].
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
            Ok(_) => Ok(()),
            Err(err) => Err(self.state.error_in_context(err)),
        }
    }

    /// Like [`drive`](Self::drive) but delivers the events to an
    /// [`EventSink`].
    ///
    /// The sink is not asked to pause (see [`drive_until`](Self::drive_until)),
    /// the value is serialized at once.
    #[inline]
    pub fn drive_sink<S: EventSink>(&mut self, sink: &mut S) -> Result<(), Error> {
        match self.drive_impl(Sink(sink)) {
            Ok(_) => Ok(()),
            Err(err) => Err(self.state.error_in_context(err)),
        }
    }

    /// Drives the serialization until it's complete or the sink pauses it.
    ///
    /// Returns `true` once the serialization is complete.  If the sink
    /// paused the driver (see [`EventSink::pause`]), `false` is returned
    /// and the next call continues where this one stopped.  At least one
    /// value is serialized per call.
    ///
    /// Unlike [`drive`](Self::drive), which emits values that only hold
    /// atoms (like a `Vec<u64>`) at once, the driver emits such values in
    /// pieces of a few hundred atoms and can pause in between.  The amount
    /// of events between two pauses only depends on the size of the atoms,
    /// not on the size of the value.
    ///
    /// ```
    /// # use deser::ser::{EventSink, SerializeDriver, SerializeRef};
    /// # use deser::{Error, Event, State};
    /// /// Collects events and pauses once it holds 100.
    /// struct Collect(Vec<Event<'static>>);
    ///
    /// impl EventSink for Collect {
    ///     fn event(
    ///         &mut self,
    ///         event: Event<'_>,
    ///         _value: SerializeRef<'_>,
    ///         _state: &mut State,
    ///     ) -> Result<(), Error> {
    ///         self.0.push(event.to_static());
    ///         Ok(())
    ///     }
    ///
    ///     fn pause(&mut self) -> bool {
    ///         self.0.len() >= 100
    ///     }
    /// }
    ///
    /// # fn do_it() -> Result<(), deser::Error> {
    /// let value: Vec<u64> = (0..10_000).collect();
    /// let mut driver = SerializeDriver::new(&value);
    /// let mut sink = Collect(Vec::new());
    /// let mut events = 0;
    /// loop {
    ///     let done = driver.drive_until(&mut sink)?;
    ///     // the events so far are processed while the driver is paused
    ///     assert!(sink.0.len() < 1000);
    ///     events += sink.0.len();
    ///     sink.0.clear();
    ///     if done {
    ///         break;
    ///     }
    /// }
    /// assert_eq!(events, 10_002);
    /// # Ok(()) } do_it().unwrap();
    /// ```
    #[inline]
    pub fn drive_until<S: EventSink>(&mut self, sink: &mut S) -> Result<bool, Error> {
        match self.drive_impl(Pausable(sink)) {
            Ok(done) => Ok(done),
            Err(err) => Err(self.state.error_in_context(err)),
        }
    }

    /// Delivers an event to the layers and the callback of
    /// [`drive`](Self::drive).
    #[inline(always)]
    fn deliver<C: Callback>(
        &mut self,
        f: &mut C,
        event: Event<'_>,
        value: SerializeRef<'_>,
    ) -> Result<(), Error> {
        // the value is only passed on if the callback wants it
        let value = if C::DESCRIBED { value } else { field_key() };
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
        value: SerializeRef<'_>,
    ) -> Result<(), Error> {
        Next::new(&mut self.layers, &mut self.state, f, value).emit(event)
    }

    /// Drives the serialization, returns `false` if the callback paused it.
    #[inline(always)]
    fn drive_impl<C: Callback>(&mut self, mut f: C) -> Result<bool, Error> {
        // `next` might have been used before.
        self.detach_delivered_event_data();
        if let Some((held, true)) = self.needs_finish.take() {
            // SAFETY: the value is alive until the end of this block
            unsafe { held.get() }.finish(&mut self.state)?;
        }
        if let Some(value) = self.next_value.take() {
            self.drive_value(value, false, &mut f)?;
        }

        // at least one value is serialized per call
        let mut first = true;
        while let Some(frame) = self.stack.last_mut() {
            // the state is complete between the iterations, the driver can
            // continue from here in the next call
            if C::PAUSABLE {
                if !first && f.pause() {
                    return Ok(false);
                }
                first = false;
            }
            // SAFETY: values produced by the emitter borrow from it.  The
            // frame stays on the stack until all of them are dropped.
            let emitter = unsafe { &mut *(&mut frame.emitter as *mut Emitter) };
            let value = match emitter {
                Emitter::Forward => {
                    self.finish_forward()?;
                    continue;
                }
                Emitter::IndexedStruct(fields, index) => {
                    if C::FAST {
                        *index = fields.emit_plain_fields(
                            *index,
                            C::PAUSABLE,
                            &mut PlainDelivery {
                                driver: self,
                                f: &mut f,
                            },
                        )?;
                        if *index == FIELDS_END {
                            self.drive_end(&mut f)?;
                            continue;
                        }
                    }
                    let field = fields.field(*index);
                    *index += 1;
                    match field {
                        StructField::Field(key, value) => {
                            self.state.is_map_key = true;
                            self.deliver(
                                &mut f,
                                Event::Atom(Atom::Str(Text::borrowed(key))),
                                field_key(),
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
                    // large plain sequences are emitted in pieces
                    if C::FAST && C::PAUSABLE {
                        // the sequence might be a key, its values are not
                        self.state.is_map_key = false;
                        let next = seq.emit_plain_chunk(
                            *index,
                            PLAIN_BUDGET,
                            &mut PlainDelivery {
                                driver: self,
                                f: &mut f,
                            },
                        )?;
                        if next != *index {
                            *index = next;
                            continue;
                        }
                    }
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
                        self.deliver(&mut f, Event::Atom(Atom::Str(key.into())), field_key())?;
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

        Ok(true)
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
        } = serializable.begin(&mut self.state)?;
        let kind = match kind {
            // callbacks that describe values need to see every value,
            // large plain values are driven on their own for callbacks that
            // pause
            BeginKind::Plain(plain)
                if C::UNBOUNDED || (C::FAST && plain.plain_cost(PLAIN_BUDGET).is_some()) =>
            {
                return plain.emit_plain(&mut PlainDelivery { driver: self, f });
            }
            BeginKind::Plain(plain) => serialize_plain(plain, &mut self.state)?,
            kind => kind,
        };
        let (emitter, event) = match kind {
            BeginKind::Emit(Emit::Atom(atom)) => {
                self.deliver(f, Event::Atom(atom), serializable)?;
                if needs_finish {
                    serializable.finish(&mut self.state)?;
                }
                return Ok(());
            }
            BeginKind::Emit(Emit::Struct(emitter)) => {
                (Emitter::Struct(emitter), Event::MapStart(shape))
            }
            BeginKind::Emit(Emit::Map(emitter)) => {
                (Emitter::Map(emitter, false), Event::MapStart(shape))
            }
            BeginKind::Emit(Emit::Seq(emitter)) => (Emitter::Seq(emitter), Event::SeqStart(shape)),
            // callbacks that describe values need to see every value
            BeginKind::Struct(fields) if C::FAST => {
                // an owned value is dropped here, not in the callee where
                // `fields` (which borrows from it) is an argument.
                let mut value = Some(value);
                let rv = self.drive_indexed_struct(&mut value, fields, shape, f);
                drop(value);
                return rv;
            }
            BeginKind::Struct(fields) => {
                (Emitter::IndexedStruct(fields, 0), Event::MapStart(shape))
            }
            // large sequences are emitted in pieces for callbacks that pause
            BeginKind::Seq(seq)
                if C::UNBOUNDED || (C::FAST && serializable.plain_cost(PLAIN_BUDGET).is_some()) =>
            {
                // see above
                let mut value = Some(value);
                let rv = self.drive_indexed_seq(&mut value, seq, shape, f);
                drop(value);
                return rv;
            }
            BeginKind::Seq(seq) => (Emitter::IndexedSeq(seq, 0), Event::SeqStart(shape)),
            BeginKind::Emit(Emit::Forward(forwarded)) => {
                let forwarded = self.push_forward(value, needs_finish, forwarded);
                return self.drive_forwarded(forwarded, is_key, f);
            }
            BeginKind::Plain(_) => unreachable!(),
        };
        self.stack.push(Frame {
            emitter,
            serializable: value,
            needs_finish,
        });
        self.state.depth += 1;
        self.deliver(f, event, serializable)
    }

    /// Starts an indexed struct.
    ///
    /// Its leading plain fields are emitted right away.  If all fields are
    /// plain the struct is ended, otherwise it's placed on the stack.
    ///
    /// The value is taken if it's placed on the stack, otherwise the caller
    /// drops it.  An owned value must not be dropped in here as `fields`
    /// borrows from it (it's an argument, which must stay valid for the
    /// whole call).
    #[inline(always)]
    fn drive_indexed_struct<C: Callback>(
        &mut self,
        value: &mut Option<Held>,
        fields: &'static dyn IndexedStruct,
        shape: ContainerShape,
        f: &mut C,
    ) -> Result<(), Error> {
        // SAFETY: the value is held by the caller or by the frame
        let serializable = unsafe { value.as_ref().unwrap().get() };
        self.state.depth += 1;
        self.deliver(f, Event::MapStart(shape), serializable)?;
        self.state.is_map_key = false;
        let index =
            fields.emit_plain_fields(0, C::PAUSABLE, &mut PlainDelivery { driver: self, f })?;
        if index == FIELDS_END {
            self.state.depth -= 1;
            self.deliver(f, Event::MapEnd, serializable)
        } else {
            self.stack.push(Frame {
                emitter: Emitter::IndexedStruct(fields, index),
                serializable: value.take().unwrap(),
                // indexed structs do not need `finish`
                needs_finish: false,
            });
            Ok(())
        }
    }

    /// Starts an indexed sequence.
    ///
    /// If its elements are plain, they are emitted right away and the
    /// sequence is ended.  Otherwise it's placed on the stack.
    ///
    /// The value is taken if it's placed on the stack, otherwise the caller
    /// drops it.  An owned value must not be dropped in here as `seq`
    /// borrows from it (it's an argument, which must stay valid for the
    /// whole call).
    #[inline(always)]
    fn drive_indexed_seq<C: Callback>(
        &mut self,
        value: &mut Option<Held>,
        seq: &'static dyn IndexedSeq,
        shape: ContainerShape,
        f: &mut C,
    ) -> Result<(), Error> {
        // SAFETY: the value is held by the caller or by the frame
        let serializable = unsafe { value.as_ref().unwrap().get() };
        self.state.depth += 1;
        self.deliver(f, Event::SeqStart(shape), serializable)?;
        self.state.is_map_key = false;
        let emitted = seq.emit_plain(&mut PlainDelivery { driver: self, f })?;
        if emitted {
            self.state.depth -= 1;
            self.deliver(f, Event::SeqEnd, serializable)
        } else {
            self.stack.push(Frame {
                emitter: Emitter::IndexedSeq(seq, 0),
                serializable: value.take().unwrap(),
                // indexed sequences do not need `finish`
                needs_finish: false,
            });
            Ok(())
        }
    }

    /// Ends the container on the top of the stack and emits the end event.
    ///
    /// Unlike with `next` the value does not need to outlive this call.
    #[inline]
    fn drive_end<C: Callback>(&mut self, f: &mut C) -> Result<(), Error> {
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
        emitter.release(&mut self.state);
        self.state.depth -= 1;
        // SAFETY: the value is held until the end of this function
        let value = unsafe { serializable.get() };
        self.deliver(f, event, value)?;
        if needs_finish {
            value.finish(&mut self.state)?;
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
                                core::mem::transmute::<Cow<'_, str>, Cow<'static, str>>(key)
                            };
                            self.state.is_map_key = true;
                            return Ok(Some((Event::Atom(Atom::Str(key.into())), field_key())));
                        }
                        None => None,
                    },
                    Emitter::IndexedSeq(seq, index) => {
                        let rv = seq.element(*index, &mut self.state)?;
                        *index += 1;
                        rv
                    }
                    Emitter::IndexedStruct(fields, index) => loop {
                        let field = fields.field(*index);
                        *index += 1;
                        match field {
                            StructField::Field(key, value) => {
                                // SAFETY: the value and key borrow from the
                                // serializable of the frame.
                                self.next_value = Some(unsafe { Held::new(value) });
                                self.state.is_map_key = true;
                                return Ok(Some((
                                    Event::Atom(Atom::Str(Text::borrowed(key))),
                                    field_key(),
                                )));
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
        emitter.release(&mut self.state);
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
    unsafe fn finished_value(&self) -> SerializeRef<'static> {
        match self.needs_finish {
            // SAFETY: the caller guarantees the value is not used for longer
            Some((ref held, _)) => unsafe { held.get() },
            None => field_key(),
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
        } = serializable.begin(&mut self.state)?;
        let kind = match kind {
            BeginKind::Plain(plain) => serialize_plain(plain, &mut self.state)?,
            kind => kind,
        };
        let (emitter, event) = match kind {
            BeginKind::Emit(Emit::Atom(atom)) => {
                self.needs_finish = Some((value, needs_finish));
                return Ok(Some((Event::Atom(atom), serializable)));
            }
            BeginKind::Emit(Emit::Struct(emitter)) => {
                (Emitter::Struct(emitter), Event::MapStart(shape))
            }
            BeginKind::Emit(Emit::Map(emitter)) => {
                (Emitter::Map(emitter, false), Event::MapStart(shape))
            }
            BeginKind::Emit(Emit::Seq(emitter)) => (Emitter::Seq(emitter), Event::SeqStart(shape)),
            BeginKind::Struct(fields) => {
                (Emitter::IndexedStruct(fields, 0), Event::MapStart(shape))
            }
            BeginKind::Seq(seq) => (Emitter::IndexedSeq(seq, 0), Event::SeqStart(shape)),
            BeginKind::Emit(Emit::Forward(forwarded)) => {
                let forwarded = self.push_forward(value, needs_finish, forwarded);
                return self.serialize_forwarded(forwarded, is_key);
            }
            BeginKind::Plain(_) => unreachable!(),
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
    let mut map = alloc::collections::BTreeMap::new();
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
            Event::MapStart(crate::ContainerShape::with_order(crate::Order::Sorted)),
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
        fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::Atom(Atom::Str(
                if state.get::<Uppercase>().is_some_and(|x| x.0) {
                    value.0.to_uppercase().into()
                } else {
                    value.0.into()
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
