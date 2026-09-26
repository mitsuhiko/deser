use std::any::TypeId;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use crate::de::{DeserializeOwned, OwnedDriver};
use crate::error::Error;
use crate::io::{DecodeBuffer, Decoder, Status};
use crate::streamed::{ElementQueue, Queue};

/// The result of [`Reader::read_next`](crate::io::Reader::read_next).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next<E, T> {
    /// An element of the [`Streamed`](crate::Streamed) sequence of the value.
    Element(E),
    /// The value is complete (without the elements that were handed out).
    Done(T),
}

/// The state of an [`ElementReader`], see [`ElementReader::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementStatus<E, T> {
    /// An element or the value is ready.
    Ready(Next<E, T>),
    /// More input is needed.
    NeedInput,
    /// There are no more values.
    End,
}

/// Reads a value and hands out the elements of its [`Streamed`](crate::Streamed) sequence
/// without doing IO.
///
/// This is to [`Reader::read_next`](crate::io::Reader::read_next) what the
/// [`DecodeBuffer`] is to the [`Reader`](crate::io::Reader): it reads the
/// value from a [`DecodeBuffer`] which is filled by the caller.  Adapters for
/// other kinds of IO (for instance async runtimes) use this.  `T` is the
/// type of the value, `E` the type of the elements of the [`Streamed`](crate::Streamed)
/// sequence which are handed out.
///
/// Elements are handed out before more input is needed, so an element is
/// available as soon as its last byte was read (if the decoder supports
/// [`Decoder::feed`], otherwise once the value is complete).
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
                elements: Mutex::new(VecDeque::new()),
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
    /// Once the value is ready ([`Next::Done`]) the reader is done, the next
    /// call starts with the next value.
    pub fn poll<D: Decoder>(
        &mut self,
        buffer: &mut DecodeBuffer<D>,
    ) -> Result<ElementStatus<E, T>, Error> {
        loop {
            if let Some(element) = self.queue.pop() {
                let element = *element.downcast::<E>().expect("elements are of type E");
                return Ok(ElementStatus::Ready(Next::Element(element)));
            }
            if let Some(value) = self.value.take() {
                return Ok(ElementStatus::Ready(Next::Done(value)));
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
                    if self.queue.elements.lock().unwrap().is_empty() {
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
