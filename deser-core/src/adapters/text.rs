//! Adapters that work on the text of values: [`Separated`],
//! [`TrimWhitespace`] and [`SkipBlank`].
use alloc::borrow::Cow;
use alloc::collections::{BTreeSet, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::hash::{BuildHasher, Hash};
use core::marker::PhantomData;
#[cfg(feature = "std")]
use std::collections::HashSet;

use crate::State;
use crate::Text;
use crate::adapters::{DeserializeAs, Same, SerializeAs};
use crate::de::{Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ContainerShape};
use crate::ext::Number;
use crate::ser::{Begin, Chunk, Describe};

/// A sequence that is written as text with a separator, like `a,b,c`.
///
/// This is for lists in places where only text fits, like environment
/// variables and command line arguments.  When deserializing, a string (or
/// a [lexical atom](Atom::Lexical)) is split at the separator `SEP` (a comma
/// by default) and every piece is deserialized with the adapter `A` (by
/// default [`Same`]) as a lexical atom, so numbers and booleans parse.  The
/// empty string is an empty sequence.  Sequences are accepted as they are
/// (their elements are not split), so the same type still reads arrays from
/// JSON or TOML.  Pieces are not trimmed and there is no escaping, see
/// [`TrimWhitespace`] to trim them.
///
/// When serializing, the elements are joined with the separator into a
/// string in all formats.  Elements have to be strings, numbers, booleans
/// or chars.  Values that would not read back are an error: an element that
/// contains the separator and a sequence of a single empty string (which
/// would read back as the empty sequence).
///
/// Supported are `Vec<T>`, `VecDeque<T>`, `BTreeSet<T>` and `HashSet<T>`.
///
/// ```
/// use deser::adapters::{Separated, TrimWhitespace};
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Config {
///     // `a,b,c`
///     #[deser(as = Separated)]
///     hosts: Vec<String>,
///     // `/usr/bin:/bin`
///     #[deser(as = Separated<':'>)]
///     search_path: Vec<String>,
///     // `80, 443`
///     #[deser(as = Separated<',', TrimWhitespace>)]
///     ports: Vec<u16>,
/// }
/// ```
pub struct Separated<const SEP: char = ',', A = Same>(PhantomData<fn() -> A>);

/// Trims whitespace from the start and end of strings.
///
/// Strings and [lexical atoms](Atom::Lexical) are trimmed before they are
/// deserialized with the adapter `A` (by default [`Same`]), so `" 42 "`
/// deserializes into a number.  What is whitespace is defined by
/// [`str::trim`].  Other values are passed on as they are.  Serialization
/// uses the inner adapter, values are not trimmed.
///
/// It's typically combined with [`Separated`] to trim the pieces of a list
/// (`Separated<',', TrimWhitespace>` reads `a, b` as `["a", "b"]`) or used on
/// its own for values that are typed by hand.
///
/// Optionals are `None` for empty values if the type does not accept them
/// (see [`Atom::Lexical`]), which is decided before the value reaches the
/// adapters in the option.  To make blank values `None`, trim outside of
/// the option (`TrimWhitespace<Option<_>>`):
///
/// ```
/// use deser::adapters::TrimWhitespace;
/// use deser::Deserialize;
///
/// #[derive(Deserialize)]
/// pub struct Login {
///     #[deser(as = TrimWhitespace)]
///     username: String,
///     // `" 80 "` is `Some(80)` and `" "` is `None`
///     #[deser(as = TrimWhitespace<Option<_>>)]
///     port: Option<u16>,
/// }
/// ```
pub struct TrimWhitespace<A = Same>(PhantomData<fn() -> A>);

/// Leaves no value for blank strings.
///
/// Strings and [lexical atoms](Atom::Lexical) that are empty or only
/// whitespace (as defined by [`str::trim`]) are skipped: they leave the
/// slot as it is.  Everything else is deserialized with the adapter `A`
/// (by default [`Same`]).  Serialization uses the inner adapter.
///
/// Sequences and collections leave out elements without a value, which
/// makes this an adapter for their elements: `Vec<SkipBlank>` drops the
/// blank elements, in sequences as well as for keys that are given more
/// than once (like `tag=&tag=a` in a query string or the whitespace between
/// elements in XML).  It can be combined with [`TrimWhitespace`] (which
/// trims the other values) and [`Separated`]:
///
/// ```
/// use deser::adapters::{Separated, SkipBlank, TrimWhitespace};
/// use deser::Deserialize;
///
/// #[derive(Deserialize)]
/// pub struct Config {
///     // `a, ,b,` is `["a", "b"]`
///     #[deser(as = Separated<',', SkipBlank<TrimWhitespace>>)]
///     hosts: Vec<String>,
///     // blank values are missing, which is `None`
///     #[deser(as = SkipBlank<Option<_>>)]
///     name: Option<String>,
/// }
/// ```
///
/// For values that are not elements a blank string is a missing value: an
/// optional is `None`, other types report the missing field (unless they
/// have a default).
pub struct SkipBlank<A = Same>(PhantomData<fn() -> A>);

/// What a [`TextSink`] does with the text it receives.
#[derive(Clone, Copy)]
enum TextOp {
    /// Splits the text into a sequence.
    Split(char),
    /// Trims the text.
    Trim,
}

/// Changes the strings and lexical atoms a sink receives.
///
/// Everything else is forwarded to the inner sink.
struct TextSink<'a, 'de> {
    inner: SinkHandle<'a, 'de>,
    op: TextOp,
}

impl<'a, 'de> TextSink<'a, 'de> {
    fn handle(inner: SinkHandle<'a, 'de>, op: TextOp) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(TextSink { inner, op })
    }
}

/// Returns the range of the text without the surrounding whitespace.
fn trimmed_range(text: &str) -> (usize, usize) {
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len().max(start);
    (start, end)
}

/// Replaces the text of a string or lexical atom.
fn with_text<'x>(atom: &Atom<'_>, text: Text<'x>) -> Atom<'x> {
    match atom {
        Atom::Str(_) => Atom::Str(text),
        _ => Atom::Lexical(text),
    }
}

/// Trims a string or lexical atom, other atoms are returned as they are.
fn trim_atom(atom: Atom<'_>) -> Atom<'_> {
    match atom {
        Atom::Str(ref text) | Atom::Lexical(ref text) => {
            let (start, end) = trimmed_range(text);
            if start == 0 && end == text.len() {
                return atom;
            }
            let trimmed = match text.borrowed_str() {
                Some(text) => Text::borrowed(&text[start..end]),
                None => Text::owned(&text[start..end]),
            };
            with_text(&atom, trimmed)
        }
        atom => atom,
    }
}

/// Splits text into the elements of the sequence of a sink.
///
/// Invokes [`Sink::seq`] and passes the pieces on as elements, the driver
/// invokes [`Sink::finish`] as for every atom.  Like the driver does for
/// the elements of sequences, the sink can recover from the error of an
/// element (see [`Sink::recover`]).
fn split_into<'de>(
    sink: &mut SinkHandle<'_, 'de>,
    text: &str,
    sep: char,
    state: &mut State,
) -> Result<(), Error> {
    sink.seq(state)?;
    if !text.is_empty() {
        for piece in text.split(sep) {
            let rv = sink.__private_value_atom(Atom::Lexical(Text::borrowed(piece)), state);
            recover_element(sink, rv, state)?;
        }
    }
    Ok(())
}

/// Lets the sink recover from the error of an element.
#[inline]
fn recover_element(
    sink: &mut SinkHandle<'_, '_>,
    rv: Result<(), Error>,
    state: &mut State,
) -> Result<(), Error> {
    match rv {
        Ok(()) => Ok(()),
        Err(err) if state.discards_errors => Err(err),
        Err(err) => sink.recover(state.attach_error_context(err), state),
    }
}

/// Splits borrowed text into the elements of the sequence of a sink.
///
/// Like [`split_into`] but the pieces are passed on borrowed.
fn split_borrowed_into<'de>(
    sink: &mut SinkHandle<'_, 'de>,
    text: &'de str,
    sep: char,
    state: &mut State,
) -> Result<(), Error> {
    sink.seq(state)?;
    if !text.is_empty() {
        for piece in text.split(sep) {
            let rv =
                sink.__private_borrowed_value_atom(Atom::Lexical(Text::borrowed(piece)), state);
            recover_element(sink, rv, state)?;
        }
    }
    Ok(())
}

