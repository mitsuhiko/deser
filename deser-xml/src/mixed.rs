use std::borrow::Cow;
use std::ops::{Deref, DerefMut};

use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser_core::ser::{
    Chunk, Describe, SerializeHandle, StructEmitter, Variant, VariantKind, VariantRepr,
};
use deser_core::{Atom, Error, ErrorKind, Serialize, State, Text};

use crate::Names;

/// The content of an element in order: its text and child elements.
///
/// Elements are maps whose repeated keys are collected by key: the text of
/// `<p>x <b>y</b> z</p>` is `["x ", " z"]` for a `Vec<String>` field with
/// the text key.  `Mixed` keeps the order instead.  Every child element
/// and every text between them is a value of `T`, which receives it as a
/// map with a single entry: the name of the element (or the
/// [text key](crate::DeserializerConfig::text_key) for text) and its
/// value.  This is what externally tagged enums expect:
///
/// ```
/// use deser::{Deserialize, Serialize};
/// use deser_xml::Mixed;
///
/// #[derive(Debug, Deserialize, Serialize, PartialEq)]
/// enum Inline {
///     #[deser(rename = "$text")]
///     Text(String),
///     #[deser(rename = "b")]
///     Bold(String),
/// }
///
/// let p: Mixed<Inline> = deser_xml::from_str("<p>x <b>y</b> z</p>").unwrap();
/// assert_eq!(
///     p.0,
///     [Inline::Text("x ".into()), Inline::Bold("y".into()), Inline::Text(" z".into())]
/// );
/// ```
///
/// Attributes are not content, they are skipped.  To read them too
/// flatten `Mixed` into a struct: the fields of the struct take their
/// attributes and child elements, the rest is the content in order.
///
/// ```
/// # use deser::{Deserialize, Serialize};
/// # use deser_xml::Mixed;
/// # #[derive(Debug, Deserialize, Serialize, PartialEq)]
/// # enum Inline {
/// #     #[deser(rename = "$text")]
/// #     Text(String),
/// #     #[deser(rename = "b")]
/// #     Bold(String),
/// # }
/// #[derive(Debug, Deserialize, Serialize, PartialEq)]
/// #[deser(rename = "p")]
/// struct Paragraph {
///     #[deser(rename = "@class")]
///     class: Option<String>,
///     #[deser(flatten)]
///     content: Mixed<Inline>,
/// }
///
/// let p: Paragraph = deser_xml::from_str(r#"<p class="note">x <b>y</b></p>"#).unwrap();
/// assert_eq!(p.class.as_deref(), Some("note"));
/// assert_eq!(p.content.len(), 2);
/// assert_eq!(
///     deser_xml::to_string(&p).unwrap(),
///     r#"<p class="note">x <b>y</b></p>"#
/// );
/// ```
///
/// Text that is only whitespace is kept within the elements whose
/// content is `Mixed` (it's not text elsewhere): `<b>y</b> <b>z</b>` has
/// the space between the elements.  If `Mixed` is flattened into a
/// struct, this starts with the first value it takes.  Texts that are
/// separated by values that are skipped or taken by the fields of the
/// struct are separate values.
///
/// When serialized, each value becomes the entries of the element it
/// serializes as (unit variants are empty elements).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mixed<T>(pub Vec<T>);

impl<T> Mixed<T> {
    /// Creates empty content.
    pub const fn new() -> Mixed<T> {
        Mixed(Vec::new())
    }

    /// Returns the values.
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

impl<T> Default for Mixed<T> {
    fn default() -> Mixed<T> {
        Mixed::new()
    }
}

impl<T> Deref for Mixed<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T> DerefMut for Mixed<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

impl<T> From<Vec<T>> for Mixed<T> {
    fn from(values: Vec<T>) -> Mixed<T> {
        Mixed(values)
    }
}

impl<T> FromIterator<T> for Mixed<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Mixed<T> {
        Mixed(iter.into_iter().collect())
    }
}

