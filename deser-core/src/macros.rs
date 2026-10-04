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

/// Forwards the calls of a sink that follow the start of its value (the
/// keys and values, `recover` and `expecting`) to the
/// [`OwnedSink`](crate::de::OwnedSink) in a field.
///
/// The sink implements the start of the value (`atom`, `borrowed_atom`,
/// `map` and `seq`) and `finish`.
macro_rules! forward_to_owned {
    ($field:ident) => {
        fn next_key(
            &mut self,
            state: &mut crate::State,
        ) -> Result<crate::de::SinkHandle<'_, 'de>, crate::Error> {
            self.$field.get_mut().next_key(state)
        }

        fn next_value(
            &mut self,
            state: &mut crate::State,
        ) -> Result<crate::de::SinkHandle<'_, 'de>, crate::Error> {
            self.$field.get_mut().next_value(state)
        }

        fn __private_key_atom(
            &mut self,
            atom: crate::Atom,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            self.$field.get_mut().__private_key_atom(atom, state)
        }

        fn __private_value_atom(
            &mut self,
            atom: crate::Atom,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            self.$field.get_mut().__private_value_atom(atom, state)
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: crate::Atom<'de>,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            self.$field
                .get_mut()
                .__private_borrowed_key_atom(atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: crate::Atom<'de>,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            self.$field
                .get_mut()
                .__private_borrowed_value_atom(atom, state)
        }

        fn value_for_key(
            &mut self,
            key: &str,
            state: &mut crate::State,
        ) -> Result<Option<crate::de::SinkHandle<'_, 'de>>, crate::Error> {
            self.$field.get_mut().value_for_key(key, state)
        }

        fn recover(
            &mut self,
            err: crate::Error,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            self.$field.get_mut().recover(err, state)
        }

        fn expecting(&self) -> alloc::borrow::Cow<'_, str> {
            self.$field.get().expecting()
        }
    };
}

/// Forwards the keys and values of a sink to the sink that a method
/// returns (`fn $sink(&mut self) -> Option<&mut dyn Sink<'de>>`), they are
/// ignored if it returns `None` (the value failed).
///
/// The sink implements the start of the value (`atom`, `borrowed_atom`,
/// `map` and `seq`), `recover`, `finish` and `expecting`.
macro_rules! forward_to_optional {
    ($sink:ident) => {
        fn next_key(
            &mut self,
            state: &mut crate::State,
        ) -> Result<crate::de::SinkHandle<'_, 'de>, crate::Error> {
            match self.$sink() {
                Some(sink) => sink.next_key(state),
                None => Ok(crate::de::SinkHandle::null()),
            }
        }

        fn next_value(
            &mut self,
            state: &mut crate::State,
        ) -> Result<crate::de::SinkHandle<'_, 'de>, crate::Error> {
            match self.$sink() {
                Some(sink) => sink.next_value(state),
                None => Ok(crate::de::SinkHandle::null()),
            }
        }

        fn __private_key_atom(
            &mut self,
            atom: crate::Atom,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            match self.$sink() {
                Some(sink) => sink.__private_key_atom(atom, state),
                None => Ok(()),
            }
        }

        fn __private_value_atom(
            &mut self,
            atom: crate::Atom,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            match self.$sink() {
                Some(sink) => sink.__private_value_atom(atom, state),
                None => Ok(()),
            }
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: crate::Atom<'de>,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            match self.$sink() {
                Some(sink) => sink.__private_borrowed_key_atom(atom, state),
                None => Ok(()),
            }
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: crate::Atom<'de>,
            state: &mut crate::State,
        ) -> Result<(), crate::Error> {
            match self.$sink() {
                Some(sink) => sink.__private_borrowed_value_atom(atom, state),
                None => Ok(()),
            }
        }

        fn value_for_key(
            &mut self,
            key: &str,
            state: &mut crate::State,
        ) -> Result<Option<crate::de::SinkHandle<'_, 'de>>, crate::Error> {
            match self.$sink() {
                Some(sink) => sink.value_for_key(key, state),
                None => Ok(None),
            }
        }
    };
}
