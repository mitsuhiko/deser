use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

use crate::descriptors::{Descriptor, NamedDescriptor, NumberDescriptor, UnorderedNamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ext::ExtValue;
use crate::ser::{Begin, Chunk, IndexedSeq, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use crate::State;

impl Serialize for bool {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bool" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Bool(*self)))
    }
}

impl Serialize for () {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "null" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Null))
    }

    fn is_optional(&self) -> bool {
        true
    }
}

impl Serialize for u8 {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "u8",
            precision: 8,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::U64(*self as u64)))
    }

    fn __private_slice_as_bytes(val: &[u8]) -> Option<Cow<'_, [u8]>> {
        Some(Cow::Borrowed(val))
    }
}

impl Serialize for char {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "char" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Char(*self)))
    }
}

macro_rules! serialize_int {
    ($ty:ty, $atom:ident) => {
        impl Serialize for $ty {
            __begin_without_finish!();

            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
                    name: stringify!($ty),
                    precision: std::mem::size_of::<$ty>() * 8,
                };
                &DESCRIPTOR
            }

            fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                Ok(Chunk::Atom(Atom::$atom(*self as _)))
            }
        }
    };
}

serialize_int!(u16, U64);
serialize_int!(u32, U64);
serialize_int!(u64, U64);
serialize_int!(i8, I64);
serialize_int!(i16, I64);
serialize_int!(i32, I64);
serialize_int!(i64, I64);
serialize_int!(isize, I64);
serialize_int!(usize, U64);
serialize_int!(f32, F64);
serialize_int!(f64, F64);

macro_rules! serialize_ext_int {
    ($ty:ty) => {
        impl Serialize for $ty {
            __begin_without_finish!();

            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
                    name: stringify!($ty),
                    precision: std::mem::size_of::<$ty>() * 8,
                };
                &DESCRIPTOR
            }

            fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
            }
        }
    };
}

serialize_ext_int!(u128);
serialize_ext_int!(i128);

impl Serialize for String {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "String" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str(self.as_str().into())))
    }
}

impl Serialize for &str {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str((*self).into())))
    }
}

impl<'a> Serialize for Cow<'a, str> {
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str(Cow::Borrowed(self))))
    }
}

impl<T> Serialize for Vec<T>
where
    T: Serialize,
{
    #[inline]
    fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
        let descriptor = self.descriptor();
        Ok(match T::__private_slice_as_bytes(&self[..]) {
            Some(bytes) => Begin::chunk(Chunk::Atom(Atom::Bytes(bytes)), descriptor, false),
            None => Begin::indexed_seq(self, descriptor),
        })
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Vec" };
        static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "ByteVec" };
        if T::__private_slice_as_bytes(self).is_some() {
            &BYTES_DESCRIPTOR
        } else {
            &SLICE_DESCRIPTOR
        }
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(bytes) = T::__private_slice_as_bytes(&self[..]) {
            Ok(Chunk::Atom(Atom::Bytes(bytes)))
        } else {
            Ok(Chunk::Seq(Box::new(SliceEmitter(self[..].iter()))))
        }
    }
}

impl<T> Serialize for &[T]
where
    T: Serialize,
{
    #[inline]
    fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
        let descriptor = self.descriptor();
        Ok(match T::__private_slice_as_bytes(&self[..]) {
            Some(bytes) => Begin::chunk(Chunk::Atom(Atom::Bytes(bytes)), descriptor, false),
            None => Begin::indexed_seq(self, descriptor),
        })
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "slice" };
        static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bytes" };
        if T::__private_slice_as_bytes(self).is_some() {
            &BYTES_DESCRIPTOR
        } else {
            &SLICE_DESCRIPTOR
        }
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(bytes) = T::__private_slice_as_bytes(self) {
            Ok(Chunk::Atom(Atom::Bytes(bytes)))
        } else {
            Ok(Chunk::Seq(Box::new(SliceEmitter(self.iter()))))
        }
    }
}

macro_rules! indexed_slice {
    ($ty:ty $(, $param:ident)*) => {
        impl<T: Serialize $(, const $param: usize)*> IndexedSeq for $ty {
            #[inline]
            fn element(
                &self,
                index: usize,
                _state: &mut State,
            ) -> Result<Option<SerializeHandle<'_>>, Error> {
                Ok(self.get(index).map(SerializeHandle::to))
            }
        }
    };
}

indexed_slice!(Vec<T>);
indexed_slice!(&[T]);
indexed_slice!([T; N], N);

struct SliceEmitter<'a, T>(std::slice::Iter<'a, T>);

impl<'a, T: Serialize> SeqEmitter for SliceEmitter<'a, T> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

impl<K, V> Serialize for BTreeMap<K, V>
where
    K: Serialize,
    V: Serialize,
{
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeMap" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        struct Emitter<'a, K, V>(std::collections::btree_map::Iter<'a, K, V>, Option<&'a V>);

        impl<'a, K, V> MapEmitter for Emitter<'a, K, V>
        where
            K: Serialize,
            V: Serialize,
        {
            fn next_key(
                &mut self,
                _state: &mut State,
            ) -> Result<Option<SerializeHandle<'_>>, Error> {
                Ok(self.0.next().map(|(k, v)| {
                    self.1 = Some(v);
                    SerializeHandle::to(k)
                }))
            }

            fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
                Ok(SerializeHandle::to(self.1.unwrap()))
            }
        }

        Ok(Chunk::Map(Box::new(Emitter(self.iter(), None))))
    }
}

