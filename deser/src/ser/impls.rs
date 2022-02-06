use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

use crate::descriptors::{Descriptor, NamedDescriptor, NumberDescriptor, UnorderedNamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle, SerializerState};

impl Serialize for bool {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bool" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Atom(Atom::Bool(*self)))
    }
}

impl Serialize for () {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "()" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Atom(Atom::Null))
    }
}

impl Serialize for u8 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "u8",
            precision: 8,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Atom(Atom::U64(*self as u64)))
    }

    fn __private_slice_as_bytes(val: &[u8]) -> Option<&[u8]> {
        Some(val)
    }
}

macro_rules! serialize_int {
    ($ty:ty, $atom:ident) => {
        impl Serialize for $ty {
            fn descriptor(&self) -> &dyn Descriptor {
                static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
                    name: stringify!($ty),
                    precision: std::mem::size_of::<$ty>() * 8,
                };
                &DESCRIPTOR
            }

            fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
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

impl Serialize for String {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "String" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Atom(Atom::Str(self.as_str().into())))
    }
}

impl<'a> Serialize for &'a str {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Atom(Atom::Str((*self).into())))
    }
}

impl<'a> Serialize for Cow<'a, str> {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Atom(Atom::Str(Cow::Borrowed(self))))
    }
}

impl<T> Serialize for Vec<T>
where
    T: Serialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Vec" };
        static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "ByteVec" };
        if T::__private_slice_as_bytes(self).is_some() {
            &BYTES_DESCRIPTOR
        } else {
            &SLICE_DESCRIPTOR
        }
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        if let Some(bytes) = T::__private_slice_as_bytes(&self[..]) {
            Ok(Chunk::Atom(Atom::Bytes(Cow::Borrowed(bytes))))
        } else {
            Ok(Chunk::Seq(Box::new(SliceEmitter((&self[..]).iter()))))
        }
    }
}

impl<'a, T> Serialize for &'a [T]
where
    T: Serialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "slice" };
        static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bytes" };
        if T::__private_slice_as_bytes(self).is_some() {
            &BYTES_DESCRIPTOR
        } else {
            &SLICE_DESCRIPTOR
        }
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        if let Some(bytes) = T::__private_slice_as_bytes(self) {
            Ok(Chunk::Atom(Atom::Bytes(Cow::Borrowed(bytes))))
        } else {
            Ok(Chunk::Seq(Box::new(SliceEmitter(self.iter()))))
        }
    }
}

struct SliceEmitter<'a, T>(std::slice::Iter<'a, T>);

impl<'a, T: Serialize> SeqEmitter for SliceEmitter<'a, T> {
    fn next(&mut self) -> Option<SerializeHandle> {
        self.0.next().map(SerializeHandle::to)
    }
}

impl<K, V> Serialize for BTreeMap<K, V>
where
    K: Serialize,
    V: Serialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeMap" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, K, V>(std::collections::btree_map::Iter<'a, K, V>, Option<&'a V>);

        impl<'a, K, V> MapEmitter for Emitter<'a, K, V>
        where
            K: Serialize,
            V: Serialize,
        {
            fn next_key(&mut self) -> Option<SerializeHandle> {
                self.0.next().map(|(k, v)| {
                    self.1 = Some(v);
                    SerializeHandle::to(k)
                })
            }

            fn next_value(&mut self) -> SerializeHandle {
                SerializeHandle::to(self.1.unwrap())
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
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashMap" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, K, V>(std::collections::hash_map::Iter<'a, K, V>, Option<&'a V>);

        impl<'a, K, V> MapEmitter for Emitter<'a, K, V>
        where
            K: Serialize,
            V: Serialize,
        {
            fn next_key(&mut self) -> Option<SerializeHandle> {
                self.0.next().map(|(k, v)| {
                    self.1 = Some(v);
                    SerializeHandle::to(k)
                })
            }

            fn next_value(&mut self) -> SerializeHandle {
                SerializeHandle::to(self.1.unwrap())
            }
        }

        Ok(Chunk::Map(Box::new(Emitter(self.iter(), None))))
    }
}

impl<T> Serialize for BTreeSet<T>
where
    T: Serialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeSet" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, T>(std::collections::btree_set::Iter<'a, T>);

        impl<'a, T> SeqEmitter for Emitter<'a, T>
        where
            T: Serialize,
        {
            fn next(&mut self) -> Option<SerializeHandle> {
                self.0.next().map(SerializeHandle::to)
            }
        }

        Ok(Chunk::Seq(Box::new(Emitter(self.iter()))))
    }
}

impl<T> Serialize for HashSet<T>
where
    T: Serialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashSet" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, T>(std::collections::hash_set::Iter<'a, T>);

        impl<'a, T> SeqEmitter for Emitter<'a, T>
        where
            T: Serialize,
        {
            fn next(&mut self) -> Option<SerializeHandle> {
                self.0.next().map(SerializeHandle::to)
            }
        }

        Ok(Chunk::Seq(Box::new(Emitter(self.iter()))))
    }
}

impl<T> Serialize for Option<T>
where
    T: Serialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "optional" };
        &DESCRIPTOR
    }

    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        match self {
            Some(value) => value.serialize(state),
            None => Ok(Chunk::Atom(Atom::Null)),
        }
    }
}
