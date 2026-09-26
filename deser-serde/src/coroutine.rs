//! Bridges serde by running it on a stackful coroutine.
//!
//! serde drives serialization (the value pushes into the serializer) and
//! deserialization (the value pulls from the deserializer) through nested
//! calls whereas deser inverts both.  Running the serde side on its own
//! stack allows it to be suspended whenever it needs the next event
//! (deserialization) or produced one (serialization).
//!
//! # Safety
//!
//! The coroutines borrow data: the value that is serialized and the data
//! that is deserialized (borrowed atoms).  They are created with
//! `Coroutine::with_stack_unchecked` which leaves it to us to ensure that
//! the borrowed data outlives every use of the coroutine:
//!
//! * The coroutines are owned by types which carry the lifetimes of the
//!   borrowed data (`CoStream<'a>` and `Feeder<'de, T>`), so they cannot
//!   be resumed after the data is gone.
//! * When dropped, the coroutine is resumed with a cancellation which
//!   makes all further serializer and deserializer calls fail.  The serde
//!   code then returns and its stack is unwound normally.  Should that not
//!   happen, corosensei unwinds the stack when it drops the coroutine.
//! * If a coroutine is leaked it is never resumed again, the borrowed data
//!   is not accessed after that point.  The stacks are always heap
//!   allocated and never borrowed.  A leaked stack is leaked as well, so
//!   memory on it is never reused (this upholds the guarantees of pinned
//!   values on the stack).  Stacks are only returned to the pool after the
//!   coroutine completed.
//!
//! Coroutines are not `Send`, so the types holding them are not `Send`
//! either and the serde code always runs on the thread it started on.
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};

use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use deser::ser::{Chunk, SerializeHandle};
use deser::{ErrorKind, Event, State};

use crate::de::{Source, ValueDe, unexpected_end};
use crate::error::Error;
use crate::ser::{Emit, EventSerializer, EventStream, StreamRoot};
use crate::sink::{Collector, Push};

/// The size of the coroutine stacks.
///
/// The memory is reserved but only the used pages are committed.
const STACK_SIZE: usize = 2 * 1024 * 1024;

/// The maximum number of stacks that are kept for reuse per thread.
const MAX_POOLED_STACKS: usize = 8;

thread_local! {
    static STACKS: RefCell<Vec<DefaultStack>> = const { RefCell::new(Vec::new()) };
}

fn take_stack() -> Result<DefaultStack, deser::Error> {
    if let Ok(Some(stack)) = STACKS.try_with(|stacks| stacks.borrow_mut().pop()) {
        return Ok(stack);
    }
    DefaultStack::new(STACK_SIZE).map_err(|err| {
        deser::Error::new(ErrorKind::Unexpected, "failed to allocate coroutine stack")
            .with_source(err)
    })
}

fn recycle_stack(stack: DefaultStack) {
    let _ = STACKS.try_with(move |stacks| {
        let mut stacks = stacks.borrow_mut();
        if stacks.len() < MAX_POOLED_STACKS {
            stacks.push(stack);
        }
    });
}

/// Inputs to coroutines which can request cancellation.
trait Cancel {
    fn cancel() -> Self;
}

/// A coroutine on a pooled stack.
struct Co<I: Cancel, Y, R> {
    inner: Option<Coroutine<I, Y, R, DefaultStack>>,
}

impl<I: Cancel, Y, R> Co<I, Y, R> {
    /// Creates a coroutine.
    ///
    /// # Safety
    ///
    /// The data borrowed by the function, the input, the yielded values
    /// and the return value must outlive the coroutine (see the module
    /// documentation).
    unsafe fn new<F>(func: F) -> Result<Co<I, Y, R>, deser::Error>
    where
        F: FnOnce(&Yielder<I, Y>, I) -> R,
    {
        let stack = take_stack()?;
        // SAFETY: upheld by the caller.
        let inner = unsafe { Coroutine::with_stack_unchecked(stack, func) };
        Ok(Co { inner: Some(inner) })
    }