impl<K, V, H> Serialize for HashMap<K, V, H>
where
    K: Serialize,
    V: Serialize,
    H: BuildHasher,
{
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashMap" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        struct Emitter<'a, K, V>(std::collections::hash_map::Iter<'a, K, V>, Option<&'a V>);

        impl<'a, K, V> MapEmitter for Emitter<'a, K, V>
        where
            K: Serialize,
            V: Serialize,
        {
            fn next_key(
                &mut self,
                _state: &mut State,
            ) -> Result<Option<SerializeHandle<'_>>, Error> {
                Ok(self.0.next().map(|(k, v)| {
                    self.1 = Some(v);
                    SerializeHandle::to(k)
                }))
            }

            fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
                Ok(SerializeHandle::to(self.1.unwrap()))
            }
        }

        Ok(Chunk::Map(Box::new(Emitter(self.iter(), None))))
    }
}

impl<T> Serialize for BTreeSet<T>
where
    T: Serialize,
{
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeSet" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        struct Emitter<'a, T>(std::collections::btree_set::Iter<'a, T>);

        impl<'a, T> SeqEmitter for Emitter<'a, T>
        where
            T: Serialize,
        {
            fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
                Ok(self.0.next().map(SerializeHandle::to))
            }
        }

        Ok(Chunk::Seq(Box::new(Emitter(self.iter()))))
    }
}

impl<T> Serialize for HashSet<T>
where
    T: Serialize,
{
    __begin_without_finish!();

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashSet" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        struct Emitter<'a, T>(std::collections::hash_set::Iter<'a, T>);

        impl<'a, T> SeqEmitter for Emitter<'a, T>
        where
            T: Serialize,
        {
            fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
                Ok(self.0.next().map(SerializeHandle::to))
            }
        }

        Ok(Chunk::Seq(Box::new(Emitter(self.iter()))))
    }
}

impl<T> Serialize for Option<T>
where
    T: Serialize,
{
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "optional" };
        match self {
            Some(value) => value.descriptor(),
            None => &DESCRIPTOR,
        }
    }

    fn is_optional(&self) -> bool {
        self.is_none()
    }

    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        match self {
            Some(value) => value.serialize(state),
            None => Ok(Chunk::Atom(Atom::Null)),
        }
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        match self {
            Some(value) => value.finish(state),
            None => Ok(()),
        }
    }

    #[inline]
    fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
        match self {
            Some(value) => value.__private_begin(state),
            None => Ok(Begin::chunk(
                Chunk::Atom(Atom::Null),
                self.descriptor(),
                false,
            )),
        }
    }
}

macro_rules! serialize_for_tuple {
    () => ();
    ($($name:ident,)+) => (
        impl<$($name: Serialize),*> Serialize for ($($name,)*) {
            #[inline]
            fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
                Ok(Begin::indexed_seq(self, self.descriptor()))
            }

            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "tuple" };
                &DESCRIPTOR
            }

            #[allow(non_snake_case)]
            fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                struct TupleSeqEmitter<'a, $($name,)*> {
                    tuple: &'a ($($name,)*),
                    index: usize,
                }

                impl<'a, $($name,)*> SeqEmitter for TupleSeqEmitter<'a, $($name,)*>
                where
                    $($name: Serialize,)*
                {
                    fn next(&mut self,_state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
                        let ($(ref $name,)*) = self.tuple;
                        let __index = self.index;
                        self.index += 1;
                        let mut __counter = 0;
                        $(
                            if __index == __counter {
                                return Ok(Some(SerializeHandle::to($name)));
                            }
                            __counter += 1;
                        )*
                        Ok(None)
                    }
                }

                Ok(Chunk::Seq(Box::new(TupleSeqEmitter {
                    tuple: self,
                    index: 0,
                })))
            }
        }
        impl<$($name: Serialize),*> IndexedSeq for ($($name,)*) {
            #[allow(non_snake_case)]
            fn element(&self, index: usize, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
                let ($(ref $name,)*) = self;
                let mut __counter = 0;
                $(
                    if index == __counter {
                        return Ok(Some(SerializeHandle::to($name)));
                    }
                    __counter += 1;
                )*
                let _ = __counter;
                Ok(None)
            }
        }

        serialize_for_tuple_peel!($($name,)*);
    )
}

macro_rules! serialize_for_tuple_peel {
    ($name:ident, $($other:ident,)*) => (serialize_for_tuple!($($other,)*);)
}

serialize_for_tuple! { T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, }

impl<T: Serialize, const N: usize> Serialize for [T; N] {
    #[inline]
    fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
        let descriptor = self.descriptor();
        Ok(match T::__private_slice_as_bytes(&self[..]) {
            Some(bytes) => Begin::chunk(Chunk::Atom(Atom::Bytes(bytes)), descriptor, false),
            None => Begin::indexed_seq(self, descriptor),
        })
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "array" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(bytes) = T::__private_slice_as_bytes(self) {
            Ok(Chunk::Atom(Atom::Bytes(bytes)))
        } else {
            Ok(Chunk::Seq(Box::new(SliceEmitter(self.iter()))))
        }
    }
}

macro_rules! forward_serialize {
    ($($ty:ty),*) => {
        $(
            impl<'a, T: Serialize + ?Sized> Serialize for $ty {
                fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
                    Serialize::serialize(&**self, state)
                }

                fn finish(&self, state: &mut State) -> Result<(), Error> {
                    Serialize::finish(&**self, state)
                }

                #[inline]
                fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
                    Serialize::__private_begin(&**self, state)
                }

                fn is_optional(&self) -> bool {
                    Serialize::is_optional(&**self)
                }

                fn descriptor(&self) -> &'static dyn Descriptor {
                    Serialize::descriptor(&**self)
                }
            }
        )*
    };
}

forward_serialize!(&'a T, &'a mut T, Box<T>);
