//! Deserializes atoms without going through the sink of a slot.
//!
//! These functions are not part of the public API.  They are used by the
//! implementations in this crate and by the derive (through
//! `deser::__derive`).
use crate::State;
use crate::Text;
#[cfg(feature = "derive")]
use crate::de::Deserialize;
use crate::de::lexical::ContentKey;
#[cfg(feature = "derive")]
use crate::de::lexical::is_empty_null;
use crate::de::{Sink, SinkHandle};
use crate::error::{Error, ErrorKind, discarded_error};
use crate::event::{Atom, Implicit};
use alloc::format;

/// Creates the sink that updates a field of a derived struct.
///
/// This is not inlined so that it exists once per type (updates are not
/// performance critical).
#[cfg(feature = "derive")]
#[inline(never)]
pub fn field_update<'a, 'de, T: Deserialize<'de>>(
    value: &'a mut T,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    T::deserialize_update(value, state)
}

/// Deserializes an atom into a slot.
///
/// This is equivalent to what the default implementation of
/// `Sink::__private_value_atom` does with the sink of the slot.
#[cfg(feature = "derive")]
#[inline]
pub fn atom_into<'de, T: Deserialize<'de>>(
    slot: &mut Option<T>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    T::__private_atom_into(slot, atom, state)
}

/// Deserializes a borrowed atom into a slot.
///
/// This is equivalent to what the default implementation of
/// `Sink::__private_borrowed_value_atom` does with the sink of the slot.
#[cfg(feature = "derive")]
#[inline]
pub fn borrowed_atom_into<'de, T: Deserialize<'de>>(
    slot: &mut Option<T>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error> {
    T::__private_borrowed_atom_into(slot, atom, state)
}

/// Checks that an atom is the value of a unit struct.
///
/// Unit structs are null (and empty lexical atoms if empty text is a
/// missing value, like for `()`, see
/// [`LexicalRules`](crate::de::LexicalRules)).  Other atoms are rejected
/// with `expecting` as expected type.
#[cfg(feature = "derive")]
pub fn unit_struct(atom: &Atom<'_>, expecting: &str, state: &State) -> Result<(), Error> {
    match atom {
        Atom::Null => Ok(()),
        Atom::Lexical(value) if is_empty_null(value, state) => Ok(()),
        Atom::Implicit(value) if value.value() == crate::ImplicitValue::Null => Ok(()),
        Atom::Ext(ext) => match ext.fallback() {
            Atom::Ext(_) => Err(atom.unexpected_error(expecting)),
            fallback => unit_struct(&fallback, expecting, state),
        },
        _ => Err(atom.unexpected_error(expecting)),
    }
}

/// Deserializes an atom into a sink handle.
///
/// This is intentionally not inlined as it's used by the default
/// implementations of the sink methods which exist for every sink.
#[inline(never)]
pub fn atom_into_handle(
    mut sink: SinkHandle<'_, '_>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    let rv = sink.atom(atom, state).and_then(|()| sink.finish(state));
    sink.release(state);
    rv
}

/// Deserializes a borrowed atom into a sink handle.
#[inline(never)]
pub fn borrowed_atom_into_handle<'de>(
    mut sink: SinkHandle<'_, 'de>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error> {
    let rv = sink
        .borrowed_atom(atom, state)
        .and_then(|()| sink.finish(state));
    sink.release(state);
    rv
}

// The following functions implement the default methods of `Sink`.  The
// default methods exist for every sink type, so they only forward to these
// functions which exist once.

/// The default of `Sink::__private_key_atom`.
#[inline(never)]
pub(crate) fn default_key_atom(
    sink: &mut dyn Sink<'_>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    atom_into_handle(sink.next_key(state)?, atom, state)
}

/// The default of `Sink::__private_value_atom`.
#[inline(never)]
pub(crate) fn default_value_atom(
    sink: &mut dyn Sink<'_>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    atom_into_handle(sink.next_value(state)?, atom, state)
}

/// The default of `Sink::__private_borrowed_key_atom`.
#[inline(never)]
pub(crate) fn default_borrowed_key_atom<'de>(
    sink: &mut dyn Sink<'de>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error> {
    borrowed_atom_into_handle(sink.next_key(state)?, atom, state)
}

/// The default of `Sink::__private_borrowed_value_atom`.
#[inline(never)]
pub(crate) fn default_borrowed_value_atom<'de>(
    sink: &mut dyn Sink<'de>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error> {
    borrowed_atom_into_handle(sink.next_value(state)?, atom, state)
}

