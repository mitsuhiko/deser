//! Serialization adapters for the standard containers.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

use crate::State;
use crate::adapters::{SerializeAs, SerializeAsRef};
use crate::error::Error;
use crate::event::{Atom, Bytes, ContainerShape, Order};
use crate::ser::{Begin, Chunk, Describe, IndexedSeq, MapEmitter, SeqEmitter, SerializeHandle};

/// Returns a handle to a value that serializes with an adapter.
#[inline(always)]
fn handle_as<A: SerializeAs<T>, T>(value: &T) -> SerializeHandle<'_> {
    SerializeHandle::to(SerializeAsRef::<A, T>::new(value))
}

/// Emits the elements of an indexed sequence.
struct IndexedSeqEmitter<'a> {
    seq: &'a dyn IndexedSeq,
    index: usize,
}

impl<'a> SeqEmitter for IndexedSeqEmitter<'a> {
    fn next(&mut self, state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        let index = self.index;
        self.index += 1;
        self.seq.element(index, state)
    }
}

/// Emits the elements of an iterator with an adapter.
#[allow(clippy::type_complexity)]
struct IterEmitter<'a, I, A>(I, std::marker::PhantomData<(&'a (), fn() -> A)>);

impl<'a, I, T, A> SeqEmitter for IterEmitter<'a, I, A>
where
    I: Iterator<Item = &'a T>,
    T: 'a,
    A: SerializeAs<T>,
{
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(handle_as::<A, T>))
    }
}

/// Emits the entries of a map iterator with adapters.
struct MapIterEmitter<'a, I, V, KA, VA> {
    iter: I,
    value: Option<&'a V>,
    _marker: std::marker::PhantomData<fn() -> (KA, VA)>,
}

impl<'a, I, K, V, KA, VA> MapEmitter for MapIterEmitter<'a, I, V, KA, VA>
where
    I: Iterator<Item = (&'a K, &'a V)>,
    K: 'a,
    V: 'a,
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.iter.next().map(|(k, v)| {
            self.value = Some(v);
            handle_as::<KA, K>(k)
        }))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        Ok(handle_as::<VA, V>(self.value.unwrap()))
    }
}

impl<T, A: SerializeAs<T>> SerializeAs<Option<T>> for Option<A> {
    fn serialize_as<'a>(value: &'a Option<T>, state: &mut State) -> Result<Chunk<'a>, Error> {
        match value {
            Some(value) => A::serialize_as(value, state),
            None => Ok(Chunk::Atom(Atom::Null)),
        }
    }

    fn finish_as(value: &Option<T>, state: &mut State) -> Result<(), Error> {
        match value {
            Some(value) => A::finish_as(value, state),
            None => Ok(()),
        }
    }

    fn is_optional_as(value: &Option<T>) -> bool {
        value.is_none()
    }

    fn container_shape_as(value: &Option<T>) -> ContainerShape {
        match value {
            Some(value) => A::container_shape_as(value),
            None => ContainerShape::new(),
        }
    }

    fn describe_as(value: &Option<T>, d: &mut dyn Describe) {
        match value {
            Some(value) => {
                d.some();
                A::describe_as(value, d);
            }
            None => d.none(),
        }
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a Option<T>, state: &mut State) -> Result<Begin<'a>, Error> {
        match value {
            Some(value) => A::__private_begin_as(value, state),
            None => Ok(Begin::chunk(
                Chunk::Atom(Atom::Null),
                ContainerShape::new(),
                false,
            )),
        }
    }
}

impl<T, A: SerializeAs<T>> SerializeAs<Box<T>> for Box<A> {
    fn serialize_as<'a>(value: &'a Box<T>, state: &mut State) -> Result<Chunk<'a>, Error> {
        A::serialize_as(value, state)
    }

    fn finish_as(value: &Box<T>, state: &mut State) -> Result<(), Error> {
        A::finish_as(value, state)
    }

    fn is_optional_as(value: &Box<T>) -> bool {
        A::is_optional_as(value)
    }

    fn container_shape_as(value: &Box<T>) -> ContainerShape {
        A::container_shape_as(value)
    }

    fn describe_as(value: &Box<T>, d: &mut dyn Describe) {
        A::describe_as(value, d)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a Box<T>, state: &mut State) -> Result<Begin<'a>, Error> {
        A::__private_begin_as(value, state)
    }
}