    /// Resumes the coroutine, returns `None` if it already completed.
    ///
    /// Panics of the coroutine are propagated.
    fn resume(&mut self, input: I) -> Option<CoroutineResult<Y, R>> {
        let co = self.inner.as_mut()?;
        let rv = co.resume(input);
        if co.done()
            && let Some(co) = self.inner.take()
        {
            recycle_stack(co.into_stack());
        }
        Some(rv)
    }
}

impl<I: Cancel, Y, R> Drop for Co<I, Y, R> {
    fn drop(&mut self) {
        let Some(mut co) = self.inner.take() else {
            return;
        };
        if co.started() && !co.done() {
            // Let the serde code fail and return which unwinds its stack.
            // This does not require unwinding support (`panic = "abort"`).
            // There is no good way to report a panic from here.
            let _ = catch_unwind(AssertUnwindSafe(|| co.resume(I::cancel())));
        }
        if co.done() {
            recycle_stack(co.into_stack());
        }
        // otherwise the coroutine is dropped which unwinds it
    }
}

/// The input to a serializing coroutine.
enum Resume {
    Next,
    Cancel,
}

impl Cancel for Resume {
    fn cancel() -> Resume {
        Resume::Cancel
    }
}

type SerCo = Co<Resume, Event<'static>, Result<(), Error>>;

/// Passes the events of the serializer out of the coroutine.
struct YieldEmit<'y> {
    yielder: &'y Yielder<Resume, Event<'static>>,
    cancelled: bool,
}

impl Emit for YieldEmit<'_> {
    fn emit(&mut self, event: Event<'_>) -> Result<(), Error> {
        if !self.cancelled {
            match self.yielder.suspend(event.to_static()) {
                Resume::Next => return Ok(()),
                Resume::Cancel => self.cancelled = true,
            }
        }
        Err(Error::cancelled())
    }
}

/// The events of a value serialized on a coroutine.
struct CoStream<'a> {
    co: RefCell<SerCo>,
    depth: Cell<usize>,
    // the coroutine borrows the value for 'a
    _marker: PhantomData<&'a ()>,
}

#[cold]
fn multiple_values() -> deser::Error {
    deser::Error::new(
        ErrorKind::Unexpected,
        "serde serializer produced more than one value",
    )
}

impl EventStream for CoStream<'_> {
    fn next_event(&self) -> Result<Event<'static>, deser::Error> {
        let mut co = self.co.borrow_mut();
        let event = match co.resume(Resume::Next) {
            Some(CoroutineResult::Yield(event)) => event,
            Some(CoroutineResult::Return(Err(err))) => return Err(err.into_deser()),
            Some(CoroutineResult::Return(Ok(()))) | None => {
                return Err(unexpected_end().into_deser());
            }
        };
        let depth = match event {
            Event::Atom(_) => self.depth.get(),
            Event::MapStart(_) | Event::SeqStart(_) => self.depth.get() + 1,
            Event::MapEnd | Event::SeqEnd => self
                .depth
                .get()
                .checked_sub(1)
                .ok_or_else(multiple_values)?,
        };
        self.depth.set(depth);
        if depth == 0 {
            // the value is complete, let the serializer return.
            match co.resume(Resume::Next) {
                Some(CoroutineResult::Return(Ok(()))) | None => {}
                Some(CoroutineResult::Return(Err(err))) => return Err(err.into_deser()),
                Some(CoroutineResult::Yield(_)) => return Err(multiple_values()),
            }
        }
        Ok(event)
    }
}

