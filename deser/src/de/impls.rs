use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;
use std::hash::Hash;
use std::mem::{take, MaybeUninit};

use crate::de::{Deserialize, DeserializerState, OwnedSink, Sink, SinkHandle};
use crate::descriptors::{Descriptor, NamedDescriptor, UnorderedNamedDescriptor};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;

make_slot_wrapper!(SlotWrapper);

macro_rules! deserialize {
    ($ty:ty) => {
        impl Deserialize for $ty {
            fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
                SlotWrapper::make_handle(out)
            }
        }
    };
}

impl Sink for SlotWrapper<()> {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "null" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
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
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bool" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
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
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "string" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor {
                    name: stringify!($ty),
                };
                &DESCRIPTOR
            }

            fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
                match atom {
                    Atom::U64(value) => {
                        let truncated = value as $ty;
                        if truncated as u64 == value {
                            **self = Some(truncated);
                            Ok(())
                        } else {
                            Err(Error::new(
                                ErrorKind::OutOfRange,
                                "value out of range for type",
                            ))
                        }
                    }
                    Atom::I64(value) => {
                        let truncated = value as $ty;
                        if truncated as i64 == value {
                            **self = Some(truncated);
                            Ok(())
                        } else {
                            Err(Error::new(
                                ErrorKind::OutOfRange,
                                "value out of range for type",
                            ))
                        }
                    }
                    other => self.unexpected_atom(other, state),
                }
            }
        }
    };
}

int_sink!(u8);

impl Deserialize for u8 {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        SlotWrapper::make_handle(out)
    }

    unsafe fn __private_is_bytes() -> bool {
        true
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

impl Sink for SlotWrapper<char> {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "char" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor {
                    name: stringify!($ty),
                };
                &DESCRIPTOR
            }

            fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "vec" };
                static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bytes" };
                if unsafe { T::__private_is_bytes() } {
                    &BYTES_DESCRIPTOR
                } else {
                    &SLICE_DESCRIPTOR
                }
            }

            fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
                match atom {
                    Atom::Bytes(value) => unsafe {
                        if T::__private_is_bytes() {
                            *self.slot = Some(std::mem::transmute(value.into_owned()));
                            Ok(())
                        } else {
                            Err(Error::new(
                                ErrorKind::Unexpected,
                                format!("unexpected bytes, expected {}", self.expecting()),
                            ))
                        }
                    },
                    other => self.unexpected_atom(other, state),
                }
            }

            fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                self.is_seq = true;
                Ok(())
            }

            fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.element))
            }

            fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: UnorderedNamedDescriptor =
                    UnorderedNamedDescriptor { name: "map" };
                &DESCRIPTOR
            }

            fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_key(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.key))
            }

            fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                Ok(Deserialize::deserialize_into(&mut self.value))
            }

            fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: UnorderedNamedDescriptor =
                    UnorderedNamedDescriptor { name: "map" };
                &DESCRIPTOR
            }

            fn next_key(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.key))
            }

            fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                Ok(Deserialize::deserialize_into(&mut self.value))
            }

            fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: UnorderedNamedDescriptor =
                    UnorderedNamedDescriptor { name: "BTreeSet" };
                &DESCRIPTOR
            }

            fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.element))
            }

            fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
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
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: UnorderedNamedDescriptor =
                    UnorderedNamedDescriptor { name: "HashSet" };
                &DESCRIPTOR
            }

            fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                Ok(())
            }

            fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.flush();
                Ok(Deserialize::deserialize_into(&mut self.element))
            }

            fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        *out = Some(None);
        SinkHandle::boxed(NullIgnoringSink {
            sink: Deserialize::deserialize_into(out.as_mut().unwrap()),
        })
    }

    fn __private_initial_value() -> Option<Self> {
        Some(None)
    }
}

struct NullIgnoringSink<'a> {
    sink: SinkHandle<'a>,
}

impl<'a> Sink for NullIgnoringSink<'a> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Null => Ok(()),
            other => self.sink.atom(other, state),
        }
    }

    fn map(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.map(state)
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.seq(state)
    }

    fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
        self.sink.next_key(state)
    }

    fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
        self.sink.next_value(state)
    }

    fn descriptor(&self) -> &dyn Descriptor {
        self.sink.descriptor()
    }
}

