use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

use crate::descriptors::{Descriptor, NamedDescriptor, NumberDescriptor, UnorderedNamedDescriptor};
use crate::error::Error;
use crate::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle, SerializerState};

impl Serialize for bool {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bool" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Bool(*self))
    }
}

impl Serialize for () {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "()" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Null)
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
        Ok(Chunk::U64(*self as u64))
    }

    fn __private_slice_as_bytes(val: &[u8]) -> Option<&[u8]> {
        Some(val)
    }
}

impl Serialize for u16 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "u16",
            precision: 16,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::U64(*self as u64))
    }
}

impl Serialize for u32 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "u32",
            precision: 32,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::U64(*self as u64))
    }
}

impl Serialize for u64 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "u64",
            precision: 64,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::U64(*self))
    }
}

impl Serialize for i8 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "i8",
            precision: 8,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::I64(*self as i64))
    }
}

impl Serialize for i16 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "i16",
            precision: 16,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::I64(*self as i64))
    }
}

impl Serialize for i32 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "i32",
            precision: 32,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::I64(*self as i64))
    }
}

impl Serialize for i64 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "i64",
            precision: 64,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::I64(*self))
    }
}

impl Serialize for isize {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "isize",
            precision: std::mem::size_of::<isize>() * 8,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::I64(*self as i64))
    }
}

impl Serialize for usize {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "usize",
            precision: std::mem::size_of::<usize>() * 8,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::U64(*self as u64))
    }
}

impl Serialize for f32 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "f32",
            precision: 32,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::F64(*self as f64))
    }
}

impl Serialize for f64 {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NumberDescriptor = NumberDescriptor {
            name: "f64",
            precision: 64,
        };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::F64(*self))
    }
}

impl Serialize for String {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "String" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Str(self.as_str().into()))
    }
}

impl<'a> Serialize for &'a str {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Str((*self).into()))
    }
}

impl<'a> Serialize for Cow<'a, str> {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Str(Cow::Borrowed(self)))
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
            Ok(Chunk::Bytes(Cow::Borrowed(bytes)))
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
            Ok(Chunk::Bytes(Cow::Borrowed(bytes)))
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