impl<T> IntoIterator for Mixed<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Mixed<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// The depths of the elements whose whitespace is text.
///
/// Whitespace between child elements is not text, unless the content of
/// the element is [`Mixed`].  Its sink registers the depth within the
/// element here, the parser checks it before it drops whitespace.
#[derive(Debug, Default)]
pub(crate) struct KeepWhitespace(pub(crate) Vec<usize>);

impl KeepWhitespace {
    /// Returns `true` if whitespace is text at the current depth.
    pub(crate) fn applies(state: &State) -> bool {
        state
            .get::<KeepWhitespace>()
            .and_then(|keep| keep.0.last())
            .is_some_and(|&depth| depth == state.depth())
    }

    /// Forgets the elements that were closed.
    ///
    /// Sinks unregister themselves when they finish, but sinks that are
    /// dropped (after errors or as flattened fields without values) do
    /// not.
    pub(crate) fn prune(state: &mut State) {
        let depth = state.depth();
        if state.get::<KeepWhitespace>().is_some() {
            state
                .get_mut::<KeepWhitespace>()
                .0
                .retain(|&keep| keep <= depth);
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Mixed<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(MixedSink {
            out,
            values: Vec::new(),
            key: None,
            pending: None,
            depth: None,
        })
    }

    /// Missing content is empty.
    fn initial_value() -> Option<Self> {
        Some(Mixed::new())
    }
}

struct MixedSink<'a, 'de, T> {
    out: &'a mut Option<Mixed<T>>,
    values: Vec<T>,
    key: Option<String>,
    /// The value that receives the value of the last key.
    pending: Option<OwnedSink<'de, T>>,
    /// The depth registered in [`KeepWhitespace`].
    depth: Option<usize>,
}

impl<'de, T: Deserialize<'de>> MixedSink<'_, 'de, T> {
    /// Keeps whitespace in the element at the depth.
    fn keep_whitespace(&mut self, depth: usize, state: &mut State) {
        if self.depth.is_none() {
            self.depth = Some(depth);
            state.get_mut::<KeepWhitespace>().0.push(depth);
        }
    }

    /// Begins a value with the key and returns the sink of its value.
    fn begin(&mut self, key: &str, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.end(state)?;
        let pending = self.pending.insert(OwnedSink::deserialize());
        let sink = pending.borrow_mut();
        sink.map(state)?;
        let mut key_sink = sink.next_key(state)?;
        key_sink.atom(Atom::Lexical(Text::borrowed(key)), state)?;
        key_sink.finish(state)?;
        drop(key_sink);
        sink.next_value(state)
    }

    /// Ends the pending value.
    fn end(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some(mut pending) = self.pending.take() {
            pending.borrow_mut().finish(state)?;
            self.values.push(pending.take().ok_or_else(|| {
                Error::new(ErrorKind::Unexpected, "value of mixed content is missing")
            })?);
        }
        Ok(())
    }
}

impl<'de, T: Deserialize<'de>> Sink<'de> for MixedSink<'_, 'de, T> {
    /// Text is an element without attributes and child elements, empty
    /// text has no content.
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Null => Ok(()),
            Atom::Str(ref text) | Atom::Lexical(ref text) if text.is_empty() => Ok(()),
            Atom::Str(_) | Atom::Lexical(_) => {
                let mut sink = self.begin(text_key(state), state)?;
                sink.atom(atom, state)?;
                sink.finish(state)
            }
            atom => self.unexpected_atom(atom, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref text) | Atom::Lexical(ref text) if !text.is_empty() => {
                let mut sink = self.begin(text_key(state), state)?;
                sink.borrowed_atom(atom, state)?;
                sink.finish(state)
            }
            atom => self.atom(atom, state),
        }
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        // the depth is increased after this
        self.keep_whitespace(state.depth() + 1, state);
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.end(state)?;
        Ok(String::deserialize_into(&mut self.key))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let key = self.key.take().unwrap_or_default();
        Ok(self
            .value_for_key(&key, state)?
            .unwrap_or_else(SinkHandle::null))
    }

    /// Takes all keys but attributes when flattened into a struct.
    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        let prefix = names(state).attribute_prefix;
        if !prefix.is_empty() && key.starts_with(prefix) {
            return Ok(None);
        }
        // within the map of the element
        self.keep_whitespace(state.depth(), state);
        self.begin(key, state).map(Some)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.end(state)?;
        if let Some(depth) = self.depth.take() {
            let keep = &mut state.get_mut::<KeepWhitespace>().0;
            if keep.last() == Some(&depth) {
                keep.pop();
            }
        }
        *self.out = Some(Mixed(std::mem::take(&mut self.values)));
        Ok(())
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.key = None;
        match self.pending {
            Some(ref mut pending) => pending.borrow_mut().recover(err, state),
            None => Err(err),
        }
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("mixed content")
    }
}