/// The default handling of atoms, which is what [`Sink::atom`] does by
/// default.
///
/// Sinks pass the atoms they do not accept to this (and values the atoms
/// their [`deserialize_atom`](crate::de::Deserialize::deserialize_atom) does
/// not accept, with the slot as sink).  Some atoms are passed on to
/// [`Sink::atom`] of the sink again in another form:
///
/// * [`Atom::Ext`] values are lowered into the core data model with their
///   [`fallback`](crate::ext::ExtValue::fallback) (the input of raw values
///   is parsed into the sink).
/// * [`Atom::F32`] is widened into an [`Atom::F64`], so sinks that accept
///   floats only need to handle `F64`.
/// * [`Atom::Lexical`] is passed on as [`Atom::Str`], so sinks that accept
///   strings accept lexical atoms too.
/// * [`Atom::Implicit`] is passed on as its value and, if that is rejected,
///   as its text (see [`Implicit`]).
///
/// For all other atoms an error is returned that is based on
/// [`Sink::expecting`] of the sink, which is
/// [`Deserialize::expecting`] for a
/// [`Slot`](crate::de::Slot).
///
/// ```
/// use deser::de::{Deserialize, Slot, default_atom};
/// use deser::{Atom, Error, State};
///
/// struct MyBool(bool);
///
/// impl<'de> Deserialize<'de> for MyBool {
///     fn deserialize_atom(
///         slot: &mut Slot<Self>,
///         atom: Atom,
///         state: &mut State,
///     ) -> Result<(), Error> {
///         match atom {
///             Atom::Bool(value) => {
///                 slot.set(MyBool(value));
///                 Ok(())
///             }
///             other => default_atom(slot, other, state),
///         }
///     }
/// }
///
/// // a lexical atom is passed on as string, which is rejected
/// let mut out = None::<MyBool>;
/// let mut driver = deser::de::DeserializeDriver::new(&mut out);
/// let err = driver.emit(Atom::Lexical("true".into())).unwrap_err();
/// assert_eq!(err.message(), "unexpected string, expected MyBool");
/// ```
#[inline(never)]
pub fn default_atom(sink: &mut dyn Sink<'_>, atom: Atom, state: &mut State) -> Result<(), Error> {
    let atom = match atom {
        Atom::F32(value) => return sink.atom(Atom::F64(f64::from(value)), state),
        Atom::Lexical(value) => {
            if let Some(key) = ContentKey::of(state) {
                return lexical_into_map(sink, value, key, state);
            }
            return sink.atom(Atom::Str(value), state);
        }
        Atom::Implicit(value) => return implicit_into(sink, value, state),
        atom => atom,
    };
    if let Atom::Ext(ref ext) = atom {
        // the input of a raw value is parsed into the sink
        if let Some(input) = ext.downcast_value_ref::<crate::ext::RawInput>() {
            return crate::ext::raw::parse_into(input, sink, state);
        }
        let fallback = ext.fallback();
        debug_assert!(
            !matches!(fallback, Atom::Ext(_)),
            "the fallback of an extension value must not be an extension value"
        );
        if !matches!(fallback, Atom::Ext(_)) {
            return sink.atom(fallback, state);
        }
    }
    if state.discards_errors {
        return Err(discarded_error(ErrorKind::Unexpected));
    }
    Err(atom.unexpected_error(&sink.expecting()))
}

/// Delivers text that a sink rejected as string or as map with the text
/// under the key of the content (see [`ContentKey`]).
///
/// If the sink rejects both, the error of the string is returned.
#[cold]
#[inline(never)]
fn lexical_into_map(
    sink: &mut dyn Sink<'_>,
    text: Text<'_>,
    key: &'static str,
    state: &mut State,
) -> Result<(), Error> {
    let err = match sink.atom(Atom::Str(text.clone()), state) {
        Err(err) if err.kind() == ErrorKind::Unexpected => err,
        rv => return rv,
    };
    if sink.map(state).is_err() {
        return Err(err);
    }
    // empty text is no content
    if !text.is_empty() {
        sink.__private_key_atom(Atom::Lexical(Text::borrowed(key)), state)?;
        sink.__private_value_atom(Atom::Lexical(text), state)?;
    }
    Ok(())
}

/// Delivers an implicit atom as its value or its text.
///
/// The text is only tried if the value is rejected for its type, errors of
/// the value (like an integer that is out of range) are passed on.  If the
/// text is rejected too, the error of the value is returned.
fn implicit_into(sink: &mut dyn Sink<'_>, value: Implicit, state: &mut State) -> Result<(), Error> {
    let (text, value) = value.into_parts();
    match sink.atom(value.to_atom(), state) {
        Err(err) if err.kind() == ErrorKind::Unexpected => {
            sink.atom(Atom::Str(text), state).map_err(|_| err)
        }
        rv => rv,
    }
}

/// The default of `Sink::map` and `Sink::seq`.
#[cold]
#[inline(never)]
pub(crate) fn default_container(
    sink: &mut dyn Sink<'_>,
    got: &str,
    state: &State,
) -> Result<(), Error> {
    if state.discards_errors {
        return Err(discarded_error(ErrorKind::Unexpected));
    }
    Err(Error::new(
        ErrorKind::Unexpected,
        format!("unexpected {}, expected {}", got, sink.expecting()),
    ))
}
