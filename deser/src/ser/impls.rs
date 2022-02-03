use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

use crate::descriptors::{Descriptor, NamedDescriptor, NumberDescriptor, UnorderedNamedDescriptor};
use crate::error::Error;
use crate::ser::{Chunk, MapEmitter, SeqEmitter, Serializable, SerializerState};

impl Serializable for bool {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bool" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Bool(*self))
    }
}

impl Serializable for () {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "()" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Null)
    }
}

impl Serializable for u8 {
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

impl Serializable for u16 {
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

impl Serializable for u32 {
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

impl Serializable for u64 {
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

impl Serializable for i8 {
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

impl Serializable for i16 {
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

impl Serializable for i32 {
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

impl Serializable for i64 {
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

impl Serializable for f32 {
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

impl Serializable for f64 {
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

impl Serializable for String {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "String" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Str(self.as_str().into()))
    }
}

impl<'a> Serializable for &'a str {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Str((*self).into()))
    }
}

impl<'a> Serializable for Cow<'a, str> {
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Str(Cow::Borrowed(self)))
    }
}

impl<T> Serializable for Vec<T>
where
    T: Serializable,
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

impl<'a, T> Serializable for &'a [T]
where
    T: Serializable,
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

impl<'a, T: Serializable> SeqEmitter for SliceEmitter<'a, T> {
    fn next(&mut self) -> Option<&dyn Serializable> {
        self.0.next().map(|x| x as _)
    }
}

impl<K, V> Serializable for BTreeMap<K, V>
where
    K: Serializable,
    V: Serializable,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeMap" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, K, V>(std::collections::btree_map::Iter<'a, K, V>, Option<&'a V>);

        impl<'a, K, V> MapEmitter for Emitter<'a, K, V>
        where
            K: Serializable,
            V: Serializable,
        {
            fn next_key(&mut self) -> Option<&dyn Serializable> {
                self.0.next().map(|(k, v)| {
                    self.1 = Some(v);
                    k as &dyn Serializable
                })
            }

            fn next_value(&mut self) -> &dyn Serializable {
                self.1.unwrap()
            }
        }

        Ok(Chunk::Map(Box::new(Emitter(self.iter(), None))))
    }
}

impl<K, V, H> Serializable for HashMap<K, V, H>
where
    K: Serializable,
    V: Serializable,
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
            K: Serializable,
            V: Serializable,
        {
            fn next_key(&mut self) -> Option<&dyn Serializable> {
                self.0.next().map(|(k, v)| {
                    self.1 = Some(v);
                    k as &dyn Serializable
                })
            }

            fn next_value(&mut self) -> &dyn Serializable {
                self.1.unwrap()
            }
        }

        Ok(Chunk::Map(Box::new(Emitter(self.iter(), None))))
    }
}

impl<T> Serializable for BTreeSet<T>
where
    T: Serializable,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeSet" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, T>(std::collections::btree_set::Iter<'a, T>);

        impl<'a, T> SeqEmitter for Emitter<'a, T>
        where
            T: Serializable,
        {
            fn next(&mut self) -> Option<&dyn Serializable> {
                self.0.next().map(|v| v as _)
            }
        }

        Ok(Chunk::Seq(Box::new(Emitter(self.iter()))))
    }
}

impl<T> Serializable for HashSet<T>
where
    T: Serializable,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashSet" };
        &DESCRIPTOR
    }

    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        struct Emitter<'a, T>(std::collections::hash_set::Iter<'a, T>);

        impl<'a, T> SeqEmitter for Emitter<'a, T>
        where
            T: Serializable,
        {
            fn next(&mut self) -> Option<&dyn Serializable> {
                self.0.next().map(|v| v as _)
            }
        }

        Ok(Chunk::Seq(Box::new(Emitter(self.iter()))))
    }
}
