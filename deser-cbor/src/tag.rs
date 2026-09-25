//! Support for CBOR tags.
//!
//! Tags are not part of the deser data model.  Instead they are exchanged
//! out of band through the [`State`]:
//!
//! * When deserializing, the tags in front of a data item are published into
//!   the state for the first event of the item (the atom or the start of the
//!   map or sequence).  Types can pick them up with
//!   [`take_tag`].  Types which do not care about tags never see them, which
//!   means that unknown tags are transparent.
//! * When serializing, [`Tagged`] registers its tag in the state and the
//!   serializer writes it in front of the next data item.
//!
//! The simplest way to work with tags is the [`Tagged`] wrapper.
//!
//! The bignum tags 2 and 3 are handled by the format itself: they are
//! converted to and from integers (and [`BigInt`](deser::ext::BigInt) for
//! bignums that do not fit into 128 bits).  The same applies to the tags of
//! the well-known types: date/time strings (tag 0), decimal fractions (tag
//! 4), UUIDs (tag 37) and full-date strings (tag 1004) are converted to and
//! from the respective [well-known types](deser::ext) if their content is
//! valid.  Otherwise they are passed on as tagged values.
use std::fmt;

use deser::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser::ser::{Chunk, Serialize};
use deser::State;
use deser::{Atom, Descriptor, Error};

/// The tags of the current data item, attached as event data when
/// deserializing.
///
/// The tags are ordered from the outermost to the innermost tag.
#[derive(Debug, Default)]
pub(crate) struct CurrentTags(pub(crate) Vec<u64>);

/// The tags that the serializer writes in front of a data item, attached as
/// event data when serializing.
#[derive(Debug, Default)]
pub(crate) struct PendingTags(pub(crate) Vec<u64>);

// Event data is reset with `clone_from` which retains the memory of the
// vectors only if it's forwarded (derived clones do not do that).
macro_rules! impl_clone {
    ($ty:ident) => {
        impl Clone for $ty {
            fn clone(&self) -> $ty {
                $ty(self.0.clone())
            }

            fn clone_from(&mut self, source: &$ty) {
                self.0.clone_from(&source.0);
            }
        }
    };
}

impl_clone!(CurrentTags);
impl_clone!(PendingTags);

/// Takes the outermost tag of the current data item from the state.
///
/// This is intended to be called by sinks from within
/// [`Sink::atom`], [`Sink::map`] or [`Sink::seq`].  Every call removes one
/// tag, so calling this repeatedly returns the nested tags from the outside
/// in.  Returns `None` if there are no (more) tags or the data format does
/// not support tags.
///
/// ```
/// use deser::State;
///
/// fn all_tags(state: &mut State) -> Vec<u64> {
///     std::iter::from_fn(|| deser_cbor::take_tag(state)).collect()
/// }
/// ```
pub fn take_tag(state: &mut State) -> Option<u64> {
    if state
        .event::<CurrentTags>()
        .is_some_and(|tags| !tags.0.is_empty())
    {
        Some(state.event_mut::<CurrentTags>().0.remove(0))
    } else {
        None
    }
}

/// Registers a tag to be written in front of the next data item.
///
/// This is what [`Tagged`] uses internally.  It must be called from
/// [`Serialize::serialize`] and applies to the value serialized from that
/// call.  The tag is attached to the first event of the value (see
/// [`State::event`]), serializers which do not support tags ignore it.
pub fn push_tag(state: &mut State, tag: u64) {
    state.event_mut::<PendingTags>().0.push(tag);
}

/// A value with an optional CBOR tag.
///
/// When serialized the tag is written in front of the value, when
/// deserialized the tag (if there is one) is captured.  If the value has
/// multiple tags, the outermost tag is captured and the inner tags are left
/// for the value (so `Tagged<Tagged<T>>` captures two tags).
///
/// ```
/// use deser_cbor::Tagged;
///
/// // 1(1363896240): an epoch based date/time
/// let bytes = deser_cbor::to_vec(&Tagged::new(1, 1363896240u64)).unwrap();
/// assert_eq!(bytes, [0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0]);
///
/// let value: Tagged<u64> = deser_cbor::from_slice(&bytes).unwrap();
/// assert_eq!(value.tag, Some(1));
/// assert_eq!(value.value, 1363896240);
/// ```
///
/// Other formats do not support tags.  When serializing to such formats the
/// tag is dropped, when deserializing the tag is `None`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tagged<T> {
    /// The tag of the value.
    pub tag: Option<u64>,
    /// The value.
    pub value: T,
}

impl<T> Tagged<T> {
    /// Creates a tagged value.
    pub fn new(tag: u64, value: T) -> Tagged<T> {
        Tagged {
            tag: Some(tag),
            value,
        }
    }

    /// Creates a value without a tag.
    pub fn untagged(value: T) -> Tagged<T> {
        Tagged { tag: None, value }
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for Tagged<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tag {
            Some(tag) => {
                write!(f, "{}(", tag)?;
                fmt::Debug::fmt(&self.value, f)?;
                write!(f, ")")
            }
            None => fmt::Debug::fmt(&self.value, f),
        }
    }
}

impl<T: Serialize> Serialize for Tagged<T> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(tag) = self.tag {
            push_tag(state, tag);
        }
        self.value.serialize(state)
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.value.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.value.is_optional()
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.value.descriptor()
    }
}

impl<T: Deserialize> Deserialize for Tagged<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SinkHandle::boxed(TaggedSink {
            out,
            slot: None,
            compound: None,
            tag: None,
        })
    }
}

struct TaggedSink<'a, T> {
    out: &'a mut Option<Tagged<T>>,
    // atoms are deserialized directly into this slot, maps and sequences
    // need a sink that lives across calls
    slot: Option<T>,
    compound: Option<OwnedSink<T>>,
    tag: Option<u64>,
}

impl<'a, T: Deserialize> TaggedSink<'a, T> {
    fn compound(&mut self) -> &mut dyn Sink {
        self.compound
            .get_or_insert_with(OwnedSink::deserialize)
            .borrow_mut()
    }
}

impl<'a, T: Deserialize> Sink for TaggedSink<'a, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        let mut sink = T::deserialize_into(&mut self.slot);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        self.compound().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        self.compound().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.compound().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.compound().next_value(state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_>>, Error> {
        self.compound().value_for_key(key, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let value = match self.compound {
            Some(ref mut compound) => {
                compound.borrow_mut().finish(state)?;
                compound.take()
            }
            None => self.slot.take(),
        };
        let tag = self.tag;
        *self.out = value.map(|value| Tagged { tag, value });
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        if let Some(ref compound) = self.compound {
            return compound.borrow().descriptor();
        }
        let mut slot = None;
        let descriptor = T::deserialize_into(&mut slot).descriptor();
        descriptor
    }
}
