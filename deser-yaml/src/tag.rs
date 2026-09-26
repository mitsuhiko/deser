//! Support for YAML tags.
//!
//! Tags are not part of the deser data model.  Standard tags that determine
//! the type of a value (`!!str`, `!!int`, `!!float`, `!!bool`, `!!null`,
//! `!!binary`, `!!timestamp`, `!!seq` and `!!map`) are handled by the format
//! itself.  All
//! other tags are exchanged out of band through the deserializer state:
//!
//! * When deserializing, the tag of a node is published into the
//!   [`State`] for the first event of the node (the atom or the
//!   start of the map or sequence).  Types can pick it up with [`take_tag`].
//!   Types which do not care about tags never see them, which means that
//!   unknown tags are transparent: the value of `!color red` is the string
//!   `red`.
//! * [`Tagged`] captures the tag of a value.
//!
//! Tags are reported fully resolved: `!foo` stays `!foo` but `!!set`
//! becomes `tag:yaml.org,2002:set` and tag handles declared with `%TAG`
//! directives are expanded.
use std::borrow::Cow;
use std::fmt;

use deser::State;
use deser::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser::ser::{Chunk, Describe, Serialize};
use deser::{Atom, ContainerShape, Error};

/// The tag of the current node, attached as event data.
#[derive(Debug, Default, Clone)]
pub(crate) struct CurrentTag(pub(crate) Option<String>);

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
    if state
        .event::<CurrentTag>()
        .is_some_and(|tag| tag.0.is_some())
    {
        state.event_mut::<CurrentTag>().0.take()
    } else {
        None
    }
}

/// A value with an optional YAML tag.
///
/// When deserialized the tag of the node (if there is one) is captured:
///
/// ```
/// use deser_yaml::Tagged;
///
/// let value: Vec<Tagged<String>> = deser_yaml::from_str("[!color red, blue]").unwrap();
/// assert_eq!(value[0].tag.as_deref(), Some("!color"));
/// assert_eq!(value[0].value, "red");
/// assert_eq!(value[1].tag, None);
/// ```
///
/// Other formats do not support tags.  When deserializing from such
/// formats, the tag is `None`.
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
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.value.serialize(state)
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.value.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.value.is_optional()
    }

    fn container_shape(&self) -> ContainerShape {
        self.value.container_shape()
    }

    fn describe(&self, d: &mut dyn Describe) {
        self.value.describe(d)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Tagged<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(TaggedSink {
            out,
            slot: None,
            compound: None,
            tag: None,
        })
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
    fn compound(&mut self) -> &mut dyn Sink<'de> {
        self.compound
            .get_or_insert_with(OwnedSink::deserialize)
            .borrow_mut()
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for TaggedSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        let mut sink = T::deserialize_into(&mut self.slot);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.tag = take_tag(state);
        let mut sink = T::deserialize_into(&mut self.slot);
        sink.borrowed_atom(atom, state)?;
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

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound().next_value(state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
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
        let tag = self.tag.take();
        *self.out = value.map(|value| Tagged { tag, value });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        if let Some(ref compound) = self.compound {
            return compound.borrow().expecting();
        }
        let mut slot = None;
        Cow::Owned(T::deserialize_into(&mut slot).expecting().into_owned())
    }
}
