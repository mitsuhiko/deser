//! The slots of values that are deserialized from atoms.
use alloc::borrow::Cow;
use alloc::string::String;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::Error;
use crate::event::Atom;
use crate::state::State;

/// The slot of a value that is deserialized from an atom.
///
/// Values that are deserialized from atoms (like numbers or strings) do not
/// need a sink with state: the sink is the slot itself.  A `Slot<T, A>` is
/// the slot of a `T` (a transparent wrapper around the `Option<T>`) which
/// is a [`Sink`] that passes the atoms it receives to
/// [`A::deserialize_atom`](Deserialize::deserialize_atom).  This is the
/// sink of the default implementation of
/// [`Deserialize::deserialize_into`], so values that are deserialized from
/// atoms only implement [`deserialize_atom`](Deserialize::deserialize_atom)
/// (and [`expecting`](Deserialize::expecting), which defaults to the name
/// of the type).  `A` is the type that implements [`Deserialize`], which is
/// the value itself unless it's an adapter (see
/// [`adapters`](crate::adapters)).
///
/// The slot dereferences to the `Option<T>`, [`set`](Self::set) places a
/// value in it.  Atoms which are not accepted are passed to
/// [`default_atom`](crate::de::default_atom) with the slot as sink:
///
/// ```
/// use std::borrow::Cow;
/// use deser::de::{DeserializeDriver, Slot, default_atom};
/// use deser::{Atom, Deserialize, Error, State};
///
/// struct Celsius(f64);
///
/// impl<'de> Deserialize<'de> for Celsius {
///     fn deserialize_atom(
///         slot: &mut Slot<Self>,
///         atom: Atom,
///         state: &mut State,
///     ) -> Result<(), Error> {
///         match atom {
///             Atom::F64(value) => {
///                 slot.set(Celsius(value));
///                 Ok(())
///             }
///             other => default_atom(slot, other, state),
///         }
///     }
///
///     fn expecting() -> Cow<'static, str> {
///         Cow::Borrowed("temperature")
///     }
/// }
///
/// let mut out = None::<Celsius>;
/// DeserializeDriver::new(&mut out).emit(21.5).unwrap();
/// assert_eq!(out.unwrap().0, 21.5);
/// ```
#[repr(transparent)]
pub struct Slot<T, A = T> {
    value: Option<T>,
    // `A` is only used for its functions
    _marker: PhantomData<fn() -> A>,
}

impl<T, A> Slot<T, A> {
    /// Wraps an `Option<T>` in a slot.
    ///
    /// This is a cast, the slot is the `Option<T>`.
    #[inline(always)]
    pub fn wrap(out: &mut Option<T>) -> &mut Slot<T, A> {
        // SAFETY: the slot is a transparent wrapper around the option
        unsafe { &mut *(out as *mut Option<T> as *mut Slot<T, A>) }
    }

    /// Places a value in the slot.
    #[inline(always)]
    pub fn set(&mut self, value: T) {
        self.value = Some(value);
    }
}

impl<'de, T: Send, A: Deserialize<'de, T>> Slot<T, A> {
    /// Returns a handle to the slot of the `Option<T>`.
    ///
    /// Unlike [`SinkHandle::to`] this does not require `A` to outlive the
    /// handle: `A` is only used for its functions, which cannot hold
    /// borrowed data (see `SinkHandle::arena_unbounded`).
    #[inline(always)]
    pub(crate) fn handle<'a>(out: &'a mut Option<T>) -> SinkHandle<'a, 'de> {
        // the slot outlives this function (like every type parameter)
        let sink: *mut (dyn Sink<'de> + '_) = out as *mut Option<T> as *mut Slot<T, A>;
        // SAFETY: the slot is a transparent wrapper around the option which
        // is borrowed for the lifetime of the handle.  What `A` stands for
        // is not used, the sink only invokes the functions of `A`.
        SinkHandle::to(unsafe {
            &mut *core::mem::transmute::<*mut (dyn Sink<'de> + '_), *mut (dyn Sink<'de> + 'a)>(sink)
        })
    }
}

impl<T, A> Deref for Slot<T, A> {
    type Target = Option<T>;

    #[inline(always)]
    fn deref(&self) -> &Option<T> {
        &self.value
    }
}

impl<T, A> DerefMut for Slot<T, A> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Option<T> {
        &mut self.value
    }
}

impl<'de, T: Send, A: Deserialize<'de, T>> Sink<'de> for Slot<T, A> {
    #[inline]
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        A::deserialize_atom(self, atom, state)
    }

    #[inline]
    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        A::deserialize_borrowed_atom(self, atom, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        A::expecting()
    }
}

/// Removes the module paths from a type name.
///
/// This turns `alloc::vec::Vec<my_crate::Point>` into `Vec<Point>`.
pub(crate) fn short_type_name(name: &'static str) -> Cow<'static, str> {
    if !name.contains("::") {
        return Cow::Borrowed(name);
    }
    let mut rv = String::with_capacity(name.len());
    // where the identifier that is written last starts
    let mut ident_start = 0;
    let mut rest = name;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("::") {
            // the identifier was a path segment
            rv.truncate(ident_start);
            rest = after;
            continue;
        }
        rv.push(c);
        if !(c.is_alphanumeric() || c == '_') {
            ident_start = rv.len();
        }
        rest = &rest[c.len_utf8()..];
    }
    Cow::Owned(rv)
}

#[cfg(test)]
mod tests {
    use super::short_type_name;

    #[test]
    fn test_short_type_name() {
        assert_eq!(short_type_name("u32"), "u32");
        assert_eq!(short_type_name("my_crate::Point"), "Point");
        assert_eq!(
            short_type_name("alloc::vec::Vec<my_crate::geo::Point>"),
            "Vec<Point>"
        );
        assert_eq!(
            short_type_name("(u8, &alloc::string::String, [a::B; 2])"),
            "(u8, &String, [B; 2])"
        );
        assert_eq!(
            short_type_name(
                "core::option::Option<std::collections::hash::map::HashMap<a::K, b::V>>"
            ),
            "Option<HashMap<K, V>>"
        );
    }
}
