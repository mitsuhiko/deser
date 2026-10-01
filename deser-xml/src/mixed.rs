use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle, default_atom};
use deser_core::ser::SerializeRef;
use deser_core::ser::{
    Boxed, Describe, Emit, SerializeHandle, StructEmitter, Variant, VariantKind, VariantRepr,
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
/// let p: Mixed<Inline> =
///     deser_xml::from_str("<p>x <b>y</b> z</p>").unwrap();
/// assert_eq!(
///     p.0,
///     [
///         Inline::Text("x ".into()),
///         Inline::Bold("y".into()),
///         Inline::Text(" z".into())
///     ]
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
/// let p: Paragraph =
///     deser_xml::from_str(r#"<p class="note">x <b>y</b></p>"#).unwrap();
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
/// struct are separate values.  For content where whitespace is only
/// indentation, [`SkipWhitespace`] leaves it out:
///
/// ```
/// use deser::Deserialize;
/// use deser::adapters::TrimWhitespace;
/// use deser_xml::{Mixed, SkipWhitespace};
///
/// #[derive(Debug, Deserialize, PartialEq)]
/// enum Block {
///     #[deser(rename = "$text")]
///     Text(#[deser(as = TrimWhitespace)] String),
///     #[deser(rename = "p")]
///     Paragraph(String),
/// }
///
/// let doc: Mixed<Block, SkipWhitespace> = deser_xml::from_str("
///     <doc>
///       <p>a</p>
///       text
///       <p>b</p>
///     </doc>
/// ").unwrap();
/// assert_eq!(doc.0, [
///     Block::Paragraph("a".into()),
///     Block::Text("text".into()),
///     Block::Paragraph("b".into()),
/// ]);
/// ```
///
/// Values that are left out by `T` (that leave no value like
/// [`SkipBlank`](deser_core::adapters::SkipBlank) does) are not content.
///
/// When serialized, each value becomes the entries of the element it
/// serializes as (unit variants are empty elements).  As whitespace is
/// text, indented output (see
/// [`SerializerConfig::indent`](crate::SerializerConfig::indent)) writes
/// the content on a single line, unless it's [`SkipWhitespace`].
pub struct Mixed<T, W = KeepWhitespace>(pub Vec<T>, PhantomData<fn() -> W>);

/// What [`Mixed`] does with text that is only whitespace.
///
/// This is [`KeepWhitespace`] or [`SkipWhitespace`].
pub trait Whitespace: sealed::Sealed + 'static {
    #[doc(hidden)]
    const KEEP: bool;
}

/// [`Mixed`] keeps text that is only whitespace (the default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeepWhitespace;

/// [`Mixed`] leaves out text that is only whitespace.
///
/// Whitespace between child elements is not text, as it's not for other
/// types than `Mixed`.  An element that is only whitespace has no
/// content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkipWhitespace;

impl Whitespace for KeepWhitespace {
    const KEEP: bool = true;
}

impl Whitespace for SkipWhitespace {
    const KEEP: bool = false;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::KeepWhitespace {}
    impl Sealed for super::SkipWhitespace {}
}

impl<T, W> Mixed<T, W> {
    /// Creates empty content.
    pub const fn new() -> Mixed<T, W> {
        Mixed(Vec::new(), PhantomData)
    }

    /// Returns the values.
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

impl<T: fmt::Debug, W> fmt::Debug for Mixed<T, W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Mixed").field(&self.0).finish()
    }
}

impl<T: Clone, W> Clone for Mixed<T, W> {
    fn clone(&self) -> Mixed<T, W> {
        Mixed(self.0.clone(), PhantomData)
    }
}

impl<T: PartialEq, W> PartialEq for Mixed<T, W> {
    fn eq(&self, other: &Mixed<T, W>) -> bool {
        self.0 == other.0
    }
}

impl<T: Eq, W> Eq for Mixed<T, W> {}

impl<T: PartialOrd, W> PartialOrd for Mixed<T, W> {
    fn partial_cmp(&self, other: &Mixed<T, W>) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T: Ord, W> Ord for Mixed<T, W> {
    fn cmp(&self, other: &Mixed<T, W>) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T: Hash, W> Hash for Mixed<T, W> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl<T, W> Default for Mixed<T, W> {
    fn default() -> Mixed<T, W> {
        Mixed::new()
    }
}

impl<T, W> Deref for Mixed<T, W> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T, W> DerefMut for Mixed<T, W> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

impl<T, W> From<Vec<T>> for Mixed<T, W> {
    fn from(values: Vec<T>) -> Mixed<T, W> {
        Mixed(values, PhantomData)
    }
}

impl<T, W> FromIterator<T> for Mixed<T, W> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Mixed<T, W> {
        Mixed(iter.into_iter().collect(), PhantomData)
    }
}

impl<T, W> IntoIterator for Mixed<T, W> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T, W> IntoIterator for &'a Mixed<T, W> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// The depths of the elements whose whitespace is text.
///
/// Whitespace between child elements is not text, unless the content of
/// the element is [`Mixed`] (with [`KeepWhitespace`]).  Its sink registers
/// the depth within the element here, the parser checks it before it
/// drops whitespace.
#[derive(Debug, Default)]
pub(crate) struct WhitespaceDepths(pub(crate) Vec<usize>);

impl WhitespaceDepths {
    /// Returns `true` if whitespace is text at the current depth.
    pub(crate) fn applies(state: &State) -> bool {
        state
            .get::<WhitespaceDepths>()
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
        if state.get::<WhitespaceDepths>().is_some() {
            state
                .get_mut::<WhitespaceDepths>()
                .0
                .retain(|&keep| keep <= depth);
        }
    }
}

impl<'de, T: Deserialize<'de>, W: Whitespace> Deserialize<'de> for Mixed<T, W> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            MixedSink {
                out,
                values: Vec::new(),
                key: None,
                pending: None,
                depth: None,
            },
            state,
        )
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("mixed content")
    }

    /// Missing content is empty.
    fn initial_value() -> Option<Self> {
        Some(Mixed::new())
    }
}