macro_rules! deserialize_for_tuple {
    () => ();
    ($($name:ident,)+) => (
        impl<$($name: Deserialize),*> Deserialize for ($($name,)*) {
            fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
                #![allow(non_snake_case)]

                struct TupleSink<'a, $($name,)*> {
                    slot: &'a mut Option<($($name,)*)>,
                    index: usize,
                    $(
                        $name: Option<$name>,
                    )*
                }

                impl<'a, $($name: Deserialize,)*> Sink for TupleSink<'a, $($name,)*> {
                    fn descriptor(&self) -> &dyn Descriptor {
                        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "tuple" };
                        &DESCRIPTOR
                    }

                    fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                        Ok(())
                    }

                    fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
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

                    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
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
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        struct ArraySink<'a, T, const N: usize> {
            slot: &'a mut Option<[T; N]>,
            buffer: Option<[MaybeUninit<T>; N]>,
            element: Option<T>,
            index: usize,
            is_seq: bool,
        }

        impl<'a, T, const N: usize> ArraySink<'a, T, N> {
            unsafe fn flush(&mut self) {
                if let Some(element) = self.element.take() {
                    let buffer = self.buffer.as_mut().unwrap();
                    buffer[self.index].write(element);
                    self.index += 1;
                }
            }
        }

        impl<'a, T, const N: usize> Drop for ArraySink<'a, T, N> {
            fn drop(&mut self) {
                if std::mem::needs_drop::<T>() {
                    if let Some(arr) = &mut self.buffer {
                        for elem in &mut arr[0..self.index] {
                            unsafe {
                                std::ptr::drop_in_place(elem.as_mut_ptr());
                            }
                        }
                    }
                }
            }
        }

        impl<'a, T: Deserialize + 'a, const N: usize> Sink for ArraySink<'a, T, N> {
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "array" };
                &DESCRIPTOR
            }

            fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
                match atom {
                    Atom::Bytes(value) => {
                        if unsafe { T::__private_is_bytes() } {
                            if value.len() == N {
                                *self.slot = Some(unsafe {
                                    let mut rv = MaybeUninit::<[T; N]>::uninit();
                                    std::ptr::copy_nonoverlapping(
                                        value.as_ptr() as *const T,
                                        rv.as_mut_ptr() as *mut T,
                                        N,
                                    );
                                    rv.assume_init()
                                });
                                Ok(())
                            } else {
                                Err(Error::new(
                                    ErrorKind::WrongLength,
                                    "byte array of wrong length",
                                ))
                            }
                        } else {
                            Err(Error::new(
                                ErrorKind::Unexpected,
                                format!("unexpected bytes, expected {}", self.expecting()),
                            ))
                        }
                    }
                    other => self.unexpected_atom(other, state),
                }
            }

            fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                self.is_seq = true;
                Ok(())
            }

            fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle, Error> {
                unsafe {
                    self.flush();
                }
                if self.index >= N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ))
                } else {
                    Ok(Deserialize::deserialize_into(&mut self.element))
                }
            }

            fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
                if !self.is_seq {
                    return Ok(());
                }
                unsafe {
                    self.flush();
                }
                if self.index != N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "not enough elements in array",
                    ))
                } else {
                    *self.slot = Some(unsafe {
                        self.buffer.take().unwrap().as_ptr().cast::<[T; N]>().read()
                    });
                    Ok(())
                }
            }
        }

        SinkHandle::boxed(ArraySink {
            slot: out,
            buffer: Some(unsafe { MaybeUninit::uninit().assume_init() }),
            element: None,
            index: 0,
            is_seq: false,
        })
    }
}

impl<T: Deserialize> Deserialize for Box<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        struct BoxSink<'a, T> {
            out: &'a mut Option<Box<T>>,
            sink: OwnedSink<T>,
        }

        impl<'a, T: Deserialize> Sink for BoxSink<'a, T> {
            fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().atom(atom, state)
            }

            fn map(&mut self, state: &DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().map(state)
            }

            fn seq(&mut self, state: &DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().seq(state)
            }

            fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.sink.borrow_mut().next_key(state)
            }

            fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle, Error> {
                self.sink.borrow_mut().next_value(state)
            }

            fn value_for_key(
                &mut self,
                key: &str,
                state: &DeserializerState,
            ) -> Result<Option<SinkHandle>, Error> {
                self.sink.borrow_mut().value_for_key(key, state)
            }

            fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
                self.sink.borrow_mut().finish(state)?;
                *self.out = self.sink.take().map(Box::new);
                Ok(())
            }

            fn descriptor(&self) -> &dyn Descriptor {
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
}
