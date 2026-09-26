use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use crate::State;
use crate::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use crate::error::Error;
use crate::event::{Atom, ContainerShape};
use crate::ser::{Begin, Chunk, Describe, Serialize};

/// A sequence whose elements can be handed out while it's read.
///
/// `Streamed<T>` behaves like a `Vec<T>`: it serializes and deserializes
/// as a sequence and the elements are collected.  But when a value
/// containing it is read from a stream with `Reader::read_next` of
/// `deser::io` (or the equivalent of an async runtime), the
/// elements are handed out as soon as they are complete instead.  This allows processing large (or unbounded) sequences
/// within a value while the stream is read:
///
/// ```
/// # fn example() -> Result<(), deser::Error> {
/// # #[cfg(all(feature = "derive", feature = "io"))] {
/// use deser::Deserialize;
/// use deser::Streamed;
/// use deser::io::{Next, Reader};
/// # use deser::de::DeserializeDriver;
/// # use deser::io::{Decoder, Frame};
/// # use deser::{Error, Event};
/// # /// Numbers on a line of their own form a page (with a sequence of the numbers).
/// # struct PagesConfig;
/// # impl Decoder for PagesConfig {
/// #     type State = ();
/// #     fn frame(&self, _: &mut (), input: &[u8], eof: bool) -> Result<Frame, Error> {
/// #         Ok(match input.iter().position(|&b| b == b'\n') {
/// #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
/// #             None if eof && input.is_empty() => Frame::End,
/// #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
/// #             None => Frame::Incomplete { consumed: 0 },
/// #         })
/// #     }
/// #     fn drive<'de>(&self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
/// #         driver.emit(Event::map_start())?;
/// #         driver.emit("items")?;
/// #         driver.emit(Event::seq_start())?;
/// #         for number in std::str::from_utf8(frame).unwrap().split(' ') {
/// #             driver.emit(number.parse::<u64>().unwrap())?;
/// #         }
/// #         driver.emit(Event::SeqEnd)?;
/// #         driver.emit(Event::MapEnd)
/// #     }
/// # }
///
/// #[derive(Deserialize)]
/// struct Page {
///     items: Streamed<u32>,
/// }
///
/// // `PagesConfig` is the configuration of a format with pages of numbers
/// let mut reader = Reader::new(&b"1 2 3"[..], PagesConfig);
/// let mut items = Vec::new();
/// while let Some(next) = reader.read_next::<Page, u32>()? {
///     match next {
///         Next::Element(item) => items.push(item),
///         // the elements were handed out, they are not in the page
///         Next::Done(page) => assert!(page.items.is_empty()),
///     }
/// }
/// assert_eq!(items, [1, 2, 3]);
/// # } Ok(()) } example().unwrap();
/// ```
///
/// If the format can deserialize values while their input arrives (see
/// `deser::io::Decoder::feed`), the memory used does not depend on the length of the
/// sequence.  The elements have to be owned (they cannot borrow from the
/// input).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Streamed<T> {
    items: Vec<T>,
}

impl<T> Streamed<T> {
    /// Creates an empty sequence.
    pub fn new() -> Streamed<T> {
        Streamed { items: Vec::new() }
    }

    /// Returns the collected elements.
    pub fn into_vec(self) -> Vec<T> {
        self.items
    }
}

impl<T> Default for Streamed<T> {
    fn default() -> Streamed<T> {
        Streamed::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Streamed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.items.fmt(f)
    }
}

impl<T> From<Vec<T>> for Streamed<T> {
    fn from(items: Vec<T>) -> Streamed<T> {
        Streamed { items }
    }
}

impl<T> From<Streamed<T>> for Vec<T> {
    fn from(value: Streamed<T>) -> Vec<T> {
        value.items
    }
}

impl<T> FromIterator<T> for Streamed<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Streamed<T> {
        Streamed {
            items: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for Streamed<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Streamed<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T> Deref for Streamed<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.items
    }
}

impl<T> DerefMut for Streamed<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }
}

impl<T: Serialize> Serialize for Streamed<T> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.items.serialize(state)
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.items.finish(state)
    }

    #[inline]
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        self.items.__private_begin(state)
    }

    fn container_shape(&self) -> ContainerShape {
        self.items.container_shape()
    }

    fn describe(&self, d: &mut dyn Describe) {
        self.items.describe(d)
    }
}

/// The queue elements of [`Streamed`] are handed out through.
pub(crate) struct Queue {
    pub(crate) type_id: TypeId,
    pub(crate) elements: Mutex<VecDeque<Box<dyn Any + Send>>>,
}

impl Queue {
    #[cfg(feature = "io")]
    pub(crate) fn pop(&self) -> Option<Box<dyn Any + Send>> {
        self.elements.lock().unwrap().pop_front()
    }
}

/// The queue registered in the state of a value whose elements are handed
/// out (see `deser::io::ElementReader`).
#[derive(Clone, Default)]
pub(crate) struct ElementQueue(pub(crate) Option<Arc<Queue>>);

impl fmt::Debug for ElementQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementQueue").finish_non_exhaustive()
    }
}

/// Hands out a complete element or collects it.
fn complete<T: Send + 'static>(value: T, items: &mut Vec<T>, state: &State) {
    if let Some(ElementQueue(Some(queue))) = state.get::<ElementQueue>()
        && queue.type_id == TypeId::of::<T>()
    {
        queue.elements.lock().unwrap().push_back(Box::new(value));
    } else {
        items.push(value);
    }
}

impl<'de, T: Deserialize<'de> + 'static> Deserialize<'de> for Streamed<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(StreamedSink {
            out,
            items: Vec::new(),
        })
    }
}

struct StreamedSink<'a, T> {
    out: &'a mut Option<Streamed<T>>,
    items: Vec<T>,
}

impl<'a, 'de, T: Deserialize<'de> + 'static> Sink<'de> for StreamedSink<'a, T> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("sequence")
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::boxed(ElementSink {
            sink: OwnedSink::deserialize(),
            items: &mut self.items,
        }))
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let mut value = None;
        T::__private_atom_into(&mut value, atom, state)?;
        if let Some(value) = value {
            complete(value, &mut self.items, state);
        }
        Ok(())
    }

    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let mut value = None;
        T::__private_borrowed_atom_into(&mut value, atom, state)?;
        if let Some(value) = value {
            complete(value, &mut self.items, state);
        }
        Ok(())
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        *self.out = Some(Streamed {
            items: std::mem::take(&mut self.items),
        });
        Ok(())
    }
}

/// Deserializes an element and hands it out once it's complete.
struct ElementSink<'a, 'de, T> {
    sink: OwnedSink<'de, T>,
    items: &'a mut Vec<T>,
}

impl<'a, 'de, T: Deserialize<'de> + 'static> Sink<'de> for ElementSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_value(state)
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().key_atom(atom, state)
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().value_atom(atom, state)
    }

    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().borrowed_key_atom(atom, state)
    }

    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.sink.borrow_mut().value_for_key(key, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        if let Some(value) = self.sink.take() {
            complete(value, self.items, state);
        }
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.borrow().expecting()
    }
}
