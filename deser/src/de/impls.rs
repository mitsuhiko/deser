use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;
use std::hash::Hash;
use std::mem::take;

use crate::de::{Deserializable, DeserializerState, MapSink, SeqSink, Sink, SinkRef};
use crate::descriptors::{Descriptor, NamedDescriptor, UnorderedNamedDescriptor};
use crate::error::{Error, ErrorKind};

make_slot_wrapper!(SlotWrapper);

macro_rules! deserializable {
    ($ty:ty) => {
        impl Deserializable for $ty {
            fn attach(out: &mut Option<Self>) -> SinkRef {
                SinkRef::Borrowed(SlotWrapper::wrap(out))
            }
        }
    };
}

impl Sink for SlotWrapper<()> {
    fn expecting(&self) -> Cow<'_, str> {
        "null".into()
    }

    fn null(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        **self = Some(());
        Ok(())
    }
}
deserializable!(());

impl Sink for SlotWrapper<bool> {
    fn expecting(&self) -> Cow<'_, str> {
        "bool".into()
    }

    fn bool(&mut self, value: bool, _state: &DeserializerState) -> Result<(), Error> {
        **self = Some(value);
        Ok(())
    }
}
deserializable!(bool);

impl Sink for SlotWrapper<String> {
    fn expecting(&self) -> Cow<'_, str> {
        "string".into()
    }

    fn str(&mut self, value: &str, _state: &DeserializerState) -> Result<(), Error> {
        **self = Some(value.to_string());
        Ok(())
    }
}
deserializable!(String);

macro_rules! int_sink {
    ($ty:ty) => {
        impl Sink for SlotWrapper<$ty> {
            fn expecting(&self) -> Cow<'_, str> {
                stringify!($ty).into()
            }

            fn u64(&mut self, value: u64, _state: &DeserializerState) -> Result<(), Error> {
                let truncated = value as $ty;
                if truncated as u64 == value {
                    **self = Some(truncated);
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::OutOfRange,
                        "value out of range for u8",
                    ))
                }
            }

            fn i64(&mut self, value: i64, _state: &DeserializerState) -> Result<(), Error> {
                let truncated = value as $ty;
                if truncated as i64 == value {
                    **self = Some(truncated);
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::OutOfRange,
                        concat!("value out of range for ", stringify!($ty)),
                    ))
                }
            }
        }
    };
}

int_sink!(u8);

impl Deserializable for u8 {
    fn attach(out: &mut Option<Self>) -> SinkRef {
        SinkRef::Borrowed(SlotWrapper::wrap(out))
    }

    fn __private_byte_slice(bytes: &[u8]) -> Option<&[Self]>
    where
        Self: Sized,
    {
        Some(bytes)
    }
}

int_sink!(u16);
deserializable!(u16);
int_sink!(u32);
deserializable!(u32);
int_sink!(u64);
deserializable!(u64);
int_sink!(i8);
deserializable!(i8);
int_sink!(i16);
deserializable!(i16);
int_sink!(i32);
deserializable!(i32);
int_sink!(i64);
deserializable!(i64);
int_sink!(isize);
deserializable!(isize);
int_sink!(usize);
deserializable!(usize);

macro_rules! float_sink {
    ($ty:ty) => {
        impl Sink for SlotWrapper<$ty> {
            fn expecting(&self) -> Cow<'_, str> {
                stringify!($ty).into()
            }

            fn u64(&mut self, value: u64, _state: &DeserializerState) -> Result<(), Error> {
                **self = Some(value as $ty);
                Ok(())
            }

            fn i64(&mut self, value: i64, _state: &DeserializerState) -> Result<(), Error> {
                **self = Some(value as $ty);
                Ok(())
            }

            fn f64(&mut self, value: f64, _state: &DeserializerState) -> Result<(), Error> {
                **self = Some(value as $ty);
                Ok(())
            }
        }
    };
}

float_sink!(f32);
deserializable!(f32);

float_sink!(f64);
deserializable!(f64);

impl<T: Deserializable + Clone> Sink for SlotWrapper<Vec<T>> {
    fn expecting(&self) -> Cow<'_, str> {
        if T::__private_byte_slice(&[][..]).is_some() {
            "bytes".into()
        } else {
            "vec".into()
        }
    }

    fn bytes(&mut self, value: &[u8], _state: &DeserializerState) -> Result<(), Error> {
        if let Some(byte_slice) = T::__private_byte_slice(value) {
            **self = Some(byte_slice.to_vec());
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Unexpected,
                format!("unexpected bytes, expected {}", self.expecting()),
            ))
        }
    }

    fn seq(&mut self, _state: &DeserializerState) -> Result<Box<dyn SeqSink + '_>, Error> {
        Ok(Box::new(VecSink {
            slot: self,
            vec: Vec::new(),
            element: None,
        }))
    }
}

