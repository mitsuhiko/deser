use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::BuildHasher;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::State;
use crate::error::Error;
use crate::event::{Atom, Bytes, ContainerShape};
use crate::ext::ExtValue;
use crate::ser::{
    Begin, Chunk, Describe, IndexedSeq, MapEmitter, PlainSink, SeqEmitter, Serialize,
    SerializeHandle, plain_atom,
};

impl Serialize for bool {
    begin_without_finish!();
    plain_atom!(|v| Atom::Bool(*v));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Bool(*self)))
    }
}

impl Serialize for () {
    begin_without_finish!();
    plain_atom!(|_v| Atom::Null);

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Null))
    }

    fn is_optional(&self) -> bool {
        true
    }
}

impl Serialize for u8 {
    begin_without_finish!();
    plain_atom!(|v| Atom::U64(u64::from(*v)));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::U64(*self as u64)))
    }

    fn __private_slice_as_bytes(val: &[u8]) -> Option<Cow<'_, [u8]>> {
        Some(Cow::Borrowed(val))
    }
}

impl Serialize for char {
    begin_without_finish!();
    plain_atom!(|v| Atom::Char(*v));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Char(*self)))
    }
}

macro_rules! serialize_int {
    ($ty:ty, $atom:ident) => {
        impl Serialize for $ty {
            begin_without_finish!();
            plain_atom!(|v| Atom::$atom(*v as _));

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
    begin_without_finish!();
    plain_atom!(|v| Atom::F32(*v));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::F32(*self)))
    }
}

impl Serialize for f64 {
    begin_without_finish!();
    plain_atom!(|v| Atom::F64(*v));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::F64(*self)))
    }
}

macro_rules! serialize_ext_int {
    ($ty:ty) => {
        impl Serialize for $ty {
            begin_without_finish!();
            plain_atom!(|v| Atom::Ext(ExtValue::borrowed(v)));

            fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
            }
        }
    };
}

serialize_ext_int!(u128);
serialize_ext_int!(i128);

impl Serialize for String {
    begin_without_finish!();
    plain_atom!(|v| Atom::Str(Cow::Borrowed(v.as_str())));

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Str(self.as_str().into())))
    }
}

impl Serialize for str {
    begin_without_finish!();

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
/// the elements by index which requires a sized value.  The containers
/// need to support `len` and indexing with `[..]`.
macro_rules! serialize_slice {
    ($([$($gen:tt)*] $ty:ty),* $(,)?) => {
        $(
            impl<$($gen)*> $crate::ser::Serialize for $ty {
                #[inline]
                fn __private_begin(
                    &self,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'_>, $crate::Error> {
                    Ok(match T::__private_slice_as_bytes(&self[..]) {
                        Some(bytes) => $crate::ser::Begin::chunk(
                            $crate::ser::Chunk::Atom($crate::Atom::Bytes($crate::Bytes::new(bytes))),
                            $crate::ContainerShape::new(),
                            false,
                        ),
                        None => $crate::ser::Begin::indexed_seq(self, self.container_shape()),
                    })
                }

                fn container_shape(&self) -> $crate::ContainerShape {
                    $crate::ContainerShape::new().with_len(self.len())
                }

                fn serialize(
                    &self,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Chunk<'_>, $crate::Error> {
                    if let Some(bytes) = T::__private_slice_as_bytes(&self[..]) {
                        Ok($crate::ser::Chunk::Atom($crate::Atom::Bytes($crate::Bytes::new(bytes))))
                    } else {
                        Ok($crate::ser::Chunk::Seq(Box::new(
                            $crate::ser::impls::SliceEmitter(self[..].iter()),
                        )))
                    }
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    T::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(&self) -> bool {
                    T::__private_is_plain() || self.is_empty()
                }

                fn __private_emit_plain(
                    &self,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<(), $crate::Error> {
                    $crate::ser::impls::emit_plain_slice(&self[..], self.container_shape(), sink)
                }
            }

            impl<$($gen)*> $crate::ser::IndexedSeq for $ty {
                #[inline]
                fn element(
                    &self,
                    index: usize,
                    _state: &mut $crate::State,
                ) -> Result<Option<$crate::ser::SerializeHandle<'_>>, $crate::Error> {
                    Ok(self[..].get(index).map($crate::ser::SerializeHandle::to))
                }

                fn emit_plain(
                    &self,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<bool, $crate::Error> {
                    $crate::ser::impls::emit_plain_elements(self[..].iter(), sink)
                }
            }
        )*
    };
}

// also used for the containers of other crates
#[allow(unused_imports)]
pub(crate) use serialize_slice;

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

    fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
        emit_plain_elements(self.iter(), sink)
    }
}

/// Emits a slice of plain values, as bytes or as sequence.
#[inline]
pub(crate) fn emit_plain_slice<T: Serialize>(
    slice: &[T],
    shape: ContainerShape,
    sink: &mut dyn PlainSink,
) -> Result<(), Error> {
    match T::__private_slice_as_bytes(slice) {
        Some(bytes) => sink.atom(Atom::Bytes(Bytes::new(bytes))),
        None => {
            sink.seq_start(shape)?;
            for value in slice {
                value.__private_emit_plain(sink)?;
            }
            sink.seq_end()
        }
    }
}

/// Emits the elements of a sequence if they are plain (or if there are
/// none).
#[inline]
pub(crate) fn emit_plain_elements<'a, T: Serialize + 'a>(
    values: impl ExactSizeIterator<Item = &'a T>,
    sink: &mut dyn PlainSink,
) -> Result<bool, Error> {
    if !T::__private_is_plain() && values.len() > 0 {
        return Ok(false);
    }
    for value in values {
        value.__private_emit_plain(sink)?;
    }
    Ok(true)
}

pub(crate) struct SliceEmitter<'a, T>(pub(crate) std::slice::Iter<'a, T>);

