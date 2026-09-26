use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::BuildHasher;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::State;
use crate::error::Error;
use crate::event::{Atom, Bytes, ContainerShape, Order};
use crate::ext::ExtValue;
use crate::ser::{
    Begin, Chunk, Describe, IndexedSeq, MapEmitter, SeqEmitter, Serialize, SerializeHandle,
};

impl Serialize for bool {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Bool(*self)))
    }
}

impl Serialize for () {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Null))
    }

    fn is_optional(&self) -> bool {
        true
    }
}

impl Serialize for u8 {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::U64(*self as u64)))
    }

    fn __private_slice_as_bytes(val: &[u8]) -> Option<Cow<'_, [u8]>> {
        Some(Cow::Borrowed(val))
    }
}

impl Serialize for char {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Char(*self)))
    }
}

macro_rules! serialize_int {
    ($ty:ty, $atom:ident) => {
        impl Serialize for $ty {
            __begin_without_finish!();

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

impl Serialize for f32 {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::F64(f64::from(*self))))
    }
}

impl Serialize for f64 {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::F64(*self)))
    }
}

macro_rules! serialize_ext_int {
    ($ty:ty) => {
        impl Serialize for $ty {
            __begin_without_finish!();

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

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str(self.as_str().into())))
    }
}

impl Serialize for str {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str(Cow::Borrowed(self))))
    }
}

/// `Cow<[T]>` is implemented separately as slices are serialized by the
/// containers holding them (see `serialize_slice`).
impl<'a, T> Serialize for Cow<'a, T>
where
    T: Serialize + ToOwned + ?Sized,
    T::Owned: Sync,
{
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

    fn container_shape(&self) -> ContainerShape {
        Serialize::container_shape(&**self)
    }

    fn describe(&self, d: &mut dyn Describe) {
        Serialize::describe(&**self, d)
    }
}

/// Implements `Serialize` for the containers of slices.
///
/// `[T]` itself does not implement `Serialize` as the containers provide
/// the elements by index which requires a sized value.
macro_rules! serialize_slice {
    ($([$($gen:tt)*] $ty:ty),* $(,)?) => {
        $(
            impl<$($gen)*> Serialize for $ty {
                #[inline]
                fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
                    Ok(match T::__private_slice_as_bytes(&self[..]) {
                        Some(bytes) => Begin::chunk(
                            Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
                            ContainerShape::new(),
                            false,
                        ),
                        None => Begin::indexed_seq(self, self.container_shape()),
                    })
                }

                fn container_shape(&self) -> ContainerShape {
                    ContainerShape::new().with_len(self.len())
                }

                fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                    if let Some(bytes) = T::__private_slice_as_bytes(&self[..]) {
                        Ok(Chunk::Atom(Atom::Bytes(Bytes::new(bytes))))
                    } else {
                        Ok(Chunk::Seq(Box::new(SliceEmitter(self[..].iter()))))
                    }
                }
            }

            impl<$($gen)*> IndexedSeq for $ty {
                #[inline]
                fn element(
                    &self,
                    index: usize,
                    _state: &mut State,
                ) -> Result<Option<SerializeHandle<'_>>, Error> {
                    Ok(self[..].get(index).map(SerializeHandle::to))
                }
            }
        )*
    };
}

serialize_slice!(
    [T: Serialize] Vec<T>,
    ['a, T: Serialize] &'a [T],
    [T: Serialize] Box<[T]>,
    [T: Serialize + Send] Arc<[T]>,
    ['a, T: Serialize + Clone] Cow<'a, [T]>,
);

impl<T: Serialize, const N: usize> IndexedSeq for [T; N] {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get(index).map(SerializeHandle::to))
    }
}

struct SliceEmitter<'a, T>(std::slice::Iter<'a, T>);

impl<'a, T: Serialize> SeqEmitter for SliceEmitter<'a, T> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

/// Emits the elements of an iterator.
struct IterEmitter<'a, I>(I, PhantomData<&'a ()>);

impl<'a, I, T> SeqEmitter for IterEmitter<'a, I>
where
    I: Iterator<Item = &'a T> + Send,
    T: Serialize + 'a,
{
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

impl<T: Serialize> Serialize for VecDeque<T> {
    #[inline]
    fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
        Ok(match self.as_bytes() {
            Some(bytes) => Begin::chunk(
                Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
                ContainerShape::new(),
                false,
            ),
            None => Begin::indexed_seq(self, self.container_shape()),
        })
    }

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new().with_len(self.len())
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(match self.as_bytes() {
            Some(bytes) => Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => Chunk::Seq(Box::new(IterEmitter(self.iter(), PhantomData))),
        })
    }
}

impl<T: Serialize> IndexedSeq for VecDeque<T> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get(index).map(SerializeHandle::to))
    }
}