impl<T, A: SerializeAs<T>> IndexedSeq for SerializeAsRef<Vec<A>, Vec<T>> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get().get(index).map(handle_as::<A, T>))
    }
}

impl<T, A: SerializeAs<T>> SerializeAs<Vec<T>> for Vec<A> {
    fn serialize_as<'a>(value: &'a Vec<T>, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(match A::__private_slice_as_bytes_as(value) {
            Some(bytes) => Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => Chunk::Seq(Box::new(IndexedSeqEmitter {
                seq: SerializeAsRef::<Vec<A>, Vec<T>>::new(value),
                index: 0,
            })),
        })
    }

    fn container_shape_as(value: &Vec<T>) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a Vec<T>, _state: &mut State) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(match A::__private_slice_as_bytes_as(value) {
            Some(bytes) => Begin::chunk(Chunk::Atom(Atom::Bytes(Bytes::new(bytes))), shape, false),
            None => Begin::indexed_seq(SerializeAsRef::<Vec<A>, Vec<T>>::new(value), shape),
        })
    }
}

impl<T, A: SerializeAs<T>, const N: usize> IndexedSeq for SerializeAsRef<[A; N], [T; N]> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get().get(index).map(handle_as::<A, T>))
    }
}

impl<T, A: SerializeAs<T>, const N: usize> SerializeAs<[T; N]> for [A; N] {
    fn serialize_as<'a>(value: &'a [T; N], _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(match A::__private_slice_as_bytes_as(value) {
            Some(bytes) => Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => Chunk::Seq(Box::new(IndexedSeqEmitter {
                seq: SerializeAsRef::<[A; N], [T; N]>::new(value),
                index: 0,
            })),
        })
    }

    fn container_shape_as(value: &[T; N]) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a [T; N], _state: &mut State) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(match A::__private_slice_as_bytes_as(value) {
            Some(bytes) => Begin::chunk(Chunk::Atom(Atom::Bytes(Bytes::new(bytes))), shape, false),
            None => Begin::indexed_seq(SerializeAsRef::<[A; N], [T; N]>::new(value), shape),
        })
    }
}

impl<T, A: SerializeAs<T>> SerializeAs<[T]> for [A] {
    fn serialize_as<'a>(value: &'a [T], _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(match A::__private_slice_as_bytes_as(value) {
            Some(bytes) => Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => Chunk::Seq(Box::new(IterEmitter::<'_, _, A>(
                value.iter(),
                std::marker::PhantomData,
            ))),
        })
    }

    fn container_shape_as(value: &[T]) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a [T], state: &mut State) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            shape,
            false,
        ))
    }
}

impl<K, V, KA, VA> SerializeAs<BTreeMap<K, V>> for BTreeMap<KA, VA>
where
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(value: &'a BTreeMap<K, V>, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Map(Box::new(MapIterEmitter::<_, V, KA, VA> {
            iter: value.iter(),
            value: None,
            _marker: std::marker::PhantomData,
        })))
    }

    fn container_shape_as(value: &BTreeMap<K, V>) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Sorted)
            .with_len(value.len())
    }

    #[inline]
    fn __private_begin_as<'a>(
        value: &'a BTreeMap<K, V>,
        state: &mut State,
    ) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            shape,
            false,
        ))
    }
}

impl<K, V, H, KA, VA> SerializeAs<HashMap<K, V, H>> for HashMap<KA, VA>
where
    H: BuildHasher,
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(
        value: &'a HashMap<K, V, H>,
        _state: &mut State,
    ) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Map(Box::new(MapIterEmitter::<_, V, KA, VA> {
            iter: value.iter(),
            value: None,
            _marker: std::marker::PhantomData,
        })))
    }

    fn container_shape_as(value: &HashMap<K, V, H>) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Arbitrary)
            .with_len(value.len())
    }

    #[inline]
    fn __private_begin_as<'a>(
        value: &'a HashMap<K, V, H>,
        state: &mut State,
    ) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            shape,
            false,
        ))
    }
}

