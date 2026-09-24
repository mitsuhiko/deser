use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;
use std::hash::Hash;
use std::mem::{take, MaybeUninit};

use crate::de::{
    atom_into, is_null_atom, Deserialize, DeserializerState, OwnedSink, Sink, SinkHandle,
};
use crate::descriptors::{Descriptor, NamedDescriptor, UnorderedNamedDescriptor};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;

make_slot_wrapper!(SlotWrapper);

macro_rules! deserialize {
    ($ty:ty) => {
        impl Deserialize for $ty {
            fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
                SlotWrapper::make_handle(out)
            }

            __slot_wrapper_atom_into!();
        }
    };
}

/// Implements `__private_atom_into` for types using a slot wrapper.
macro_rules! __slot_wrapper_atom_into {
    () => {
        #[inline]
        fn __private_atom_into(
            out: &mut Option<Self>,
            atom: Atom,
            state: &mut DeserializerState,
        ) -> Result<(), Error> {
            let sink = SlotWrapper::wrap(out);
            sink.atom(atom, state)?;
            sink.finish(state)
        }
    };
}

impl Sink for SlotWrapper<()> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "null" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Null => {
                **self = Some(());
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(());

impl Sink for SlotWrapper<bool> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bool" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Bool(value) => {
                **self = Some(value);
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(bool);

impl Sink for SlotWrapper<String> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "string" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                **self = Some(value.into_owned());
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(String);

macro_rules! int_sink {
    ($ty:ty) => {
        impl Sink for SlotWrapper<$ty> {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor {
                    name: stringify!($ty),
                };
                &DESCRIPTOR
            }

            #[allow(clippy::useless_conversion)]
            fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                let value = match atom {
                    Atom::U64(value) => <$ty>::try_from(value).ok(),
                    Atom::I64(value) => <$ty>::try_from(value).ok(),
                    Atom::Ext(ref ext) if ext.is::<u128>() => {
                        <$ty>::try_from(*ext.downcast_ref::<u128>().unwrap()).ok()
                    }
                    Atom::Ext(ref ext) if ext.is::<i128>() => {
                        <$ty>::try_from(*ext.downcast_ref::<i128>().unwrap()).ok()
                    }
                    Atom::Str(ref value) if state.is_map_key() => match value.parse::<$ty>() {
                        Ok(value) => Some(value),
                        Err(_) => return Err(atom.unexpected_error(&self.expecting())),
                    },
                    other => return self.unexpected_atom(other, state),
                };
                match value {
                    Some(value) => {
                        **self = Some(value);
                        Ok(())
                    }
                    None => Err(Error::new(
                        ErrorKind::OutOfRange,
                        "value out of range for type",
                    )),
                }
            }
        }
    };
}

int_sink!(u8);

impl Deserialize for u8 {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SlotWrapper::make_handle(out)
    }

    __slot_wrapper_atom_into!();

    fn __private_is_bytes() -> bool {
        true
    }

    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<u8>> {
        Some(bytes)
    }

    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
        bytes.try_into().ok()
    }
}

int_sink!(u16);
deserialize!(u16);
int_sink!(u32);
deserialize!(u32);
int_sink!(u64);
deserialize!(u64);
int_sink!(i8);
deserialize!(i8);
int_sink!(i16);
deserialize!(i16);
int_sink!(i32);
deserialize!(i32);
int_sink!(i64);
deserialize!(i64);
int_sink!(isize);
deserialize!(isize);
int_sink!(usize);
deserialize!(usize);
int_sink!(u128);
deserialize!(u128);
int_sink!(i128);
deserialize!(i128);