/// Serializes a serde value on a coroutine.
pub(crate) fn serialize<'a, T: serde::Serialize + ?Sized>(
    value: &'a T,
) -> Result<Chunk<'a>, deser::Error> {
    // SAFETY: the coroutine borrows the value for 'a and is owned by the
    // `CoStream<'a>`.
    let co = unsafe {
        Co::new(
            move |yielder: &Yielder<Resume, Event<'static>>, input: Resume| {
                if let Resume::Cancel = input {
                    return Err(Error::cancelled());
                }
                let mut emit = YieldEmit {
                    yielder,
                    cancelled: false,
                };
                value.serialize(EventSerializer::new(&mut emit))
            },
        )?
    };
    let stream = CoStream {
        co: RefCell::new(co),
        depth: Cell::new(0),
        _marker: PhantomData,
    };
    match stream.next_event()? {
        // atoms are complete, the coroutine is already done.
        Event::Atom(atom) => Ok(Chunk::Atom(atom)),
        first => Ok(Chunk::Forward(SerializeHandle::boxed(StreamRoot::new(
            stream, first,
        )))),
    }
}

/// The input to a deserializing coroutine.
enum Feed<'de> {
    Event(Event<'de>),
    Cancel,
}

impl Cancel for Feed<'_> {
    fn cancel() -> Self {
        Feed::Cancel
    }
}

/// Passes the events into the coroutine.
struct YieldSource<'y, 'de> {
    yielder: &'y Yielder<Feed<'de>, ()>,
    peeked: Option<Event<'de>>,
    cancelled: bool,
}

impl<'de> YieldSource<'_, 'de> {
    fn pull(&mut self) -> Result<Event<'de>, Error> {
        if !self.cancelled {
            match self.yielder.suspend(()) {
                Feed::Event(event) => return Ok(event),
                Feed::Cancel => self.cancelled = true,
            }
        }
        Err(Error::cancelled())
    }
}

impl<'de> Source<'de> for YieldSource<'_, 'de> {
    fn next(&mut self) -> Result<Event<'de>, Error> {
        match self.peeked.take() {
            Some(event) => Ok(event),
            None => self.pull(),
        }
    }

    fn peek(&mut self) -> Result<&Event<'de>, Error> {
        if self.peeked.is_none() {
            self.peeked = Some(self.pull()?);
        }
        Ok(self.peeked.as_ref().unwrap())
    }
}

/// Feeds the events of a value into a deserializing coroutine.
pub(crate) struct Feeder<'de, T> {
    co: Option<Co<Feed<'de>, (), Result<T, Error>>>,
    value: Option<T>,
}

impl<'de, T> Default for Feeder<'de, T> {
    fn default() -> Self {
        Feeder {
            co: None,
            value: None,
        }
    }
}

impl<'de, T> Push<'de> for Feeder<'de, T> {
    fn push(&mut self, event: Event<'de>, _state: &State) -> Result<(), deser::Error> {
        let Some(ref mut co) = self.co else {
            return Ok(());
        };
        match co.resume(Feed::Event(event)) {
            Some(CoroutineResult::Yield(())) | None => Ok(()),
            Some(CoroutineResult::Return(Ok(value))) => {
                self.value = Some(value);
                Ok(())
            }
            // the error has no location, the driver attaches the one of
            // the current event.
            Some(CoroutineResult::Return(Err(err))) => Err(err.into_deser()),
        }
    }
}

impl<'de, T: serde::Deserialize<'de>> Collector<'de, T> for Feeder<'de, T> {
    fn begin(&mut self, event: Event<'de>, key: bool, state: &State) -> Result<(), deser::Error> {
        // SAFETY: the coroutine only borrows data for 'de and is owned by
        // the `Feeder<'de, T>`.
        let co = unsafe {
            Co::new(move |yielder: &Yielder<Feed<'de>, ()>, first: Feed<'de>| {
                let first = match first {
                    Feed::Event(event) => event,
                    Feed::Cancel => return Err(Error::cancelled()),
                };
                let mut src = YieldSource {
                    yielder,
                    peeked: Some(first),
                    cancelled: false,
                };
                T::deserialize(ValueDe::new(&mut src, key))
            })?
        };
        self.co = Some(co);
        self.push(event, state)
    }

    fn finish(&mut self) -> Result<T, deser::Error> {
        self.value
            .take()
            .ok_or_else(|| unexpected_end().into_deser())
    }
}
