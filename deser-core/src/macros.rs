/// Implements `__private_begin` for types which do not implement `finish`.
///
/// The derive generates the same code.
macro_rules! begin_without_finish {
    () => {
        begin_without_finish!(Self);
    };
    ($ty:ty) => {
        #[inline]
        fn __private_begin<'a>(
            value: &'a $ty,
            state: &mut crate::State,
        ) -> Result<crate::ser::Begin<'a>, crate::Error> {
            let shape = Self::container_shape(value);
            Ok(crate::ser::Begin::emit(
                Self::serialize(value, state)?,
                shape,
                false,
            ))
        }
    };
}

/// Implements the fast path of atoms for types that are deserialized from
/// atoms (with a `Slot`).
macro_rules! slot_atom_into {
    () => {
        slot_atom_into!(Self);
    };
    ($ty:ty) => {
        #[inline]
        fn __private_atom_into(
            out: &mut Option<$ty>,
            atom: Atom,
            state: &mut State,
        ) -> Result<(), Error> {
            Self::deserialize_atom(crate::de::Slot::wrap(out), atom, state)
        }

        #[inline]
        fn __private_borrowed_atom_into(
            out: &mut Option<$ty>,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            Self::deserialize_borrowed_atom(crate::de::Slot::wrap(out), atom, state)
        }
    };
}