impl Sink for SlotWrapper<char> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "char" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Char(value) => {
                **self = Some(value);
                Ok(())
            }
            Atom::Str(ref s) => {
                let mut chars = s.chars();
                if let Some(first_char) = chars.next() {
                    if chars.next().is_none() {
                        **self = Some(first_char);
                        return Ok(());
                    }
                }
                Err(atom.unexpected_error(&self.expecting()))
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(char);

macro_rules! float_sink {
    ($ty:ty) => {
        impl Sink for SlotWrapper<$ty> {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor {
                    name: stringify!($ty),
                };
                &DESCRIPTOR
            }

            fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                match atom {
                    Atom::U64(value) => {
                        **self = Some(value as $ty);
                        Ok(())
                    }
                    Atom::I64(value) => {
                        **self = Some(value as $ty);
                        Ok(())
                    }
                    Atom::F64(value) => {
                        **self = Some(value as $ty);
                        Ok(())
                    }
                    Atom::Ext(ref ext) if ext.is::<u128>() => {
                        **self = Some(*ext.downcast_ref::<u128>().unwrap() as $ty);
                        Ok(())
                    }
                    Atom::Ext(ref ext) if ext.is::<i128>() => {
                        **self = Some(*ext.downcast_ref::<i128>().unwrap() as $ty);
                        Ok(())
                    }
                    other => self.unexpected_atom(other, state),
                }
            }
        }
    };
}

float_sink!(f32);
deserialize!(f32);

float_sink!(f64);
deserialize!(f64);

impl<T: Deserialize> Deserialize for Vec<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct VecSink<'a, T> {
            slot: &'a mut Option<Vec<T>>,
            vec: Vec<T>,
            element: Option<T>,
            is_seq: bool,
        }

        impl<'a, T: 'a> VecSink<'a, T> {
            fn flush(&mut self) {
                if let Some(element) = self.element.take() {
                    self.vec.push(element);
                }
            }
        }

        impl<'a, T: Deserialize> Sink for VecSink<'a, T> {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "vec" };
                static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bytes" };
                if T::__private_is_bytes() {
                    &BYTES_DESCRIPTOR
                } else {
                    &SLICE_DESCRIPTOR
                }
            }

            fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                match atom {
                    Atom::Bytes(value) => match T::__private_vec_from_bytes(value.into_owned()) {
                        Some(vec) => {
                            *self.slot = Some(vec);
                            Ok(())
                        }
                        None => Err(Error::new(
                            ErrorKind::Unexpected,
                            format!("unexpected bytes, expected {}", self.expecting()),
                        )),
                    },
                    other => self.unexpected_atom(other, state),
                }
            }

            fn seq(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                self.is_seq = true;
                Ok(())
            }

            fn next_value(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.element))
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                self.flush();
                atom_into(&mut self.element, atom, state)
            }

            fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                if self.is_seq {
                    self.flush();
                    *self.slot = Some(take(&mut self.vec));
                }
                Ok(())
            }
        }

        SinkHandle::boxed(VecSink {
            slot: out,
            vec: Vec::new(),
            element: None,
            is_seq: false,
        })
    }
}

impl<K, V> Deserialize for BTreeMap<K, V>
where
    K: Ord + Deserialize,
    V: Deserialize,
{
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct MapSink<'a, K: 'a, V: 'a> {
            slot: &'a mut Option<BTreeMap<K, V>>,
            map: BTreeMap<K, V>,
            key: Option<K>,
            value: Option<V>,
        }

        impl<'a, K, V> MapSink<'a, K, V>
        where
            K: Ord,
        {
            fn flush(&mut self) {
                if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                    self.map.insert(key, value);
                }
            }
        }

        impl<'a, K, V> Sink for MapSink<'a, K, V>
        where
            K: Ord + Deserialize,
            V: Deserialize,
        {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeMap" };
                &DESCRIPTOR
            }

            fn map(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_key(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.key))
            }

            fn next_value(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                Ok(Deserialize::deserialize_into(&mut self.value))
            }

            fn key_atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                self.flush();
                atom_into(&mut self.key, atom, state)
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                atom_into(&mut self.value, atom, state)
            }

            fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                self.flush();
                *self.slot = Some(take(&mut self.map));
                Ok(())
            }
        }

        SinkHandle::boxed(MapSink {
            slot: out,
            map: BTreeMap::new(),
            key: None,
            value: None,
        })
    }
}

