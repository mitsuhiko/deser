//! The root element of documents.
use std::borrow::Cow;

use deser_core::State;
use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{Chunk, Describe, Serialize};
use deser_core::{Atom, ContainerShape, Error};

/// The name of the root element and the namespaces declared on it,
/// attached as event data to the first event of the root element.
///
/// The deserializer publishes them, the serializer writes the root element
/// with them.  Like the tags of CBOR and YAML they are kept by values that
/// capture event data (such as [`Recording`](deser_core::de::Recording)
/// and the values of `deser-value`), so documents that are read into such
/// values or transcoded keep their root element.
#[derive(Debug, Default)]
pub(crate) struct RootData {
    pub(crate) name: Option<String>,
    pub(crate) namespaces: Vec<(String, String)>,
}

// Event data is reset with `clone_from` which retains the memory only if
// it's forwarded (derived clones do not do that).
impl Clone for RootData {
    fn clone(&self) -> RootData {
        RootData {
            name: self.name.clone(),
            namespaces: self.namespaces.clone(),
        }
    }

    fn clone_from(&mut self, source: &RootData) {
        self.name.clone_from(&source.name);
        self.namespaces.clone_from(&source.namespaces);
    }
}

/// The namespaces declared on an element other than the root, attached as
/// event data to its first event.
///
/// Like [`RootData`] it's published by the deserializer and written by the
/// serializer, so values that capture event data keep the declarations.
/// An empty URI undeclares the default namespace (`xmlns=""`).
#[derive(Debug, Default)]
pub(crate) struct Declarations(pub(crate) Vec<(String, String)>);

impl Clone for Declarations {
    fn clone(&self) -> Declarations {
        Declarations(self.0.clone())
    }

    fn clone_from(&mut self, source: &Declarations) {
        self.0.clone_from(&source.0);
    }
}

/// A document: the value of the root element with its name and the
/// namespaces declared on it.
///
/// When deserialized the name of the root element and its namespace
/// declarations are captured, when serialized the root element is written
/// with them.  The name is kept as the deserializer passes on names: as
/// written, with the configured prefix of its namespace or as `{uri}local`
/// if namespaces are resolved (see [`DeserializerConfig`]).  The prefixes
/// of the namespaces are the ones the names use (the configured prefixes
/// for namespaces that have one).
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_xml::Root;
///
/// let doc: Root<BTreeMap<String, String>> =
///     deser_xml::from_str(r#"<rss xmlns:dc="urn:dc"><dc:creator>Jane</dc:creator></rss>"#)
///         .unwrap();
/// assert_eq!(doc.name.as_deref(), Some("rss"));
/// assert_eq!(doc.namespaces, [("dc".to_string(), "urn:dc".to_string())]);
/// assert_eq!(doc.value["dc:creator"], "Jane");
///
/// // values without a name (like maps) are named by the root
/// let doc = Root::new("r", BTreeMap::from([("@a", 1), ("b", 2)]));
/// assert_eq!(deser_xml::to_string(&doc).unwrap(), r#"<r a="1"><b>2</b></r>"#);
/// ```
///
/// The name of the root is used over the name of the type of the value
/// and the [configured name](crate::SerializerConfig::root), its namespaces
/// are declared before the [configured
/// ones](crate::SerializerConfig::namespaces) (which are left out if their
/// prefix is taken).  Only documents have a root: in other places, and when
/// serializing to other formats, the name and the namespaces are ignored,
/// when deserializing from other formats the name is `None` and there are
/// no namespaces.
///
/// [`DeserializerConfig`]: crate::DeserializerConfig
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Root<T> {
    /// The name of the root element.
    pub name: Option<String>,
    /// The namespaces declared on the root element as prefix and URI (the
    /// empty prefix is the default namespace).
    pub namespaces: Vec<(String, String)>,
    /// The value of the root element.
    pub value: T,
}

impl<T> Root<T> {
    /// Creates a root element with a name.
    pub fn new<S: Into<String>>(name: S, value: T) -> Root<T> {
        Root {
            name: Some(name.into()),
            namespaces: Vec::new(),
            value,
        }
    }

    /// Declares a namespace on the root element.
    ///
    /// ```
    /// use deser_xml::Root;
    ///
    /// let doc = Root::new("{urn:a}doc", "x").with_namespace("a", "urn:a");
    /// assert_eq!(
    ///     deser_xml::to_string(&doc).unwrap(),
    ///     r#"<a:doc xmlns:a="urn:a">x</a:doc>"#
    /// );
    /// ```
    pub fn with_namespace<P: Into<String>, U: Into<String>>(
        mut self,
        prefix: P,
        uri: U,
    ) -> Root<T> {
        self.namespaces.push((prefix.into(), uri.into()));
        self
    }

    /// Returns the value of the root element.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Serialize> Serialize for Root<T> {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        // the root is set after the value attached its data (like the root
        // of a recorded value), it replaces it
        let chunk = T::serialize(&this.value, state)?;
        if this.name.is_some() || !this.namespaces.is_empty() {
            let data = state.event_mut::<RootData>();
            data.name.clone_from(&this.name);
            data.namespaces.clone_from(&this.namespaces);
        }
        Ok(chunk)
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

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Root<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            RootSink {
                out,
                slot: None,
                compound: None,
                data: RootData::default(),
            },
            state,
        )
    }
}

struct RootSink<'a, 'de, T> {
    out: &'a mut Option<Root<T>>,
    // atoms are deserialized directly into this slot, maps and sequences
    // need a sink that lives across calls
    slot: Option<T>,
    compound: Option<OwnedSink<'de, T>>,
    data: RootData,
}

impl<'a, 'de, T: Deserialize<'de>> RootSink<'a, 'de, T> {
    /// Takes the root data of the first event from the state.
    ///
    /// It's detached so that the value does not see (or capture) it.
    fn take_data(&mut self, state: &mut State) {
        if let Some(data) = state.take_event::<RootData>() {
            self.data = data;
        }
    }

    fn compound(&mut self, state: &mut State) -> &mut dyn Sink<'de> {
        self.compound
            .get_or_insert_with(|| OwnedSink::deserialize(state))
            .borrow_mut()
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for RootSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.take_data(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.take_data(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.borrowed_atom(atom, state)?;
        sink.finish(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.take_data(state);
        self.compound(state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.take_data(state);
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
            Some(ref mut compound) => compound.borrow_mut().recover(err, state),
            None => Err(err),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let value = match self.compound {
            Some(ref mut compound) => {
                compound.borrow_mut().finish(state)?;
                compound.take()
            }
            None => self.slot.take(),
        };
        let RootData { name, namespaces } = std::mem::take(&mut self.data);
        *self.out = value.map(|value| Root {
            name,
            namespaces,
            value,
        });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        if let Some(ref compound) = self.compound {
            return compound.borrow().expecting();
        }
        let mut slot = None;
        let mut state = State::new();
        Cow::Owned(
            T::deserialize_into(&mut slot, &mut state)
                .expecting()
                .into_owned(),
        )
    }
}
