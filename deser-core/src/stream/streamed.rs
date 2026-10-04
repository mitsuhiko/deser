use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Deref, DerefMut};

use crate::State;
use crate::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use crate::error::Error;
use crate::event::{Atom, ContainerShape};
use crate::ser::{Begin, Describe, Emit, Serialize};

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
/// use deser::io::Reader;
/// use deser::stream::{Part, Streamed};
/// # use deser::de::{DeserializeDriver, Frame, StreamDeserializer};
/// # use deser::{Error, Event};
/// # /// Numbers on a line of their own form a page (with a sequence of the numbers).
/// # struct Pages;
/// # impl StreamDeserializer for Pages {
/// #     fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
/// #         Ok(match input.iter().position(|&b| b == b'\n') {
/// #             Some(end) => Frame::Value { start: 0, end, consumed: end + 1 },
/// #             None if eof && input.is_empty() => Frame::End,
/// #             None if eof => Frame::Value { start: 0, end: input.len(), consumed: input.len() },
/// #             None => Frame::Incomplete { consumed: 0 },
/// #         })
/// #     }
/// #     fn drive_frame<'de>(&mut self, frame: &'de [u8], driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
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
/// // `Pages` is the stream deserializer of a format with pages of numbers
/// let mut reader = Reader::new(&b"1 2 3"[..], Pages);
/// let mut items = Vec::new();
/// while let Some(next) = reader.read_next::<Page, u32>()? {
///     match next {
///         Part::Element(item) => items.push(item),
///         // the elements were handed out, they are not in the page
///         Part::Done(page) => assert!(page.items.is_empty()),
///     }
/// }
/// assert_eq!(items, [1, 2, 3]);
/// # } Ok(()) } example().unwrap();
/// ```
///
/// If the format can deserialize values while their input arrives (see
/// [`StreamDeserializer::drive_partial`](crate::de::StreamDeserializer::drive_partial)),
/// the memory used does not depend on the length of the sequence.  The
/// elements have to be owned (they cannot borrow from the input).  Without
/// IO, the elements are handed out by an
/// [`ElementReader`](crate::stream::ElementReader).
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
    type IntoIter = alloc::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Streamed<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

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
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Vec::<T>::serialize(&value.items, state)
    }

    fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
        Vec::<T>::finish(&value.items, state)
    }

    #[inline]
    fn __private_begin<'a>(value: &'a Self, state: &mut State) -> Result<Begin<'a>, Error> {
        Vec::<T>::__private_begin(&value.items, state)
    }

    fn container_shape(value: &Self) -> ContainerShape {
        Vec::<T>::container_shape(&value.items)
    }

    fn describe(value: &Self, d: &mut dyn Describe) {
        Vec::<T>::describe(&value.items, d)
    }
}

/// Hands out a complete element or collects it.
fn complete<T: Send + 'static>(value: T, items: &mut Vec<T>, state: &State) {
    let Err(value) = crate::stream::elements::hand_out(value, state) else {
        return;
    };
    items.push(value);
}

/// What streamed values expect.
const STREAMED_NAME: &str = "sequence";

impl<'de, T: Deserialize<'de> + 'static> Deserialize<'de> for Streamed<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            StreamedSink {
                out,
                items: Vec::new(),
            },
            state,
        )
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(STREAMED_NAME)
    }
}

struct StreamedSink<'a, T> {
    out: &'a mut Option<Streamed<T>>,
    items: Vec<T>,
}

impl<'a, 'de, T: Deserialize<'de> + 'static> Sink<'de> for StreamedSink<'a, T> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(STREAMED_NAME)
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::arena(
            ElementSink {
                sink: OwnedSink::deserialize(state),
                items: &mut self.items,
            },
            state,
        ))
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let mut value = None;
        T::__private_atom_into(&mut value, atom, state)?;
        if let Some(value) = value {
            complete(value, &mut self.items, state);
        }
        Ok(())
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut value = None;
        T::__private_borrowed_atom_into(&mut value, atom, state)?;
        if let Some(value) = value {
            complete(value, &mut self.items, state);
        }
        Ok(())
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        *self.out = Some(Streamed {
            items: core::mem::take(&mut self.items),
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
        self.sink.get_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().seq(state)
    }

    forward_to_owned!(sink);

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().finish(state)?;
        if let Some(value) = self.sink.take() {
            complete(value, self.items, state);
        }
        Ok(())
    }
}