impl<'a, 'de> Sink<'de> for TextSink<'a, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match (self.op, atom) {
            (TextOp::Split(sep), Atom::Str(text) | Atom::Lexical(text)) => {
                split_into(&mut self.inner, &text, sep, state)
            }
            (TextOp::Trim, atom) => self.inner.atom(trim_atom(atom), state),
            (_, atom) => self.inner.atom(atom, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match (self.op, atom) {
            (TextOp::Split(sep), Atom::Str(ref text) | Atom::Lexical(ref text))
                if text.is_borrowed() =>
            {
                let text = text.borrowed_str().unwrap_or_default();
                split_borrowed_into(&mut self.inner, text, sep, state)
            }
            (TextOp::Split(sep), Atom::Str(text) | Atom::Lexical(text)) => {
                split_into(&mut self.inner, &text, sep, state)
            }
            (TextOp::Trim, atom) => self.inner.borrowed_atom(trim_atom(atom), state),
            (_, atom) => self.inner.borrowed_atom(atom, state),
        }
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.inner.next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.inner.next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.inner.__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.inner.__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.inner.value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.inner.recover(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.finish(state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.inner.expecting()
    }
}

/// Returns `true` if the atom is a blank string.
#[inline]
fn is_blank(atom: &Atom) -> bool {
    matches!(atom, Atom::Str(text) | Atom::Lexical(text) if text.trim().is_empty())
}

/// The sink of [`SkipBlank`].
///
/// The sink of the value is only created once a value arrives that is not
/// blank, as creating it can already set the slot (like it does for
/// optionals).  A blank string leaves a null sink.
enum SkipBlankSink<'a, 'de, T, A> {
    Pending(&'a mut Option<T>, PhantomData<fn() -> A>),
    Active(SinkHandle<'a, 'de>),
}

impl<'a, 'de, T, A: DeserializeAs<'de, T>> SkipBlankSink<'a, 'de, T, A> {
    /// Returns the sink of the value, creates it if needed.
    fn active(&mut self) -> &mut SinkHandle<'a, 'de> {
        if let SkipBlankSink::Pending(..) = self
            && let SkipBlankSink::Pending(out, _) =
                core::mem::replace(self, SkipBlankSink::Active(SinkHandle::null()))
        {
            *self = SkipBlankSink::Active(A::deserialize_into_as(out));
        }
        match self {
            SkipBlankSink::Active(sink) => sink,
            SkipBlankSink::Pending(..) => unreachable!(),
        }
    }

    /// Skips the atom if it's blank and the first event.
    fn skip(&mut self, atom: &Atom) -> bool {
        if matches!(self, SkipBlankSink::Pending(..)) && is_blank(atom) {
            *self = SkipBlankSink::Active(SinkHandle::null());
            return true;
        }
        false
    }
}

impl<'a, 'de, T: Send, A: DeserializeAs<'de, T>> Sink<'de> for SkipBlankSink<'a, 'de, T, A> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if self.skip(&atom) {
            return Ok(());
        }
        self.active().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        if self.skip(&atom) {
            return Ok(());
        }
        self.active().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.active().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.active().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.active().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.active().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.active().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.active().__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.active().__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.active().__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.active().value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.active().recover(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.active().finish(state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        match self {
            SkipBlankSink::Active(sink) => sink.expecting(),
            // the sink of the value does not exist yet
            SkipBlankSink::Pending(..) => {
                let mut slot = None;
                Cow::Owned(A::deserialize_into_as(&mut slot).expecting().into_owned())
            }
        }
    }
}

impl<'de, T: Send, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for SkipBlank<A> {
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(SkipBlankSink::<T, A>::Pending(out, PhantomData))
    }