impl<T, A: SerializeAs<T>> SerializeAs<BTreeSet<T>> for BTreeSet<A> {
    fn serialize_as<'a>(value: &'a BTreeSet<T>, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Seq(Box::new(IterEmitter::<'_, _, A>(
            value.iter(),
            std::marker::PhantomData,
        ))))
    }

    fn container_shape_as(value: &BTreeSet<T>) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Sorted)
            .with_len(value.len())
    }

    fn describe_as(_value: &BTreeSet<T>, d: &mut dyn Describe) {
        d.set();
    }

    #[inline]
    fn __private_begin_as<'a>(
        value: &'a BTreeSet<T>,
        state: &mut State,
    ) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            shape,
            false,
        ))
    }
}

impl<T, H: BuildHasher, A: SerializeAs<T>> SerializeAs<HashSet<T, H>> for HashSet<A> {
    fn serialize_as<'a>(value: &'a HashSet<T, H>, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Seq(Box::new(IterEmitter::<'_, _, A>(
            value.iter(),
            std::marker::PhantomData,
        ))))
    }

    fn container_shape_as(value: &HashSet<T, H>) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Arbitrary)
            .with_len(value.len())
    }

    fn describe_as(_value: &HashSet<T, H>, d: &mut dyn Describe) {
        d.set();
    }

    #[inline]
    fn __private_begin_as<'a>(
        value: &'a HashSet<T, H>,
        state: &mut State,
    ) -> Result<Begin<'a>, Error> {
        let shape = Self::container_shape_as(value);
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            shape,
            false,
        ))
    }
}

/// Counts as one, used to count repetitions.
macro_rules! count_one {
    ($name:ident) => {
        1
    };
}

macro_rules! serialize_as_for_tuple {
    () => ();
    ($(($name:ident, $adapter:ident),)+) => (
        impl<$($name,)* $($adapter: SerializeAs<$name>),*> IndexedSeq
            for SerializeAsRef<($($adapter,)*), ($($name,)*)>
        {
            #[allow(non_snake_case)]
            fn element(&self, index: usize, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
                let ($(ref $name,)*) = *self.get();
                let mut __counter = 0;
                $(
                    if index == __counter {
                        return Ok(Some(handle_as::<$adapter, $name>($name)));
                    }
                    __counter += 1;
                )*
                let _ = __counter;
                Ok(None)
            }
        }

        impl<$($name,)* $($adapter: SerializeAs<$name>),*> SerializeAs<($($name,)*)> for ($($adapter,)*) {
            fn serialize_as<'a>(value: &'a ($($name,)*), _state: &mut State) -> Result<Chunk<'a>, Error> {
                Ok(Chunk::Seq(Box::new(IndexedSeqEmitter {
                    seq: SerializeAsRef::<($($adapter,)*), ($($name,)*)>::new(value),
                    index: 0,
                })))
            }

            fn describe_as(_value: &($($name,)*), d: &mut dyn Describe) {
                d.tuple();
            }

            fn container_shape_as(_value: &($($name,)*)) -> ContainerShape {
                ContainerShape::new().with_len(0 $(+ count_one!($name))*)
            }

            #[inline]
            fn __private_begin_as<'a>(value: &'a ($($name,)*), _state: &mut State) -> Result<Begin<'a>, Error> {
                Ok(Begin::indexed_seq(
                    SerializeAsRef::<($($adapter,)*), ($($name,)*)>::new(value),
                    Self::container_shape_as(value),
                ))
            }
        }

        serialize_as_for_tuple_peel!($(($name, $adapter),)*);
    )
}

macro_rules! serialize_as_for_tuple_peel {
    ($first:tt, $($other:tt,)*) => (serialize_as_for_tuple!($($other,)*);)
}

serialize_as_for_tuple! {
    (T1, A1), (T2, A2), (T3, A3), (T4, A4), (T5, A5), (T6, A6),
    (T7, A7), (T8, A8), (T9, A9), (T10, A10), (T11, A11), (T12, A12),
}
