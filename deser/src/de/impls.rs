use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;
use std::hash::Hash;
use std::mem::take;

use crate::de::{Deserialize, DeserializerState, MapSink, SeqSink, Sink, SinkHandle};
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
    fn expecting(&self) -> Cow<'_, str> {
        "null".into()
    }

    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Null => {
                **self = Some(());
                Ok(())
            }
            other => Err(other.unexpected_error(&self.expecting())),
        }
    }
}
deserialize!(());

impl Sink for SlotWrapper<bool> {
    fn expecting(&self) -> Cow<'_, str> {
        "bool".into()
    }

    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Bool(value) => {
                **self = Some(value);
                Ok(())
            }
            other => Err(other.unexpected_error(&self.expecting())),
        }
    }
}
deserialize!(bool);

impl Sink for SlotWrapper<String> {
    fn expecting(&self) -> Cow<'_, str> {
        "string".into()
    }

    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                **self = Some(value.to_string());
                Ok(())
            }
            other => Err(other.unexpected_error(&self.expecting())),
        }
    }
}
deserialize!(String);

macro_rules! int_sink {
    ($ty:ty) => {
        impl Sink for SlotWrapper<$ty> {
            fn expecting(&self) -> Cow<'_, str> {
                stringify!($ty).into()
            }

            fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
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
                    other => Err(other.unexpected_error(&self.expecting())),
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

    fn __private_byte_slice_to_vec(bytes: &[u8]) -> Option<Vec<Self>>
    where
        Self: Sized,
    {
        Some(bytes.to_vec())
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

macro_rules! float_sink {
    ($ty:ty) => {
        impl Sink for SlotWrapper<$ty> {
            fn expecting(&self) -> Cow<'_, str> {
                stringify!($ty).into()
            }

            fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
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
                    other => Err(other.unexpected_error(&self.expecting())),
                }
            }
        }
    };
}

float_sink!(f32);
deserialize!(f32);

float_sink!(f64);
deserialize!(f64);

impl<T: Deserialize> Sink for SlotWrapper<Vec<T>> {
    fn expecting(&self) -> Cow<'_, str> {
        if T::__private_byte_slice_to_vec(&[][..]).is_some() {
            "bytes".into()
        } else {
            "vec".into()
        }
    }

    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Bytes(value) => {
                if let Some(bytes) = T::__private_byte_slice_to_vec(&value) {
                    **self = Some(bytes);
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::Unexpected,
                        format!("unexpected bytes, expected {}", self.expecting()),
                    ))
                }
            }
            other => Err(other.unexpected_error(&self.expecting())),
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

impl<T: Deserialize> Deserialize for Vec<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        SlotWrapper::make_handle(out)
    }
}

impl<K, V> Sink for SlotWrapper<BTreeMap<K, V>>
where
    K: Ord + Deserialize,
    V: Deserialize,
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

impl<K, V> Deserialize for BTreeMap<K, V>
where
    K: Ord + Deserialize,
    V: Deserialize,
{
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        SlotWrapper::make_handle(out)
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
    K: Ord + Deserialize,
    V: Deserialize,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BTreeMap" };
        &DESCRIPTOR
    }

    fn key(&mut self) -> Result<SinkHandle, Error> {
        self.flush();
        Ok(Deserialize::deserialize_into(&mut self.key))
    }

    fn value(&mut self) -> Result<SinkHandle, Error> {
        Ok(Deserialize::deserialize_into(&mut self.value))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.flush();
        *self.slot = Some(take(&mut self.map));
        Ok(())
    }
}

impl<K, V, H> Sink for SlotWrapper<HashMap<K, V, H>>
where
    K: Hash + Eq + Deserialize,
    V: Deserialize,
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

impl<K, V, H> Deserialize for HashMap<K, V, H>
where
    K: Hash + Eq + Deserialize,
    V: Deserialize,
    H: BuildHasher + Default,
{
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        SlotWrapper::make_handle(out)
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
    K: Hash + Eq + Deserialize,
    V: Deserialize,
    H: BuildHasher + Default,
{
    fn descriptor(&self) -> &dyn Descriptor {
        static DESCRIPTOR: UnorderedNamedDescriptor = UnorderedNamedDescriptor { name: "HashMap" };
        &DESCRIPTOR
    }

    fn key(&mut self) -> Result<SinkHandle, Error> {
        self.flush();
        Ok(Deserialize::deserialize_into(&mut self.key))
    }

    fn value(&mut self) -> Result<SinkHandle, Error> {
        Ok(Deserialize::deserialize_into(&mut self.value))
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

impl<'a, T: Deserialize> SeqSink for VecSink<'a, T> {
    fn descriptor(&self) -> &dyn Descriptor {
        static SLICE_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Vec" };
        static BYTES_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "ByteVec" };
        if T::__private_byte_slice_to_vec(&[]).is_some() {
            &BYTES_DESCRIPTOR
        } else {
            &SLICE_DESCRIPTOR
        }
    }

    fn item(&mut self) -> Result<SinkHandle, Error> {
        self.flush();
        Ok(Deserialize::deserialize_into(&mut self.element))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.flush();
        *self.slot = Some(take(&mut self.vec));
        Ok(())
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

    fn map(&mut self, state: &DeserializerState) -> Result<Box<dyn MapSink + '_>, Error> {
        self.sink.map(state)
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<Box<dyn SeqSink + '_>, Error> {
        self.sink.seq(state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        "optional".into()
    }
}