    fn initial_value_as() -> Option<T> {
        A::initial_value_as()
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        if is_blank(&atom) {
            return Ok(());
        }
        A::__private_atom_into_as(out, atom, state)
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        if is_blank(&atom) {
            return Ok(());
        }
        A::__private_borrowed_atom_into_as(out, atom, state)
    }

    fn __private_is_bytes_as() -> bool {
        A::__private_is_bytes_as()
    }

    fn __private_vec_from_bytes_as(bytes: Vec<u8>) -> Option<Vec<T>> {
        A::__private_vec_from_bytes_as(bytes)
    }

    fn __private_array_from_bytes_as<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
        A::__private_array_from_bytes_as::<N>(bytes)
    }
}

impl<'de, T, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for TrimWhitespace<A> {
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        TextSink::handle(A::deserialize_into_as(out), TextOp::Trim)
    }

    fn initial_value_as() -> Option<T> {
        A::initial_value_as()
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        A::__private_atom_into_as(out, trim_atom(atom), state)
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        A::__private_borrowed_atom_into_as(out, trim_atom(atom), state)
    }

    fn __private_is_bytes_as() -> bool {
        A::__private_is_bytes_as()
    }

    fn __private_vec_from_bytes_as(bytes: Vec<u8>) -> Option<Vec<T>> {
        A::__private_vec_from_bytes_as(bytes)
    }

    fn __private_array_from_bytes_as<const N: usize>(bytes: &[u8]) -> Option<[T; N]> {
        A::__private_array_from_bytes_as::<N>(bytes)
    }
}