fn names(state: &State) -> &Names {
    const DEFAULT: Names = Names::new();
    state.get::<Names>().unwrap_or(&DEFAULT)
}

fn text_key(state: &State) -> &'static str {
    names(state).text_key
}

impl<T: Serialize> Serialize for Mixed<T> {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Struct(Box::new(MixedEmitter {
            values: self.0.iter(),
            current: None,
        })))
    }
}

/// The entries of the value that is serialized.
enum Entries<'a> {
    Struct(Box<dyn StructEmitter + 'a>),
    /// A unit variant, an empty element.
    Unit(Option<Cow<'a, str>>),
}

struct MixedEmitter<'a, T> {
    values: std::slice::Iter<'a, T>,
    current: Option<(&'a T, Entries<'a>)>,
}

impl<'a, T: Serialize> StructEmitter for MixedEmitter<'a, T> {
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        loop {
            if let Some((_, ref mut entries)) = self.current {
                let entry = match entries {
                    Entries::Struct(emitter) => emitter.next(state)?,
                    Entries::Unit(name) => {
                        name.take().map(|name| (name, SerializeHandle::boxed("")))
                    }
                };
                // SAFETY: the entry borrows from `self.current`.  If it's
                // returned `self.current` is not touched again in this call,
                // otherwise it's dropped before `self.current` is replaced.
                // The borrow checker does not understand that the borrow
                // does not continue into the next iteration.
                let entry = unsafe {
                    std::mem::transmute::<
                        Option<(Cow<'_, str>, SerializeHandle<'_>)>,
                        Option<(Cow<'a, str>, SerializeHandle<'a>)>,
                    >(entry)
                };
                if let Some(entry) = entry {
                    return Ok(Some(entry));
                }
                let (value, entries) = self.current.take().unwrap();
                drop(entries);
                value.finish(state)?;
            }
            let Some(value) = self.values.next() else {
                return Ok(None);
            };
            let entries = match value.serialize(state)? {
                Chunk::Struct(emitter) => Entries::Struct(emitter),
                Chunk::Atom(Atom::Str(name) | Atom::Lexical(name)) if is_unit_variant(value) => {
                    Entries::Unit(Some(name.into_cow()))
                }
                Chunk::Atom(Atom::Null) => Entries::Unit(None),
                _ => {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "the values of mixed content must be structs or externally tagged enums",
                    ));
                }
            };
            self.current = Some((value, entries));
        }
    }
}

/// Returns `true` if the value is a unit variant that is its name.
fn is_unit_variant(value: &dyn Serialize) -> bool {
    struct Check(bool);

    impl Describe for Check {
        fn variant(&mut self, variant: &Variant<'_>) {
            self.0 = variant.kind == VariantKind::Unit && variant.repr == VariantRepr::External;
        }
    }

    let mut check = Check(false);
    value.describe(&mut check);
    check.0
}
