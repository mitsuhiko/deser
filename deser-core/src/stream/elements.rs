use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::any::{Any, TypeId};
use core::fmt;
use core::marker::PhantomData;

use crate::State;
use crate::de::{DeserializeOwned, OwnedDriver, StreamDeserializer};
use crate::error::Error;
use crate::stream::{InputBuffer, Status};
use crate::sync::Mutex;

/// The queue elements of [`Streamed`](super::Streamed) are handed out through.
struct Queue {
    type_id: TypeId,
    elements: Mutex<VecDeque<Box<dyn Any + Send>>>,
}

impl Queue {
    fn pop(&self) -> Option<Box<dyn Any + Send>> {
        self.elements.lock().pop_front()
    }
}

/// The queue registered in the state of a value whose elements are handed
/// out (see [`ElementReader`]).
#[derive(Clone, Default)]
struct ElementQueue(Option<Arc<Queue>>);

impl fmt::Debug for ElementQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementQueue").finish_non_exhaustive()
    }
}

/// Hands out a complete element if the value is read by an
/// [`ElementReader`] which hands out elements of its type.
///
/// Returns the element back if it's not handed out.
pub(crate) fn hand_out<T: Send + 'static>(value: T, state: &State) -> Result<(), T> {
    match state.get::<ElementQueue>() {
        Some(ElementQueue(Some(queue))) if queue.type_id == TypeId::of::<T>() => {
            queue.elements.lock().push_back(Box::new(value));
            Ok(())
        }
        _ => Err(value),
    }
}

/// A part of a value that is read with the elements of its
/// [`Streamed`](super::Streamed) sequence handed out.
///
/// Such a value is read in parts: first the elements, then the rest of the
/// value.  This is the result of [`ElementReader::poll`] (and of
/// `Reader::read_next` of `deser::io`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part<E, T> {
    /// An element of the [`Streamed`](super::Streamed) sequence of the value.
    Element(E),
    /// The value is complete (without the elements that were handed out).
    Done(T),
}

/// The state of an [`ElementReader`], see [`ElementReader::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementStatus<E, T> {
    /// An element or the value is ready.
    Ready(Part<E, T>),
    /// More input is needed.
    NeedInput,
    /// There are no more values.
    End,
}

/// Reads a value and hands out the elements of its [`Streamed`](super::Streamed) sequence
/// without doing IO.
///
/// This reads the value from an [`InputBuffer`] which is filled by the
/// caller.  `Reader::read_next` of `deser::io` and adapters for other
/// kinds of IO (for instance async runtimes) use this.  `T` is the
/// type of the value, `E` the type of the elements of the [`Streamed`](super::Streamed)
/// sequence which are handed out.
///
/// Elements are handed out before more input is needed, so an element is
/// available as soon as its last byte was read (if the stream
/// deserializer supports [`StreamDeserializer::feed`], otherwise once the
/// value is complete).
pub struct ElementReader<T, E> {
    queue: Arc<Queue>,
    // the value that is deserialized while its input arrives
    driver: Option<OwnedDriver<'static, T>>,
    // the complete value, handed out after the elements
    value: Option<T>,
    _marker: PhantomData<fn() -> E>,
}

impl<T: DeserializeOwned + 'static, E: Send + 'static> Default for ElementReader<T, E> {
    fn default() -> ElementReader<T, E> {
        ElementReader::new()
    }
}

impl<T: DeserializeOwned + 'static, E: Send + 'static> ElementReader<T, E> {
    /// Creates a reader for a value.
    pub fn new() -> ElementReader<T, E> {
        ElementReader {
            queue: Arc::new(Queue {
                type_id: TypeId::of::<E>(),
                elements: Mutex::default(),
            }),
            driver: None,
            value: None,
            _marker: PhantomData,
        }
    }

    /// Returns `true` if a value is being read.
    ///
    /// This is `false` before the first call and after the value was
    /// handed out.
    pub fn is_reading(&self) -> bool {
        self.driver.is_some() || self.value.is_some()
    }

    /// Returns the next element or the value.
    ///
    /// Once the value is ready ([`Part::Done`]) the reader is done, the next
    /// call starts with the next value.
    pub fn poll<D: StreamDeserializer>(
        &mut self,
        buffer: &mut InputBuffer<D>,
    ) -> Result<ElementStatus<E, T>, Error> {
        loop {
            if let Some(element) = self.queue.pop() {
                let element = *element.downcast::<E>().expect("elements are of type E");
                return Ok(ElementStatus::Ready(Part::Element(element)));
            }
            if let Some(value) = self.value.take() {
                return Ok(ElementStatus::Ready(Part::Done(value)));
            }

            let register = ElementQueue(Some(self.queue.clone()));
            if !buffer.supports_feed() {
                match buffer.poll()? {
                    Status::Ready => {
                        self.value = Some(buffer.deserialize_with(|driver| {
                            *driver.state_mut().get_mut::<ElementQueue>() = register;
                        })?);
                    }
                    Status::NeedInput => return Ok(ElementStatus::NeedInput),
                    Status::End => return Ok(ElementStatus::End),
                }
                continue;
            }

            let driver = self.driver.get_or_insert_with(|| {
                let mut driver = OwnedDriver::new();
                driver.with(|driver| *driver.state_mut().get_mut::<ElementQueue>() = register);
                driver
            });
            match driver.with(|driver| buffer.feed(driver)) {
                Ok(Status::Ready) => {
                    self.value = Some(self.driver.take().unwrap().finish()?);
                }
                // elements that were completed are handed out first
                Ok(Status::NeedInput) => {
                    if self.queue.elements.lock().is_empty() {
                        return Ok(ElementStatus::NeedInput);
                    }
                }
                Ok(Status::End) => {
                    self.driver = None;
                    return Ok(ElementStatus::End);
                }
                Err(err) => {
                    self.driver = None;
                    return Err(err);
                }
            }
        }
    }
}