impl<K, V, H> Deserialize for HashMap<K, V, H>
where
    K: Hash + Eq + Deserialize,
    V: Deserialize,
    H: BuildHasher + Default,
{
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct MapSink<'a, K: 'a, V: 'a, H> {
            slot: &'a mut Option<HashMap<K, V, H>>,
            map: HashMap<K, V, H>,
            key: Option<K>,
            value: Option<V>,
        }

        impl<'a, K, V, H> MapSink<'a, K, V, H>
        where
            K: Hash + Eq,
            H: BuildHasher,
        {
            fn flush(&mut self) {
                if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                    self.map.insert(key, value);
                }
            }
        }

        impl<'a, K, V, H> Sink for MapSink<'a, K, V, H>
        where
            K: Hash + Eq + Deserialize,
            V: Deserialize,
            H: BuildHasher + Default,
        {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: UnorderedNamedDescriptor =
                    UnorderedNamedDescriptor { name: "HashMap" };
                &DESCRIPTOR
            }

            fn map(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_key(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.key))
            }

            fn next_value(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                Ok(Deserialize::deserialize_into(&mut self.value))
            }

            fn key_atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                self.flush();
                atom_into(&mut self.key, atom, state)
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                atom_into(&mut self.value, atom, state)
            }

            fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                self.flush();
                *self.slot = Some(take(&mut self.map));
                Ok(())
            }
        }

        SinkHandle::boxed(MapSink {
            slot: out,
            map: HashMap::default(),
            key: None,
            value: None,
        })
    }
}

impl<T: Deserialize + Ord> Deserialize for BTreeSet<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct BTreeSetSink<'a, T> {
            slot: &'a mut Option<BTreeSet<T>>,
            set: BTreeSet<T>,
            element: Option<T>,
        }

        impl<'a, T: 'a + Ord> BTreeSetSink<'a, T> {
            fn flush(&mut self) {
                if let Some(element) = self.element.take() {
                    self.set.insert(element);
                }
            }
        }

        impl<'a, T: Deserialize + Ord> Sink for BTreeSetSink<'a, T> {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeSet" };
                &DESCRIPTOR
            }

            fn seq(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_value(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.element))
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                self.flush();
                atom_into(&mut self.element, atom, state)
            }

            fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                self.flush();
                *self.slot = Some(take(&mut self.set));
                Ok(())
            }
        }

        SinkHandle::boxed(BTreeSetSink {
            slot: out,
            set: BTreeSet::new(),
            element: None,
        })
    }
}

impl<T, H> Deserialize for HashSet<T, H>
where
    T: Deserialize + Hash + Eq,
    H: BuildHasher + Default,
{
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct HashSetSink<'a, T, H> {
            slot: &'a mut Option<HashSet<T, H>>,
            set: HashSet<T, H>,
            element: Option<T>,
        }

        impl<'a, T, H> HashSetSink<'a, T, H>
        where
            T: Hash + Eq,
            H: BuildHasher,
        {
            fn flush(&mut self) {
                if let Some(element) = self.element.take() {
                    self.set.insert(element);
                }
            }
        }

        impl<'a, T, H> Sink for HashSetSink<'a, T, H>
        where
            T: Hash + Eq + Deserialize,
            H: BuildHasher + Default,
        {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: UnorderedNamedDescriptor =
                    UnorderedNamedDescriptor { name: "HashSet" };
                &DESCRIPTOR
            }

            fn seq(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_value(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.element))
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                self.flush();
                atom_into(&mut self.element, atom, state)
            }

            fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                self.flush();
                *self.slot = Some(take(&mut self.set));
                Ok(())
            }
        }

        SinkHandle::boxed(HashSetSink {
            slot: out,
            set: HashSet::default(),
            element: None,
        })
    }
}