impl<T: ?Sized, A: SerializeAs<T>> SerializeAs<T> for TrimWhitespace<A> {
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
        A::serialize_as(value, state)
    }

    fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
        A::finish_as(value, state)
    }

    fn is_optional_as(value: &T) -> bool {
        A::is_optional_as(value)
    }

    fn container_shape_as(value: &T) -> ContainerShape {
        A::container_shape_as(value)
    }

    fn describe_as(value: &T, d: &mut dyn Describe) {
        A::describe_as(value, d)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        A::__private_begin_as(value, state)
    }

    fn __private_slice_as_bytes_as(val: &[T]) -> Option<Cow<'_, [u8]>>
    where
        T: Sized,
    {
        A::__private_slice_as_bytes_as(val)
    }
}

impl<T: ?Sized, A: SerializeAs<T>> SerializeAs<T> for SkipBlank<A> {
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
        A::serialize_as(value, state)
    }

    fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
        A::finish_as(value, state)
    }

    fn is_optional_as(value: &T) -> bool {
        A::is_optional_as(value)
    }

    fn container_shape_as(value: &T) -> ContainerShape {
        A::container_shape_as(value)
    }

    fn describe_as(value: &T, d: &mut dyn Describe) {
        A::describe_as(value, d)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        A::__private_begin_as(value, state)
    }

    fn __private_slice_as_bytes_as(val: &[T]) -> Option<Cow<'_, [u8]>>
    where
        T: Sized,
    {
        A::__private_slice_as_bytes_as(val)
    }
}

#[cold]
fn unsupported_element(what: &str) -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        format!(
            "cannot join {}, elements must be strings, numbers, booleans or chars",
            what
        ),
    )
}

/// Returns the text of an element.
fn atom_text<'a>(atom: &'a Atom<'_>) -> Result<Cow<'a, str>, Error> {
    Ok(match *atom {
        Atom::Bool(value) => Cow::Borrowed(if value { "true" } else { "false" }),
        Atom::Str(ref value) | Atom::Lexical(ref value) => Cow::Borrowed(value),
        Atom::Char(value) => Cow::Owned(value.to_string()),
        Atom::U64(value) => Cow::Owned(value.to_string()),
        Atom::I64(value) => Cow::Owned(value.to_string()),
        Atom::F32(value) => Cow::Owned(value.to_string()),
        Atom::F64(value) => Cow::Owned(value.to_string()),
        Atom::Ext(ref ext) => {
            if let Some(number) = ext.downcast_value_ref::<Number>() {
                // numbers keep their text
                Cow::Owned(number.as_str().to_string())
            } else if let Some(value) = ext.downcast_ref::<u128>() {
                Cow::Owned(value.to_string())
            } else if let Some(value) = ext.downcast_ref::<i128>() {
                Cow::Owned(value.to_string())
            } else {
                match ext.fallback() {
                    Atom::Ext(_) => return Err(unsupported_element(ext.name())),
                    fallback => Cow::Owned(atom_text(&fallback)?.into_owned()),
                }
            }
        }
        ref atom => return Err(unsupported_element(atom.name())),
    })
}