impl<'a, T: Serialize> SeqEmitter for SliceEmitter<'a, T> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

/// Emits the elements of an iterator.
pub(crate) struct IterEmitter<'a, I>(pub(crate) I, pub(crate) PhantomData<&'a ()>);

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

    #[inline]
    fn __private_is_plain() -> bool {
        T::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(&self) -> bool {
        T::__private_is_plain() || self.is_empty()
    }

    fn __private_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        match self.as_bytes() {
            Some(bytes) => sink.atom(Atom::Bytes(Bytes::new(bytes))),
            None => {
                sink.seq_start(self.container_shape())?;
                emit_plain_elements(self.iter(), sink)?;
                sink.seq_end()
            }
        }
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

    fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
        emit_plain_elements(self.iter(), sink)
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
    begin_without_finish!();

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

/// Emits the entries of a map iterator.
pub(crate) struct MapIterEmitter<'a, I, V> {
    pub(crate) iter: I,
    pub(crate) value: Option<&'a V>,
}

impl<'a, I, K, V> MapEmitter for MapIterEmitter<'a, I, V>
where
    I: Iterator<Item = (&'a K, &'a V)> + Send,
    K: Serialize + 'a,
    V: Serialize + 'a,
{
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.iter.next().map(|(k, v)| {
            self.value = Some(v);
            SerializeHandle::to(k)
        }))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        Ok(SerializeHandle::to(self.value.unwrap()))
    }
}