impl<T> Deserialize for Option<T>
where
    T: Deserialize,
{
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        *out = Some(None);
        Deserialize::deserialize_into(out.as_mut().unwrap()).ignore_null()
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut DeserializerState,
    ) -> Result<(), Error> {
        let inner = out.insert(None);
        if is_null_atom(&atom) {
            // the sink is created (and dropped without being used) so that
            // this behaves exactly like `deserialize_into`.  This matters
            // for nested options where the inner one becomes `Some(None)`.
            drop(T::deserialize_into(inner));
            Ok(())
        } else {
            T::__private_atom_into(inner, atom, state)
        }
    }

    fn __private_initial_value() -> Option<Self> {
        Some(None)
    }
}

macro_rules! deserialize_for_tuple {
    () => ();
    ($($name:ident,)+) => (
        impl<$($name: Deserialize),*> Deserialize for ($($name,)*) {
            fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
                #![allow(non_snake_case)]

                struct TupleSink<'a, $($name,)*> {
                    slot: &'a mut Option<($($name,)*)>,
                    index: usize,
                    $(
                        $name: Option<$name>,
                    )*
                }

                impl<'a, $($name: Deserialize,)*> Sink for TupleSink<'a, $($name,)*> {
                    fn descriptor(&self) -> &'static dyn Descriptor {
                        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "tuple" };
                        &DESCRIPTOR
                    }

                    fn seq(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                        Ok(())
                    }

                    fn next_value(&mut self, _state: &mut DeserializerState) -> Result<SinkHandle<'_>, Error> {
                        let __index = self.index;
                        self.index += 1;
                        let mut __counter = 0;
                        $(
                            if __index == __counter {
                                return Ok(Deserialize::deserialize_into(&mut self.$name));
                            }
                            __counter += 1;
                        )*
                        Err(Error::new(ErrorKind::WrongLength, "too many elements in tuple"))
                    }

                    fn value_atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                        let __index = self.index;
                        self.index += 1;
                        let mut __counter = 0;
                        $(
                            if __index == __counter {
                                return atom_into(&mut self.$name, atom, state);
                            }
                            __counter += 1;
                        )*
                        Err(Error::new(ErrorKind::WrongLength, "too many elements in tuple"))
                    }

                    fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                        *self.slot = Some(($(
                            self.$name
                                .take()
                                .ok_or_else(|| Error::new(ErrorKind::WrongLength, "not enough elements in tuple"))?,
                        )*));
                        Ok(())
                    }
                }

                SinkHandle::boxed(TupleSink {
                    slot: out,
                    index: 0,
                    $(
                        $name: None,
                    )*
                })
            }
        }

        deserialize_for_tuple_peel!($($name,)*);
    )
}

macro_rules! deserialize_for_tuple_peel {
    ($name:ident, $($other:ident,)*) => (deserialize_for_tuple!($($other,)*);)
}

deserialize_for_tuple! { T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, }