/// Appends the text of a serialized element.
fn push_chunk(chunk: Chunk<'_>, state: &mut State, out: &mut String) -> Result<(), Error> {
    match chunk {
        Chunk::Atom(ref atom) => out.push_str(&atom_text(atom)?),
        Chunk::Forward(handle) => {
            push_chunk(handle.serialize(state)?, state, out)?;
            handle.finish(state)?;
        }
        Chunk::Struct(_) | Chunk::Map(_) => return Err(unsupported_element("map")),
        Chunk::Seq(_) => return Err(unsupported_element("sequence")),
    }
    Ok(())
}

/// Joins the elements of a sequence into a string.
fn join<'v, T: 'v, A: SerializeAs<T>>(
    values: impl Iterator<Item = &'v T>,
    sep: char,
    state: &mut State,
) -> Result<String, Error> {
    let mut out = String::new();
    let mut count = 0;
    for value in values {
        if count > 0 {
            out.push(sep);
        }
        count += 1;
        let start = out.len();
        push_chunk(A::serialize_as(value, state)?, state, &mut out)?;
        A::finish_as(value, state)?;
        if out[start..].contains(sep) {
            return Err(Error::new(
                ErrorKind::Unexpected,
                format!(
                    "cannot join {:?}, it contains the separator {:?}",
                    &out[start..],
                    sep
                ),
            ));
        }
    }
    if count == 1 && out.is_empty() {
        return Err(Error::new(
            ErrorKind::Unexpected,
            "cannot join a single empty string, it would read back as no elements",
        ));
    }
    Ok(out)
}

/// Implements `Separated` for sequences.
macro_rules! separated_impls {
    ($([$($bound:tt)*] [$($ser_bound:tt)*] $target:ty => $adapter:ty;)*) => {
        $(
            impl<'de, $($bound)*, A: DeserializeAs<'de, T>, const SEP: char>
                DeserializeAs<'de, $target> for Separated<SEP, A>
            {
                fn deserialize_into_as(out: &mut Option<$target>) -> SinkHandle<'_, 'de> {
                    TextSink::handle(
                        <$adapter as DeserializeAs<'de, $target>>::deserialize_into_as(out),
                        TextOp::Split(SEP),
                    )
                }
            }

            impl<$($ser_bound)*, A: SerializeAs<T>, const SEP: char> SerializeAs<$target>
                for Separated<SEP, A>
            {
                fn serialize_as<'a>(
                    value: &'a $target,
                    state: &mut State,
                ) -> Result<Chunk<'a>, Error> {
                    let text = join::<T, A>(value.iter(), SEP, state)?;
                    Ok(Chunk::Atom(Atom::Str(Text::owned(text))))
                }

                #[inline]
                fn __private_begin_as<'a>(
                    value: &'a $target,
                    state: &mut State,
                ) -> Result<Begin<'a>, Error> {
                    Ok(Begin::chunk(
                        Self::serialize_as(value, state)?,
                        ContainerShape::new(),
                        false,
                    ))
                }
            }
        )*
    };
}

separated_impls! {
    [T: Send] [T] Vec<T> => Vec<A>;
    [T: Send] [T] VecDeque<T> => VecDeque<A>;
    [T: Ord + Send] [T] BTreeSet<T> => BTreeSet<A>;
}

#[cfg(feature = "std")]
separated_impls! {
    [T: Hash + Eq + Send, H: BuildHasher + Default + Send] [T, H] HashSet<T, H> => HashSet<A>;
}
