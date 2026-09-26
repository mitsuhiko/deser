//! Deserializes atoms without going through the sink of a slot.
//!
//! These functions are not part of the public API.  They are used by the
//! implementations in this crate and by the derive (through
//! `deser::__derive`).
use crate::State;
#[cfg(feature = "derive")]
use crate::de::Deserialize;
use crate::de::SinkHandle;
use crate::error::Error;
use crate::event::Atom;

/// Deserializes an atom into a slot.
///
/// This is equivalent to what the default implementation of
/// [`Sink::value_atom`](crate::de::Sink::value_atom) does with the sink of the slot.
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
/// [`Sink::borrowed_value_atom`](crate::de::Sink::borrowed_value_atom) does with the sink of the slot.
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
