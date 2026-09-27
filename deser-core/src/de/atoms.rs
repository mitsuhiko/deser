//! Deserializes atoms without going through the sink of a slot.
//!
//! These functions are not part of the public API.  They are used by the
//! implementations in this crate and by the derive (through
//! `deser::__derive`).
use crate::State;
#[cfg(feature = "derive")]
use crate::de::Deserialize;
use crate::de::{Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;

/// Creates the sink for a field of a derived struct.
///
/// This is not inlined so that the code to create the sink exists once per
/// type instead of once per field.  It's only used for values that are not
/// atoms.
#[cfg(feature = "derive")]
#[inline(never)]
pub fn field_sink<'a, 'de, T: Deserialize<'de>>(slot: &'a mut Option<T>) -> SinkHandle<'a, 'de> {
    T::deserialize_into(slot)
}

/// Creates the sink that updates a field of a derived struct.
///
/// Like [`field_sink`] this is not inlined so that it exists once per type.
#[cfg(feature = "derive")]
#[inline(never)]
pub fn field_update<'a, 'de, T: Deserialize<'de>>(value: &'a mut T) -> SinkHandle<'a, 'de> {
    T::deserialize_update(value)
}

/// Creates the sink for a field of a derived struct with an adapter.
#[cfg(feature = "derive")]
#[inline(never)]
pub fn field_sink_as<'a, 'de, A, T>(slot: &'a mut Option<T>) -> SinkHandle<'a, 'de>
where
    A: crate::adapters::DeserializeAs<'de, T>,
    T: 'a,
{
    A::deserialize_into_as(slot)
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
    sink.atom(atom, state)?;
    sink.finish(state)
}

/// Deserializes a borrowed atom into a sink handle.
#[inline(never)]
pub fn borrowed_atom_into_handle<'de>(
    mut sink: SinkHandle<'_, 'de>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error> {
    sink.borrowed_atom(atom, state)?;
    sink.finish(state)
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

/// The default of `Sink::unexpected_atom`.
///
/// Extension values are lowered to their fallback, [`Atom::F32`] is widened
/// into an [`Atom::F64`] and [`Atom::Lexical`] is passed on as
/// [`Atom::Str`].  All other atoms are an error.
#[inline(never)]
pub(crate) fn default_unexpected_atom(
    sink: &mut dyn Sink<'_>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    let atom = match atom {
        Atom::F32(value) => return sink.atom(Atom::F64(f64::from(value)), state),
        Atom::Lexical(value) => return sink.atom(Atom::Str(value), state),
        atom => atom,
    };
    if let Atom::Ext(ref ext) = atom {
        let fallback = ext.fallback();
        debug_assert!(
            !matches!(fallback, Atom::Ext(_)),
            "the fallback of an extension value must not be an extension value"
        );
        if !matches!(fallback, Atom::Ext(_)) {
            return sink.atom(fallback, state);
        }
    }
    Err(atom.unexpected_error(&sink.expecting()))
}

/// The default of `Sink::map` and `Sink::seq`.
#[cold]
#[inline(never)]
pub(crate) fn default_container(sink: &mut dyn Sink<'_>, got: &str) -> Result<(), Error> {
    Err(Error::new(
        ErrorKind::Unexpected,
        format!("unexpected {}, expected {}", got, sink.expecting()),
    ))
}