struct MixedSink<'a, 'de, T, W> {
    out: &'a mut Option<Mixed<T, W>>,
    values: Vec<T>,
    key: Option<String>,
    /// The value that receives the value of the last key.
    pending: Option<OwnedSink<'de, T>>,
    /// The depth registered in [`WhitespaceDepths`].
    depth: Option<usize>,
}

impl<'de, T: Deserialize<'de>, W: Whitespace> MixedSink<'_, 'de, T, W> {
    /// Keeps whitespace in the element at the depth.
    fn keep_whitespace(&mut self, depth: usize, state: &mut State) {
        if W::KEEP && self.depth.is_none() {
            self.depth = Some(depth);
            state.get_mut::<WhitespaceDepths>().0.push(depth);
        }
    }

    /// Returns `true` if the text is content.
    fn is_content(text: &str) -> bool {
        if W::KEEP {
            !text.is_empty()
        } else {
            !text.trim().is_empty()
        }
    }

    /// Begins a value with the key and returns the sink of its value.
    fn begin(&mut self, key: &str, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.end(state)?;
        let pending = self.pending.insert(OwnedSink::deserialize(state));
        let sink = pending.get_mut();
        sink.map(state)?;
        let mut key_sink = sink.next_key(state)?;
        key_sink.atom(Atom::Lexical(Text::borrowed(key)), state)?;
        key_sink.finish(state)?;
        drop(key_sink);
        sink.next_value(state)
    }

    /// Ends the pending value.
    ///
    /// Values that leave no value are left out.
    fn end(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some(mut pending) = self.pending.take() {
            pending.get_mut().finish(state)?;
            self.values.extend(pending.take());
        }
        Ok(())
    }
}

impl<'de, T: Deserialize<'de>, W: Whitespace> Sink<'de> for MixedSink<'_, 'de, T, W> {
    /// Text is an element without attributes and child elements, empty
    /// text (and blank text with [`SkipWhitespace`]) has no content.
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Null => Ok(()),
            Atom::Str(ref text) | Atom::Lexical(ref text) if !Self::is_content(text) => Ok(()),
            Atom::Str(_) | Atom::Lexical(_) => {
                let mut sink = self.begin(text_key(state), state)?;
                sink.atom(atom, state)?;
                sink.finish(state)
            }
            atom => default_atom(self, atom, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref text) | Atom::Lexical(ref text) if Self::is_content(text) => {
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
        Ok(String::deserialize_into(&mut self.key, state))
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
            let keep = &mut state.get_mut::<WhitespaceDepths>().0;
            if keep.last() == Some(&depth) {
                keep.pop();
            }
        }
        *self.out = Some(Mixed(std::mem::take(&mut self.values), PhantomData));
        Ok(())
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.key = None;
        match self.pending {
            Some(ref mut pending) => pending.get_mut().recover(err, state),
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

/// Marks content that keeps whitespace as text, the serializer does not
/// indent it.
///
/// This is event data of the start of the map or, if the content is
/// flattened, of its first key.
#[derive(Debug, Default, Clone)]
pub(crate) struct KeepsWhitespace(pub(crate) bool);

impl<T: Serialize, W: Whitespace> Serialize for Mixed<T, W> {
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        if W::KEEP {
            state.event_mut::<KeepsWhitespace>().0 = true;
        }
        Ok(Emit::structure(
            MixedEmitter {
                values: value.0.iter(),
                current: None,
            },
            state,
        ))
    }
}

/// The entries of the value that is serialized.
enum Entries<'a> {
    Struct(Boxed<dyn StructEmitter + 'a>),
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
                    Entries::Unit(name) => name.take().map(|name| (name, SerializeHandle::to(&""))),
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
                T::finish(value, state)?;
            }
            let Some(value) = self.values.next() else {
                return Ok(None);
            };
            let entries = match T::serialize(value, state)? {
                Emit::Struct(emitter) => Entries::Struct(emitter),
                Emit::Atom(Atom::Str(name) | Atom::Lexical(name))
                    if is_unit_variant(SerializeRef::new(value)) =>
                {
                    Entries::Unit(Some(name.into_cow()))
                }
                Emit::Atom(Atom::Null) => Entries::Unit(None),
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
fn is_unit_variant(value: SerializeRef<'_>) -> bool {
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