/// Returns the bytes of a deque of `u8`.
trait DequeBytes {
    fn as_bytes(&self) -> Option<Cow<'_, [u8]>>;
}

impl<T: Serialize> DequeBytes for VecDeque<T> {
    fn as_bytes(&self) -> Option<Cow<'_, [u8]>> {
        let (front, back) = self.as_slices();
        let front = T::__private_slice_as_bytes(front)?;
        if back.is_empty() {
            return Some(front);
        }
        let back = T::__private_slice_as_bytes(back)?;
        let mut rv = front.into_owned();
        rv.extend_from_slice(&back);
        Some(Cow::Owned(rv))
    }
}

impl<T: Serialize> Serialize for LinkedList<T> {
    __begin_without_finish!();

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new().with_len(self.len())
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Seq(Box::new(IterEmitter(self.iter(), PhantomData))))
    }
}

/// The elements are emitted in the (arbitrary) order of the heap.
impl<T: Serialize> Serialize for BinaryHeap<T> {
    #[inline]
    fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
        Ok(match T::__private_slice_as_bytes(self.as_slice()) {
            Some(bytes) => Begin::chunk(
                Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
                ContainerShape::new(),
                false,
            ),
            None => Begin::indexed_seq(self, self.container_shape()),
        })
    }

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new().with_len(self.len())
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(match T::__private_slice_as_bytes(self.as_slice()) {
            Some(bytes) => Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => Chunk::Seq(Box::new(SliceEmitter(self.as_slice().iter()))),
        })
    }
}

impl<T: Serialize> IndexedSeq for BinaryHeap<T> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.as_slice().get(index).map(SerializeHandle::to))
    }
}

impl<K, V> Serialize for BTreeMap<K, V>
where
    K: Serialize,
    V: Serialize,
{
    __begin_without_finish!();

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Sorted)
            .with_len(self.len())
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
    H: Sync,
    H: BuildHasher,
{
    __begin_without_finish!();

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Arbitrary)
            .with_len(self.len())
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

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Sorted)
            .with_len(self.len())
    }

    fn describe(&self, d: &mut dyn Describe) {
        d.set();
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

impl<T, H> Serialize for HashSet<T, H>
where
    T: Serialize,
    H: BuildHasher + Sync,
{
    __begin_without_finish!();

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
            .with_order(Order::Arbitrary)
            .with_len(self.len())
    }

    fn describe(&self, d: &mut dyn Describe) {
        d.set();
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
    fn is_optional(&self) -> bool {
        self.is_none()
    }

    fn container_shape(&self) -> ContainerShape {
        match self {
            Some(value) => value.container_shape(),
            None => ContainerShape::new(),
        }
    }

    fn describe(&self, d: &mut dyn Describe) {
        match self {
            Some(value) => {
                d.some();
                value.describe(d);
            }
            None => d.none(),
        }
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
                ContainerShape::new(),
                false,
            )),
        }
    }
}

/// Counts as one, used to count repetitions.
macro_rules! count_one {
    ($name:ident) => {
        1
    };
}

macro_rules! serialize_for_tuple {
    () => ();
    ($($name:ident,)+) => (
        impl<$($name: Serialize),*> Serialize for ($($name,)*) {
            #[inline]
            fn __private_begin(&self, _state: &mut State) -> Result<Begin<'_>, Error> {
                Ok(Begin::indexed_seq(self, self.container_shape()))
            }

            fn container_shape(&self) -> ContainerShape {
                ContainerShape::new().with_len(0 $(+ count_one!($name))*)
            }

            fn describe(&self, d: &mut dyn Describe) {
                d.tuple();
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
                        let ($($name,)*) = self.tuple;
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
                let ($($name,)*) = self;
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
        Ok(match T::__private_slice_as_bytes(&self[..]) {
            Some(bytes) => Begin::chunk(
                Chunk::Atom(Atom::Bytes(Bytes::new(bytes))),
                ContainerShape::new(),
                false,
            ),
            None => Begin::indexed_seq(self, self.container_shape()),
        })
    }

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new().with_len(self.len())
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(bytes) = T::__private_slice_as_bytes(self) {
            Ok(Chunk::Atom(Atom::Bytes(Bytes::new(bytes))))
        } else {
            Ok(Chunk::Seq(Box::new(SliceEmitter(self.iter()))))
        }
    }
}

macro_rules! forward_serialize {
    ($([$($bound:tt)*] $ty:ty),*) => {
        $(
            impl<'a, T: Serialize + $($bound)* ?Sized> Serialize for $ty {
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

                fn container_shape(&self) -> ContainerShape {
                    Serialize::container_shape(&**self)
                }

                fn describe(&self, d: &mut dyn Describe) {
                    Serialize::describe(&**self, d)
                }

            }
        )*
    };
}

forward_serialize!([] &'a T, [] &'a mut T, [] Box<T>, [Send +] Arc<T>);