/// Implements `Serialize` for maps.
///
/// The maps need to support `len`, `is_empty` and `iter`.
macro_rules! serialize_map {
    ($([$($gen:tt)*] $ty:ty => $order:ident;)*) => {
        $(
            impl<$($gen)*> $crate::ser::Serialize for $ty
            where
                K: $crate::ser::Serialize,
                V: $crate::ser::Serialize,
            {
                #[inline]
                fn __private_begin(
                    &self,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'_>, $crate::Error> {
                    let shape = self.container_shape();
                    if $crate::ser::Serialize::__private_is_plain_value(self) {
                        Ok($crate::ser::Begin::plain(self, shape))
                    } else {
                        Ok($crate::ser::Begin::chunk(self.serialize(state)?, shape, false))
                    }
                }

                fn container_shape(&self) -> $crate::ContainerShape {
                    $crate::ContainerShape::new()
                        .with_order($crate::Order::$order)
                        .with_len(self.len())
                }

                fn serialize(
                    &self,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Chunk<'_>, $crate::Error> {
                    Ok($crate::ser::Chunk::Map(Box::new(
                        $crate::ser::impls::MapIterEmitter {
                            iter: self.iter(),
                            value: None,
                        },
                    )))
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    K::__private_is_plain() && V::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(&self) -> bool {
                    (K::__private_is_plain() && V::__private_is_plain()) || self.is_empty()
                }

                fn __private_emit_plain(
                    &self,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<(), $crate::Error> {
                    sink.map_start(self.container_shape())?;
                    for (key, value) in self.iter() {
                        sink.key();
                        key.__private_emit_plain(sink)?;
                        value.__private_emit_plain(sink)?;
                    }
                    sink.map_end()
                }
            }
        )*
    };
}

// also used for the containers of other crates
#[allow(unused_imports)]
pub(crate) use serialize_map;

serialize_map! {
    [K, V] BTreeMap<K, V> => Sorted;
    [K, V, H: BuildHasher + Sync] HashMap<K, V, H> => Arbitrary;
}

/// Implements `Serialize` for sets.
///
/// The sets need to support `len`, `is_empty` and `iter`.
macro_rules! serialize_set {
    ($([$($gen:tt)*] $ty:ty => $order:ident;)*) => {
        $(
            impl<$($gen)*> $crate::ser::Serialize for $ty
            where
                T: $crate::ser::Serialize,
            {
                #[inline]
                fn __private_begin(
                    &self,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'_>, $crate::Error> {
                    let shape = self.container_shape();
                    if $crate::ser::Serialize::__private_is_plain_value(self) {
                        Ok($crate::ser::Begin::plain(self, shape))
                    } else {
                        Ok($crate::ser::Begin::chunk(self.serialize(state)?, shape, false))
                    }
                }

                fn container_shape(&self) -> $crate::ContainerShape {
                    $crate::ContainerShape::new()
                        .with_order($crate::Order::$order)
                        .with_len(self.len())
                }

                fn describe(&self, d: &mut dyn $crate::ser::Describe) {
                    d.set();
                }

                fn serialize(
                    &self,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Chunk<'_>, $crate::Error> {
                    Ok($crate::ser::Chunk::Seq(Box::new(
                        $crate::ser::impls::IterEmitter(self.iter(), std::marker::PhantomData),
                    )))
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    T::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(&self) -> bool {
                    T::__private_is_plain() || self.is_empty()
                }

                fn __private_emit_plain(
                    &self,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<(), $crate::Error> {
                    sink.seq_start(self.container_shape())?;
                    for value in self.iter() {
                        value.__private_emit_plain(sink)?;
                    }
                    sink.seq_end()
                }
            }
        )*
    };
}

// also used for the containers of other crates
#[allow(unused_imports)]
pub(crate) use serialize_set;

serialize_set! {
    [T] BTreeSet<T> => Sorted;
    [T, H: BuildHasher + Sync] HashSet<T, H> => Arbitrary;
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

    #[inline]
    fn __private_is_plain() -> bool {
        T::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(&self) -> bool {
        match self {
            Some(value) => value.__private_is_plain_value(),
            None => true,
        }
    }

    #[inline]
    fn __private_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        match self {
            Some(value) => value.__private_emit_plain(sink),
            None => sink.atom(Atom::Null),
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

            #[inline]
            fn __private_is_plain() -> bool {
                true $(&& $name::__private_is_plain())*
            }

            #[allow(non_snake_case)]
            fn __private_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
                let ($($name,)*) = self;
                sink.seq_start(self.container_shape())?;
                $($name.__private_emit_plain(sink)?;)*
                sink.seq_end()
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

            #[allow(non_snake_case)]
            fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
                if !<Self as Serialize>::__private_is_plain() {
                    return Ok(false);
                }
                let ($($name,)*) = self;
                $($name.__private_emit_plain(sink)?;)*
                Ok(true)
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

    #[inline]
    fn __private_is_plain() -> bool {
        T::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(&self) -> bool {
        T::__private_is_plain() || N == 0
    }

    fn __private_emit_plain(&self, sink: &mut dyn PlainSink) -> Result<(), Error> {
        emit_plain_slice(self, self.container_shape(), sink)
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
