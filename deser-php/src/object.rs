//! Class names and property visibility (see the crate documentation).
use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt;

use deser_core::State;
use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{Describe, Emit, Serialize};
use deser_core::{Atom, ContainerShape, Error};

/// The class of a value, attached as event data to its first event.
///
/// The deserializer publishes the classes of objects, enum cases and
/// custom serialized objects, the serializer writes them.
#[derive(Debug, Default, Clone)]
pub(crate) struct ClassName(pub(crate) Option<String>);

/// The visibility of a property, attached as event data to its key.
#[derive(Debug, Default, Clone)]
pub(crate) struct PropertyVisibility(pub(crate) Option<Visibility>);

/// The visibility of a property of an object.
///
/// PHP writes the names of protected and private properties with a prefix
/// (`\0*\0name` and `\0Class\0name`).  The deserializer passes on the bare
/// name and the visibility as event data of the key (see
/// [`take_visibility`]), the serializer adds the prefix again if a key has
/// a visibility (see [`set_visibility`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Visibility {
    /// A public property (no prefix).
    #[default]
    Public,
    /// A protected property (`\0*\0name`).
    Protected,
    /// A private property of the given class (`\0Class\0name`).
    Private(String),
}

/// Takes the class of the current value from the state.
///
/// This is intended to be called by sinks from within
/// [`Sink::atom`] or [`Sink::map`].  Returns `None` if the value is not an
/// object (or an enum case or custom serialized object) or the data format
/// is not PHP's.
///
/// ```
/// use deser::State;
///
/// fn class(state: &mut State) -> Option<String> {
///     deser_php::take_class(state)
/// }
/// ```
pub fn take_class(state: &mut State) -> Option<String> {
    // the class is detached so that values that capture event data do not
    // keep an empty class which would replace the class of a wrapper
    state.take_event::<ClassName>().and_then(|class| class.0)
}

/// Sets the class of the value that is serialized.
///
/// This is what [`Object`] uses internally.  It must be called from
/// [`Serialize::serialize`] and applies to the value serialized from that
/// call (see [`State::event`]).  Maps are written as objects of the class,
/// strings as enum cases (`E:`) and bytes as custom serialized objects
/// (`C:`).  Serializers of other formats ignore it.
pub fn set_class<S: Into<String>>(state: &mut State, class: S) {
    state.event_mut::<ClassName>().0 = Some(class.into());
}

/// Takes the visibility of the current key from the state.
///
/// Returns `None` for keys that are not names of protected or private
/// properties.
pub fn take_visibility(state: &mut State) -> Option<Visibility> {
    state
        .take_event::<PropertyVisibility>()
        .and_then(|visibility| visibility.0)
}

/// Sets the visibility of the key that is serialized.
///
/// It must be called from [`Serialize::serialize`] of the key.  The key is
/// written with the prefix of the visibility.
pub fn set_visibility(state: &mut State, visibility: Visibility) {
    state.event_mut::<PropertyVisibility>().0 = Some(visibility);
}

/// A value with the class of a PHP object.
///
/// When deserialized the class of the value (if there is one) is captured,
/// when serialized the value is written as an object of the class:
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_php::Object;
///
/// let input = br#"O:4:"User":1:{s:4:"name";s:4:"Jane";}"#;
/// let user: Object<BTreeMap<String, String>> = deser_php::from_slice(input).unwrap();
/// assert_eq!(user.class.as_deref(), Some("User"));
/// assert_eq!(user.value["name"], "Jane");
/// assert_eq!(deser_php::to_vec(&user).unwrap(), input);
/// ```
///
/// Maps are written as objects, strings as enum cases (`E:`) and bytes as
/// custom serialized objects (`C:`).  Other formats do not have classes.
/// When deserializing from such formats, the class is `None`, when
/// serializing it's ignored.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Object<T> {
    /// The class of the value.
    pub class: Option<String>,
    /// The value.
    pub value: T,
}

impl<T> Object<T> {
    /// Creates a value with a class.
    pub fn new<S: Into<String>>(class: S, value: T) -> Object<T> {
        Object {
            class: Some(class.into()),
            value,
        }
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for Object<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.class {
            Some(ref class) => {
                write!(f, "{} ", class)?;
                fmt::Debug::fmt(&self.value, f)
            }
            None => fmt::Debug::fmt(&self.value, f),
        }
    }
}

impl<T: Serialize> Serialize for Object<T> {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        // the class is set after the value attached its data (like the
        // class of a recorded value), it replaces it
        let emit = T::serialize(&this.value, state)?;
        if let Some(ref class) = this.class {
            set_class(state, class.as_str());
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

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            ObjectSink {
                out,
                slot: None,
                compound: None,
                class: None,
            },
            state,
        )
    }

    fn expecting() -> Cow<'static, str> {
        T::expecting()
    }
}

struct ObjectSink<'a, 'de, T> {
    out: &'a mut Option<Object<T>>,
    // atoms are deserialized directly into this slot, maps and sequences
    // need a sink that lives across calls
    slot: Option<T>,
    compound: Option<OwnedSink<'de, T>>,
    class: Option<String>,
}

impl<'a, 'de, T: Deserialize<'de>> ObjectSink<'a, 'de, T> {
    fn compound(&mut self, state: &mut State) -> &mut dyn Sink<'de> {
        self.compound
            .get_or_insert_with(|| OwnedSink::deserialize(state))
            .get_mut()
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for ObjectSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.class = take_class(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.class = take_class(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.borrowed_atom(atom, state)?;
        sink.finish(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.class = take_class(state);
        self.compound(state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.class = take_class(state);
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
        let class = self.class.take();
        *self.out = value.map(|value| Object { class, value });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        if let Some(ref compound) = self.compound {
            return compound.get().expecting();
        }
        T::expecting()
    }
}