impl<T: Deserialize, const N: usize> Deserialize for [T; N] {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        // Invariant: if `buffer` is `Some`, the first `index` elements of it
        // are initialized.  Once the buffer was moved into the slot, `buffer`
        // is `None`.
        struct ArraySink<'a, T, const N: usize> {
            slot: &'a mut Option<[T; N]>,
            buffer: Option<[MaybeUninit<T>; N]>,
            element: Option<T>,
            index: usize,
            is_seq: bool,
        }

        impl<'a, T, const N: usize> ArraySink<'a, T, N> {
            fn flush(&mut self) {
                if let Some(element) = self.element.take() {
                    // indexing panics if the sink is misused and too many
                    // elements are pushed or the buffer is already gone.
                    let buffer = self.buffer.as_mut().expect("array already finished");
                    buffer[self.index].write(element);
                    self.index += 1;
                }
            }
        }

        impl<'a, T, const N: usize> Drop for ArraySink<'a, T, N> {
            fn drop(&mut self) {
                if let Some(ref mut buffer) = self.buffer {
                    for elem in &mut buffer[..self.index] {
                        // SAFETY: the first `index` elements are initialized
                        unsafe { elem.assume_init_drop() };
                    }
                }
            }
        }

        impl<'a, T: Deserialize + 'a, const N: usize> Sink for ArraySink<'a, T, N> {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "array" };
                &DESCRIPTOR
            }

            fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                match atom {
                    Atom::Bytes(value) => match T::__private_array_from_bytes::<N>(&value) {
                        Some(array) => {
                            *self.slot = Some(array);
                            Ok(())
                        }
                        None if T::__private_is_bytes() => Err(Error::new(
                            ErrorKind::WrongLength,
                            "byte array of wrong length",
                        )),
                        None => Err(Error::new(
                            ErrorKind::Unexpected,
                            format!("unexpected bytes, expected {}", self.expecting()),
                        )),
                    },
                    other => self.unexpected_atom(other, state),
                }
            }

            fn seq(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                self.is_seq = true;
                Ok(())
            }

            fn next_value(
                &mut self,
                _state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.flush();
                if self.index >= N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ))
                } else {
                    Ok(Deserialize::deserialize_into(&mut self.element))
                }
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                self.flush();
                if self.index >= N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ))
                } else {
                    atom_into(&mut self.element, atom, state)
                }
            }

            fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
                if !self.is_seq {
                    return Ok(());
                }
                self.flush();
                if self.index != N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "not enough elements in array",
                    ))
                } else if let Some(buffer) = self.buffer.take() {
                    // SAFETY: all `N` elements are initialized and ownership
                    // is transferred as the buffer was taken out of the sink.
                    // `MaybeUninit<T>` has the same layout as `T`.
                    let array = unsafe {
                        (&buffer as *const [MaybeUninit<T>; N])
                            .cast::<[T; N]>()
                            .read()
                    };
                    *self.slot = Some(array);
                    Ok(())
                } else {
                    Ok(())
                }
            }
        }

        SinkHandle::boxed(ArraySink {
            slot: out,
            // SAFETY: an array of `MaybeUninit` does not require initialization
            buffer: Some(unsafe { MaybeUninit::uninit().assume_init() }),
            element: None,
            index: 0,
            is_seq: false,
        })
    }
}

impl<T: Deserialize> Deserialize for Box<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct BoxSink<'a, T> {
            out: &'a mut Option<Box<T>>,
            sink: OwnedSink<T>,
        }

        impl<'a, T: Deserialize> Sink for BoxSink<'a, T> {
            fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().atom(atom, state)
            }

            fn map(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().map(state)
            }

            fn seq(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().seq(state)
            }

            fn next_key(&mut self, state: &mut DeserializerState) -> Result<SinkHandle<'_>, Error> {
                self.sink.borrow_mut().next_key(state)
            }

            fn next_value(
                &mut self,
                state: &mut DeserializerState,
            ) -> Result<SinkHandle<'_>, Error> {
                self.sink.borrow_mut().next_value(state)
            }

            fn key_atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().key_atom(atom, state)
            }

            fn value_atom(
                &mut self,
                atom: Atom,
                state: &mut DeserializerState,
            ) -> Result<(), Error> {
                self.sink.borrow_mut().value_atom(atom, state)
            }

            fn value_for_key(
                &mut self,
                key: &str,
                state: &mut DeserializerState,
            ) -> Result<Option<SinkHandle<'_>>, Error> {
                self.sink.borrow_mut().value_for_key(key, state)
            }

            fn finish(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().finish(state)?;
                *self.out = self.sink.take().map(Box::new);
                Ok(())
            }

            fn descriptor(&self) -> &'static dyn Descriptor {
                self.sink.borrow().descriptor()
            }

            fn expecting(&self) -> std::borrow::Cow<'_, str> {
                self.sink.borrow().expecting()
            }
        }

        SinkHandle::boxed(BoxSink {
            out,
            sink: OwnedSink::deserialize(),
        })
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut DeserializerState,
    ) -> Result<(), Error> {
        let mut inner = None;
        T::__private_atom_into(&mut inner, atom, state)?;
        *out = inner.map(Box::new);
        Ok(())
    }
}