impl<T: Deserializable + Clone> Deserializable for Vec<T> {
    fn attach(out: &mut Option<Self>) -> SinkRef {
        SinkRef::Borrowed(SlotWrapper::wrap(out))
    }
}

impl<K, V> Sink for SlotWrapper<BTreeMap<K, V>>
where
    K: Ord + Deserializable,
    V: Deserializable,
{
    fn expecting(&self) -> Cow<'_, str> {
        "map".into()
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<Box<dyn super::MapSink + '_>, Error> {
        Ok(Box::new(BTreeMapSink {
            slot: self,
            map: BTreeMap::new(),
            key: None,
            value: None,
        }))
    }
}

impl<K, V> Deserializable for BTreeMap<K, V>
where
    K: Ord + Deserializable,
    V: Deserializable,
{
    fn attach(out: &mut Option<Self>) -> SinkRef {
        SinkRef::Borrowed(SlotWrapper::wrap(out))
    }
}

struct BTreeMapSink<'a, K: 'a, V: 'a> {
    slot: &'a mut Option<BTreeMap<K, V>>,
    map: BTreeMap<K, V>,
    key: Option<K>,
    value: Option<V>,
}

impl<'a, K, V> BTreeMapSink<'a, K, V>
where
    K: Ord,
{
    fn flush(&mut self) {
        if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
            self.map.insert(key, value);
        }
    }
}

impl<'a, K, V> MapSink for BTreeMapSink<'a, K, V>
where
    K: Ord + Deserializable,
    V: Deserializable,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeMap" };
        &DESCRIPTOR
    }

    fn key(&mut self) -> Result<SinkRef, Error> {
        self.flush();
        Ok(Deserializable::attach(&mut self.key))
    }

    fn value(&mut self) -> Result<SinkRef, Error> {
        Ok(Deserializable::attach(&mut self.value))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.flush();
        *self.slot = Some(take(&mut self.map));
        Ok(())
    }
}

impl<K, V, H> Sink for SlotWrapper<HashMap<K, V, H>>
where
    K: Hash + Eq + Deserializable,
    V: Deserializable,
    H: BuildHasher + Default,
{
    fn expecting(&self) -> Cow<'_, str> {
        "map".into()
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<Box<dyn super::MapSink + '_>, Error> {
        Ok(Box::new(HashMapSink {
            slot: self,
            map: HashMap::default(),
            key: None,
            value: None,
        }))
    }
}

impl<K, V, H> Deserializable for HashMap<K, V, H>
where
    K: Hash + Eq + Deserializable,
    V: Deserializable,
    H: BuildHasher + Default,
{
    fn attach(out: &mut Option<Self>) -> SinkRef {
        SinkRef::Borrowed(SlotWrapper::wrap(out))
    }
}

struct HashMapSink<'a, K: 'a, V: 'a, H> {
    slot: &'a mut Option<HashMap<K, V, H>>,
    map: HashMap<K, V, H>,
    key: Option<K>,
    value: Option<V>,
}

impl<'a, K, V, H> HashMapSink<'a, K, V, H>
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

impl<'a, K, V, H> MapSink for HashMapSink<'a, K, V, H>
where
    K: Hash + Eq + Deserializable,
    V: Deserializable,
    H: BuildHasher + Default,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashMap" };
        &DESCRIPTOR
    }

    fn key(&mut self) -> Result<SinkRef, Error> {
        self.flush();
        Ok(Deserializable::attach(&mut self.key))
    }

    fn value(&mut self) -> Result<SinkRef, Error> {
        Ok(Deserializable::attach(&mut self.value))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.flush();
        *self.slot = Some(take(&mut self.map));
        Ok(())
    }
}

struct VecSink<'a, T: 'a> {
    slot: &'a mut Option<Vec<T>>,
    vec: Vec<T>,
    element: Option<T>,
}

impl<'a, T: 'a> VecSink<'a, T> {
    fn flush(&mut self) {
        if let Some(element) = self.element.take() {
            self.vec.push(element);
        }
    }
}

impl<'a, T: Deserializable> SeqSink for VecSink<'a, T> {
    fn descriptor(&self) -> &dyn Descriptor {
        static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Vec" };
        static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "ByteVec" };
        if T::__private_byte_slice(&[]).is_some() {
            &BYTES_DESCRIPTOR
        } else {
            &SLICE_DESCRIPTOR
        }
    }

    fn item(&mut self) -> Result<SinkRef, Error> {
        self.flush();
        Ok(Deserializable::attach(&mut self.element))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.flush();
        *self.slot = Some(take(&mut self.vec));
        Ok(())
    }
}
