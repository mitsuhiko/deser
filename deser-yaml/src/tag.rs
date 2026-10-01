//! Support for YAML tags (see the crate documentation).
use std::borrow::Cow;
use std::fmt;

use deser_core::State;
use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{Describe, Emit, Serialize};
use deser_core::{Atom, ContainerShape, Error};

/// The tag of a node, attached as event data to its first event.
///
/// The deserializer publishes the tags it reads, the serializer writes the
/// tags in front of the nodes.
#[derive(Debug, Default, Clone)]
pub(crate) struct NodeTag(pub(crate) Option<String>);

/// Takes the tag of the current node from the state.
///
/// This is intended to be called by sinks from within [`Sink::atom`],
/// [`Sink::map`] or [`Sink::seq`].  Returns `None` if the node has no tag
/// (or only a standard tag) or the data format does not support tags.
///
/// ```
/// use deser::State;
///
/// fn tag(state: &mut State) -> Option<String> {
///     deser_yaml::take_tag(state)
/// }
/// ```
pub fn take_tag(state: &mut State) -> Option<String> {
    // the tag is detached so that values that capture event data do not
    // keep an empty tag which would replace the tag of a wrapper
    state.take_event::<NodeTag>().and_then(|tag| tag.0)
}

/// Sets the tag of the node that is serialized.
///
/// This is what [`Tagged`] uses internally.  It must be called from
/// [`Serialize::serialize`] and applies to the value serialized from that
/// call.  The tag is attached to the first event of the value (see
/// [`State::event`]), serializers which do not support tags ignore it.
/// Tags are given fully resolved: `!foo` for local tags and
/// `tag:yaml.org,2002:set` for `!!set`.
pub fn set_tag<S: Into<String>>(state: &mut State, tag: S) {
    state.event_mut::<NodeTag>().0 = Some(tag.into());
}

/// A value with an optional YAML tag.
///
/// When deserialized the tag of the node (if there is one) is captured,
/// when serialized it's written in front of the value:
///
/// ```
/// use deser_yaml::Tagged;
///
/// let value: Vec<Tagged<String>> =
///     deser_yaml::from_str("[!color red, blue]").unwrap();
/// assert_eq!(value[0].tag.as_deref(), Some("!color"));
/// assert_eq!(value[0].value, "red");
/// assert_eq!(value[1].tag, None);
/// assert_eq!(
///     deser_yaml::to_string(&value).unwrap(),
///     "- !color red\n- blue\n"
/// );
/// ```
///
/// Other formats do not support tags.  When deserializing from such
/// formats, the tag is `None`, when serializing it's ignored.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tagged<T> {
    /// The tag of the value.
    pub tag: Option<String>,
    /// The value.
    pub value: T,
}

impl<T> Tagged<T> {
    /// Creates a tagged value.
    pub fn new<S: Into<String>>(tag: S, value: T) -> Tagged<T> {
        Tagged {
            tag: Some(tag.into()),
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
            Some(ref tag) => {
                write!(f, "{} ", tag)?;
                fmt::Debug::fmt(&self.value, f)
            }
            None => fmt::Debug::fmt(&self.value, f),
        }
    }
}

impl<T: Serialize> Serialize for Tagged<T> {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        // the tag is set after the value attached its data (like the tag of
        // a recorded value), it replaces it
        let emit = T::serialize(&this.value, state)?;
        if let Some(ref tag) = this.tag {
            set_tag(state, tag.as_str());
        }
        Ok(emit)
    }

    fn finish(this: &Self, state: &mut State) -> Result<(), Error> {
        T::finish(&this.value, state)
    }

    fn is_optional(this: &Self) -> bool {
        T::is_optional(&this.value)
    }

    fn container_shape(this: &Self) -> ContainerShape {
        T::container_shape(&this.value)
    }

    fn describe(this: &Self, d: &mut dyn Describe) {
        T::describe(&this.value, d)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Tagged<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            TaggedSink {
                out,
                slot: None,
                compound: None,
                tag: None,
            },
            state,
        )
    }

    fn expecting() -> Cow<'static, str> {
        T::expecting()
    }
}

struct TaggedSink<'a, 'de, T> {
    out: &'a mut Option<Tagged<T>>,
    // atoms are deserialized directly into this slot, maps and sequences
    // need a sink that lives across calls
    slot: Option<T>,
    compound: Option<OwnedSink<'de, T>>,
    tag: Option<String>,
}

impl<'a, 'de, T: Deserialize<'de>> TaggedSink<'a, 'de, T> {
    fn compound(&mut self, state: &mut State) -> &mut dyn Sink<'de> {
        self.compound
            .get_or_insert_with(|| OwnedSink::deserialize(state))
            .get_mut()
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for TaggedSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.borrowed_atom(atom, state)?;
        sink.finish(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        self.compound(state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        self.compound(state).seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound(state).next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound(state).next_value(state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.compound(state).value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        match self.compound {
            Some(ref mut compound) => compound.get_mut().recover(err, state),
            None => Err(err),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let value = match self.compound {
            Some(ref mut compound) => {
                compound.get_mut().finish(state)?;
                compound.take()
            }
            None => self.slot.take(),
        };
        let tag = self.tag.take();
        *self.out = value.map(|value| Tagged { tag, value });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        if let Some(ref compound) = self.compound {
            return compound.get().expecting();
        }
        T::expecting()
    }
}
