//! `Serialize` for the standard types.
//!
//! The containers are generic over the adapters of their values (see
//! `crate::adapters`), the container of `T` itself is the adapter with
//! `A = T`.
use alloc::borrow::Cow;
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap, LinkedList, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::hash::BuildHasher;
use core::marker::PhantomData;
#[cfg(feature = "std")]
use std::collections::{HashMap, HashSet};

use crate::State;
use crate::Text;
use crate::error::Error;
use crate::event::{Atom, Bytes, ContainerShape};
use crate::ext::ExtValue;
use crate::ser::{
    Adapted, Begin, Describe, Emit, IndexedSeq, MapEmitter, PlainSink, SeqEmitter, Serialize,
    SerializeHandle, SerializeRef, atom_cost, plain_atom,
};

impl Serialize for bool {
    begin_without_finish!();
    plain_atom!(|v| Atom::Bool(*v));

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Bool(*value)))
    }
}

impl Serialize for () {
    begin_without_finish!();
    plain_atom!(|_v| Atom::Null);

    fn serialize<'a>(_value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Null))
    }

    fn is_optional(_value: &Self) -> bool {
        true
    }
}

impl Serialize for u8 {
    begin_without_finish!();
    plain_atom!(|v| Atom::U64(u64::from(*v)));

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::U64(*value as u64)))
    }

    fn __private_slice_as_bytes(val: &[u8]) -> Option<Cow<'_, [u8]>> {
        Some(Cow::Borrowed(val))
    }
}

impl Serialize for char {
    begin_without_finish!();
    plain_atom!(|v| Atom::Char(*v));

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Char(*value)))
    }
}

macro_rules! serialize_int {
    ($ty:ty, $atom:ident) => {
        impl Serialize for $ty {
            begin_without_finish!();
            plain_atom!(|v| Atom::$atom(*v as _));

            fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
                Ok(Emit::Atom(Atom::$atom(*value as _)))
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

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::F32(*value)))
    }
}

impl Serialize for f64 {
    begin_without_finish!();
    plain_atom!(|v| Atom::F64(*v));

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::F64(*value)))
    }
}

macro_rules! serialize_ext_int {
    ($ty:ty) => {
        impl Serialize for $ty {
            begin_without_finish!();
            plain_atom!(|v| Atom::Ext(ExtValue::borrowed(v)));

            fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
                Ok(Emit::Atom(Atom::Ext(ExtValue::borrowed(value))))
            }
        }
    };
}

serialize_ext_int!(u128);
serialize_ext_int!(i128);

impl Serialize for String {
    begin_without_finish!();
    plain_atom!(|v| Atom::Str(Text::borrowed(v.as_str())));

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Str(value.as_str().into())))
    }
}

impl Serialize for str {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Str(Text::borrowed(value))))
    }
}

/// `Cow<[T]>` is implemented separately as slices are serialized by the
/// containers holding them (see `serialize_slice`).
impl<'a, T> Serialize for Cow<'a, T>
where
    T: Serialize + ToOwned + ?Sized,
    T::Owned: Sync,
{
    fn serialize<'b>(value: &'b Self, state: &mut State) -> Result<Emit<'b>, Error> {
        T::serialize(value, state)
    }

    fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
        T::finish(value, state)
    }

    #[inline]
    fn __private_begin<'b>(value: &'b Self, state: &mut State) -> Result<Begin<'b>, Error> {
        T::__private_begin(value, state)
    }

    fn is_optional(value: &Self) -> bool {
        T::is_optional(value)
    }

    fn container_shape(value: &Self) -> ContainerShape {
        T::container_shape(value)
    }

    fn describe(value: &Self, d: &mut dyn Describe) {
        T::describe(value, d)
    }
}

/// Returns a handle to a value that serializes with an adapter.
#[inline(always)]
pub(crate) fn handle_as<A: Serialize<T>, T: Sync>(value: &T) -> SerializeHandle<'_> {
    SerializeHandle::from(SerializeRef::serialize_as::<A, T>(value))
}

/// Emits the elements of an iterator with an adapter.
#[allow(clippy::type_complexity)]
pub(crate) struct IterEmitter<'a, I, A>(I, PhantomData<(&'a (), fn() -> A)>);

impl<'a, I, A> IterEmitter<'a, I, A> {
    /// Emits a sequence of the elements.
    ///
    /// The adapter does not need to outlive the `Emit` (see
    /// `Emit::seq_unbounded`).
    #[inline(always)]
    pub(crate) fn emit<T>(iter: I, state: &mut State) -> Emit<'a>
    where
        I: Iterator<Item = &'a T> + Send + 'a,
        T: Sync + 'a,
        A: Serialize<T>,
    {
        // SAFETY: the emitter only holds a marker of the adapter
        unsafe { Emit::seq_unbounded(IterEmitter::<'a, I, A>(iter, PhantomData), state) }
    }
}

impl<'a, I, T, A> SeqEmitter for IterEmitter<'a, I, A>
where
    I: Iterator<Item = &'a T> + Send,
    T: Sync + 'a,
    A: Serialize<T>,
{
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(handle_as::<A, T>))
    }
}

/// Emits the entries of a map iterator with adapters.
pub(crate) struct MapIterEmitter<'a, I, V, KA, VA> {
    iter: I,
    value: Option<&'a V>,
    _marker: PhantomData<fn() -> (KA, VA)>,
}

impl<'a, I, V, KA, VA> MapIterEmitter<'a, I, V, KA, VA> {
    /// Emits a map of the entries.
    ///
    /// The adapters do not need to outlive the `Emit` (see
    /// `Emit::map_unbounded`).
    #[inline(always)]
    pub(crate) fn emit<K>(iter: I, state: &mut State) -> Emit<'a>
    where
        I: Iterator<Item = (&'a K, &'a V)> + Send + 'a,
        K: Sync + 'a,
        V: Sync + 'a,
        KA: Serialize<K>,
        VA: Serialize<V>,
    {
        let emitter = MapIterEmitter::<I, V, KA, VA> {
            iter,
            value: None,
            _marker: PhantomData,
        };
        // SAFETY: the emitter only holds a marker of the adapters
        unsafe { Emit::map_unbounded(emitter, state) }
    }
}

impl<'a, I, K, V, KA, VA> MapEmitter for MapIterEmitter<'a, I, V, KA, VA>
where
    I: Iterator<Item = (&'a K, &'a V)> + Send,
    K: Sync + 'a,
    V: Sync + 'a,
    KA: Serialize<K>,
    VA: Serialize<V>,
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

/// Begins a sequence which provides its elements by index (see
/// [`Adapted`]).
///
/// The adapter does not need to outlive the value (see
/// `Begin::indexed_seq_unbounded`).
#[inline(always)]
pub(crate) fn begin_indexed<'a, A, T>(value: &'a T, shape: ContainerShape) -> Begin<'a>
where
    Adapted<A, T>: IndexedSeq,
{
    // SAFETY: the wrapper is valid for 'a (it's the value), it only holds a
    // marker of the adapter
    unsafe { Begin::indexed_seq_unbounded(Adapted::<A, T>::ptr(value), shape) }
}

/// Begins a plain value with an adapter (see [`Begin::plain`]).
#[inline(always)]
pub(crate) fn begin_plain<'a, A: Serialize<T>, T: Sync>(
    value: &'a T,
    shape: ContainerShape,
) -> Begin<'a> {
    Begin::plain(SerializeRef::serialize_as::<A, T>(value), shape)
}

/// Implements `Serialize` for the containers of slices, generic over the
/// adapter of the elements.
///
/// `[T]` itself does not implement `Serialize` as the containers provide
/// the elements by index which requires a sized value.  The containers
/// need to support `len` and indexing with `[..]`.  The entries are
/// `[generics] Container<T> => Container<A>, A;` where the last part is the
/// adapter of the elements.
macro_rules! serialize_slice {
    ($([$($gen:tt)*] $ty:ty => $adapter:ty, $elem:ty;)*) => {
        $(
            impl<$($gen)*> $crate::ser::Serialize<$ty> for $adapter {
                #[inline]
                fn __private_begin<'a>(
                    value: &'a $ty,
                    _state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'a>, $crate::Error> {
                    Ok(match <$elem as $crate::ser::Serialize<T>>::__private_slice_as_bytes(&value[..]) {
                        Some(bytes) => $crate::ser::Begin::emit(
                            $crate::ser::Emit::Atom($crate::Atom::Bytes($crate::Bytes::new(bytes))),
                            $crate::ContainerShape::new(),
                            false,
                        ),
                        None => $crate::ser::impls::begin_indexed::<$adapter, $ty>(
                            value,
                            Self::container_shape(value),
                        ),
                    })
                }

                fn container_shape(value: &$ty) -> $crate::ContainerShape {
                    $crate::ContainerShape::new().with_len(value.len())
                }

                fn serialize<'a>(
                    value: &'a $ty,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Emit<'a>, $crate::Error> {
                    Ok(match <$elem as $crate::ser::Serialize<T>>::__private_slice_as_bytes(&value[..]) {
                        Some(bytes) => {
                            $crate::ser::Emit::Atom($crate::Atom::Bytes($crate::Bytes::new(bytes)))
                        }
                        None => $crate::ser::impls::IterEmitter::<_, $elem>::emit(value[..].iter(), state),
                    })
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    <$elem as $crate::ser::Serialize<T>>::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(value: &$ty) -> bool {
                    <$elem as $crate::ser::Serialize<T>>::__private_is_plain() || value.is_empty()
                }

                fn __private_emit_plain(
                    value: &$ty,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<(), $crate::Error> {
                    $crate::ser::impls::emit_plain_slice::<T, $elem>(
                        &value[..],
                        Self::container_shape(value),
                        sink,
                    )
                }

                #[inline]
                fn __private_plain_cost(value: &$ty, budget: usize) -> Option<usize> {
                    $crate::ser::impls::plain_cost_slice::<T, $elem>(&value[..], budget)
                }
            }

            impl<$($gen)*> $crate::ser::IndexedSeq for $crate::ser::Adapted<$adapter, $ty> {
                #[inline]
                fn element(
                    &self,
                    index: usize,
                    _state: &mut $crate::State,
                ) -> Result<Option<$crate::ser::SerializeHandle<'_>>, $crate::Error> {
                    Ok(self.get()[..]
                        .get(index)
                        .map($crate::ser::impls::handle_as::<$elem, T>))
                }

                fn emit_plain(
                    &self,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<bool, $crate::Error> {
                    $crate::ser::impls::emit_plain_elements::<T, $elem>(self.get()[..].iter(), sink)
                }

                fn emit_plain_chunk(
                    &self,
                    index: usize,
                    budget: usize,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<usize, $crate::Error> {
                    $crate::ser::impls::emit_plain_chunk::<T, $elem>(
                        self.get()[..].get(index..).unwrap_or_default().iter(),
                        index,
                        budget,
                        sink,
                    )
                }
            }
        )*
    };
}

// also used for the containers of other crates
#[allow(unused_imports)]
pub(crate) use serialize_slice;

serialize_slice! {
    [T: Sync, A: Serialize<T>] Vec<T> => Vec<A>, A;
    ['r, 's, T: Sync, A: Serialize<T>] &'r [T] => &'s [A], A;
    [T: Sync, A: Serialize<T>] Box<[T]> => Box<[A]>, A;
    [T: Send + Sync, A: Serialize<T> + Send] Arc<[T]> => Arc<[A]>, A;
    ['r, T: Serialize + Clone] Cow<'r, [T]> => Cow<'r, [T]>, T;
}

/// Emits a slice of plain values, as bytes or as sequence.
#[inline]
pub(crate) fn emit_plain_slice<T, A: Serialize<T>>(
    slice: &[T],
    shape: ContainerShape,
    sink: &mut dyn PlainSink,
) -> Result<(), Error> {
    match A::__private_slice_as_bytes(slice) {
        Some(bytes) => sink.atom(Atom::Bytes(Bytes::new(bytes))),
        None => {
            sink.seq_start(shape)?;
            for value in slice {
                A::__private_emit_plain(value, sink)?;
            }
            sink.seq_end()
        }
    }
}

/// Emits the elements of a sequence if they are plain (or if there are
/// none).
#[inline]
pub(crate) fn emit_plain_elements<'a, T: 'a, A: Serialize<T>>(
    values: impl ExactSizeIterator<Item = &'a T>,
    sink: &mut dyn PlainSink,
) -> Result<bool, Error> {
    if !A::__private_is_plain() && values.len() > 0 {
        return Ok(false);
    }
    for value in values {
        A::__private_emit_plain(value, sink)?;
    }
    Ok(true)
}

/// Returns the budget that is left after emitting a slice of plain values
/// at once (see `Serialize::__private_plain_cost`).
#[inline]
pub(crate) fn plain_cost_slice<T, A: Serialize<T>>(slice: &[T], budget: usize) -> Option<usize> {
    if let Some(bytes) = A::__private_slice_as_bytes(slice) {
        return budget.checked_sub(atom_cost(&Atom::Bytes(Bytes::new(bytes))));
    }
    plain_cost_values::<T, A>(slice.iter(), budget)
}

/// Returns the budget that is left after emitting a sequence of plain
/// values at once.
#[inline]
pub(crate) fn plain_cost_values<'a, T: 'a, A: Serialize<T>>(
    values: impl Iterator<Item = &'a T>,
    budget: usize,
) -> Option<usize> {
    let mut budget = budget.checked_sub(1)?;
    // values that are not plain are driven on their own
    if !A::__private_is_plain() {
        return Some(budget);
    }
    for value in values {
        budget = A::__private_plain_cost(value, budget)?;
    }
    Some(budget)
}

/// Emits plain elements as long as they fit into the budget (see
/// `IndexedSeq::emit_plain_chunk`).
///
/// `index` is the index of the first element, the index of the first
/// element that was not emitted is returned.
#[inline]
pub(crate) fn emit_plain_chunk<'a, T: 'a, A: Serialize<T>>(
    values: impl Iterator<Item = &'a T>,
    mut index: usize,
    mut budget: usize,
    sink: &mut dyn PlainSink,
) -> Result<usize, Error> {
    if !A::__private_is_plain() {
        return Ok(index);
    }
    for value in values {
        match A::__private_plain_cost(value, budget) {
            Some(left) => budget = left,
            None => break,
        }
        A::__private_emit_plain(value, sink)?;
        index += 1;
    }
    Ok(index)
}

/// Returns the bytes of a deque of `u8`.
fn deque_bytes<T, A: Serialize<T>>(value: &VecDeque<T>) -> Option<Cow<'_, [u8]>> {
    let (front, back) = value.as_slices();
    let front = A::__private_slice_as_bytes(front)?;
    if back.is_empty() {
        return Some(front);
    }
    let back = A::__private_slice_as_bytes(back)?;
    let mut rv = front.into_owned();
    rv.extend_from_slice(&back);
    Some(Cow::Owned(rv))
}

impl<T: Sync, A: Serialize<T>> Serialize<VecDeque<T>> for VecDeque<A> {
    #[inline]
    fn __private_begin<'a>(value: &'a VecDeque<T>, _state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(match deque_bytes::<T, A>(value) {
            Some(bytes) => Begin::emit(
                Emit::Atom(Atom::Bytes(Bytes::new(bytes))),
                ContainerShape::new(),
                false,
            ),
            None => begin_indexed::<Self, _>(value, Self::container_shape(value)),
        })
    }

    fn container_shape(value: &VecDeque<T>) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    fn serialize<'a>(value: &'a VecDeque<T>, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(match deque_bytes::<T, A>(value) {
            Some(bytes) => Emit::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => IterEmitter::<_, A>::emit(value.iter(), state),
        })
    }

    #[inline]
    fn __private_is_plain() -> bool {
        A::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(value: &VecDeque<T>) -> bool {
        A::__private_is_plain() || value.is_empty()
    }

    fn __private_emit_plain(value: &VecDeque<T>, sink: &mut dyn PlainSink) -> Result<(), Error> {
        match deque_bytes::<T, A>(value) {
            Some(bytes) => sink.atom(Atom::Bytes(Bytes::new(bytes))),
            None => {
                sink.seq_start(Self::container_shape(value))?;
                emit_plain_elements::<T, A>(value.iter(), sink)?;
                sink.seq_end()
            }
        }
    }

    #[inline]
    fn __private_plain_cost(value: &VecDeque<T>, budget: usize) -> Option<usize> {
        let (front, back) = value.as_slices();
        plain_cost_slice::<T, A>(back, plain_cost_slice::<T, A>(front, budget)?)
    }
}

impl<T: Sync, A: Serialize<T>> IndexedSeq for Adapted<VecDeque<A>, VecDeque<T>> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get().get(index).map(handle_as::<A, T>))
    }

    fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
        emit_plain_elements::<T, A>(self.get().iter(), sink)
    }

    fn emit_plain_chunk(
        &self,
        index: usize,
        budget: usize,
        sink: &mut dyn PlainSink,
    ) -> Result<usize, Error> {
        emit_plain_chunk::<T, A>(self.get().iter().skip(index), index, budget, sink)
    }
}

impl<T: Sync, A: Serialize<T>> Serialize<LinkedList<T>> for LinkedList<A> {
    begin_without_finish!(LinkedList<T>);

    fn container_shape(value: &LinkedList<T>) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    fn serialize<'a>(value: &'a LinkedList<T>, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(IterEmitter::<_, A>::emit(value.iter(), state))
    }
}

/// The elements are emitted in the (arbitrary) order of the heap.
impl<T: Sync, A: Serialize<T>> Serialize<BinaryHeap<T>> for BinaryHeap<A> {
    #[inline]
    fn __private_begin<'a>(
        value: &'a BinaryHeap<T>,
        _state: &mut State,
    ) -> Result<Begin<'a>, Error> {
        Ok(match A::__private_slice_as_bytes(value.as_slice()) {
            Some(bytes) => Begin::emit(
                Emit::Atom(Atom::Bytes(Bytes::new(bytes))),
                ContainerShape::new(),
                false,
            ),
            None => begin_indexed::<Self, _>(value, Self::container_shape(value)),
        })
    }

    fn container_shape(value: &BinaryHeap<T>) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    fn serialize<'a>(value: &'a BinaryHeap<T>, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(match A::__private_slice_as_bytes(value.as_slice()) {
            Some(bytes) => Emit::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => IterEmitter::<_, A>::emit(value.as_slice().iter(), state),
        })
    }
}

impl<T: Sync, A: Serialize<T>> IndexedSeq for Adapted<BinaryHeap<A>, BinaryHeap<T>> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get().as_slice().get(index).map(handle_as::<A, T>))
    }
}

/// Implements `Serialize` for maps, generic over the adapters of the keys
/// and values.
///
/// The maps need to support `len`, `is_empty` and `iter`.
macro_rules! serialize_map {
    ($([$($gen:tt)*] $ty:ty => $adapter:ty, $order:ident;)*) => {
        $(
            impl<$($gen)*> $crate::ser::Serialize<$ty> for $adapter
            where
                K: Sync,
                V: Sync,
                KA: $crate::ser::Serialize<K>,
                VA: $crate::ser::Serialize<V>,
            {
                #[inline]
                fn __private_begin<'a>(
                    value: &'a $ty,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'a>, $crate::Error> {
                    let shape = Self::container_shape(value);
                    if Self::__private_is_plain_value(value) {
                        Ok($crate::ser::impls::begin_plain::<Self, $ty>(value, shape))
                    } else {
                        Ok($crate::ser::Begin::emit(Self::serialize(value, state)?, shape, false))
                    }
                }

                fn container_shape(value: &$ty) -> $crate::ContainerShape {
                    $crate::ContainerShape::new()
                        .with_order($crate::Order::$order)
                        .with_len(value.len())
                }

                fn serialize<'a>(
                    value: &'a $ty,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Emit<'a>, $crate::Error> {
                    Ok($crate::ser::impls::MapIterEmitter::<_, V, KA, VA>::emit::<K>(value.iter(), state))
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    KA::__private_is_plain() && VA::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(value: &$ty) -> bool {
                    (KA::__private_is_plain() && VA::__private_is_plain()) || value.is_empty()
                }

                fn __private_emit_plain(
                    value: &$ty,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<(), $crate::Error> {
                    sink.map_start(Self::container_shape(value))?;
                    for (key, value) in value.iter() {
                        sink.key();
                        KA::__private_emit_plain(key, sink)?;
                        VA::__private_emit_plain(value, sink)?;
                    }
                    sink.map_end()
                }

                #[inline]
                fn __private_plain_cost(value: &$ty, budget: usize) -> Option<usize> {
                    let mut budget = budget.checked_sub(1)?;
                    for (key, value) in value.iter() {
                        budget = KA::__private_plain_cost(key, budget)?;
                        budget = VA::__private_plain_cost(value, budget)?;
                    }
                    Some(budget)
                }
            }
        )*
    };
}

// also used for the containers of other crates
#[allow(unused_imports)]
pub(crate) use serialize_map;

serialize_map! {
    [K, V, KA, VA] BTreeMap<K, V> => BTreeMap<KA, VA>, Sorted;
}

// the hasher of the adapter is not used
#[cfg(feature = "std")]
serialize_map! {
    [K, V, H: BuildHasher + Sync, KA, VA, AH: Sync] HashMap<K, V, H> => HashMap<KA, VA, AH>, Arbitrary;
}

/// Implements `Serialize` for sets, generic over the adapter of the
/// elements.
///
/// The sets need to support `len`, `is_empty` and `iter`.
macro_rules! serialize_set {
    ($([$($gen:tt)*] $ty:ty => $adapter:ty, $order:ident;)*) => {
        $(
            impl<$($gen)*> $crate::ser::Serialize<$ty> for $adapter
            where
                T: Sync,
                A: $crate::ser::Serialize<T>,
            {
                #[inline]
                fn __private_begin<'a>(
                    value: &'a $ty,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Begin<'a>, $crate::Error> {
                    let shape = Self::container_shape(value);
                    if Self::__private_is_plain_value(value) {
                        Ok($crate::ser::impls::begin_plain::<Self, $ty>(value, shape))
                    } else {
                        Ok($crate::ser::Begin::emit(Self::serialize(value, state)?, shape, false))
                    }
                }

                fn container_shape(value: &$ty) -> $crate::ContainerShape {
                    $crate::ContainerShape::new()
                        .with_order($crate::Order::$order)
                        .with_len(value.len())
                }

                fn describe(_value: &$ty, d: &mut dyn $crate::ser::Describe) {
                    d.set();
                }

                fn serialize<'a>(
                    value: &'a $ty,
                    state: &mut $crate::State,
                ) -> Result<$crate::ser::Emit<'a>, $crate::Error> {
                    Ok($crate::ser::impls::IterEmitter::<_, A>::emit(value.iter(), state))
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    A::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(value: &$ty) -> bool {
                    A::__private_is_plain() || value.is_empty()
                }

                fn __private_emit_plain(
                    value: &$ty,
                    sink: &mut dyn $crate::ser::PlainSink,
                ) -> Result<(), $crate::Error> {
                    sink.seq_start(Self::container_shape(value))?;
                    for value in value.iter() {
                        A::__private_emit_plain(value, sink)?;
                    }
                    sink.seq_end()
                }

                #[inline]
                fn __private_plain_cost(value: &$ty, budget: usize) -> Option<usize> {
                    $crate::ser::impls::plain_cost_values::<T, A>(value.iter(), budget)
                }
            }
        )*
    };
}

// also used for the containers of other crates
#[allow(unused_imports)]
pub(crate) use serialize_set;

serialize_set! {
    [T, A] BTreeSet<T> => BTreeSet<A>, Sorted;
}

// the hasher of the adapter is not used
#[cfg(feature = "std")]
serialize_set! {
    [T, H: BuildHasher + Sync, A, AH: Sync] HashSet<T, H> => HashSet<A, AH>, Arbitrary;
}

impl<T: Sync, A: Serialize<T>> Serialize<Option<T>> for Option<A> {
    fn is_optional(value: &Option<T>) -> bool {
        value.is_none()
    }

    fn container_shape(value: &Option<T>) -> ContainerShape {
        match value {
            Some(value) => A::container_shape(value),
            None => ContainerShape::new(),
        }
    }

    fn describe(value: &Option<T>, d: &mut dyn Describe) {
        match value {
            Some(value) => {
                d.some();
                A::describe(value, d);
            }
            None => d.none(),
        }
    }

    fn serialize<'a>(value: &'a Option<T>, state: &mut State) -> Result<Emit<'a>, Error> {
        match value {
            Some(value) => A::serialize(value, state),
            None => Ok(Emit::Atom(Atom::Null)),
        }
    }

    fn finish(value: &Option<T>, state: &mut State) -> Result<(), Error> {
        match value {
            Some(value) => A::finish(value, state),
            None => Ok(()),
        }
    }

    #[inline]
    fn __private_begin<'a>(value: &'a Option<T>, state: &mut State) -> Result<Begin<'a>, Error> {
        match value {
            Some(value) => A::__private_begin(value, state),
            None => Ok(Begin::emit(
                Emit::Atom(Atom::Null),
                ContainerShape::new(),
                false,
            )),
        }
    }

    #[inline]
    fn __private_is_plain() -> bool {
        A::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(value: &Option<T>) -> bool {
        match value {
            Some(value) => A::__private_is_plain_value(value),
            None => true,
        }
    }

    #[inline]
    fn __private_emit_plain(value: &Option<T>, sink: &mut dyn PlainSink) -> Result<(), Error> {
        match value {
            Some(value) => A::__private_emit_plain(value, sink),
            None => sink.atom(Atom::Null),
        }
    }

    #[inline]
    fn __private_plain_cost(value: &Option<T>, budget: usize) -> Option<usize> {
        match value {
            Some(value) => A::__private_plain_cost(value, budget),
            None => budget.checked_sub(1),
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
    ($(($name:ident, $adapter:ident),)+) => (
        impl<$($name: Sync,)* $($adapter: Serialize<$name>),*> Serialize<($($name,)*)> for ($($adapter,)*) {
            #[inline]
            fn __private_begin<'a>(value: &'a ($($name,)*), _state: &mut State) -> Result<Begin<'a>, Error> {
                Ok(begin_indexed::<Self, _>(value, Self::container_shape(value)))
            }

            fn container_shape(_value: &($($name,)*)) -> ContainerShape {
                ContainerShape::new().with_len(0 $(+ count_one!($name))*)
            }

            fn describe(_value: &($($name,)*), d: &mut dyn Describe) {
                d.tuple();
            }

            #[inline]
            fn __private_is_plain() -> bool {
                true $(&& $adapter::__private_is_plain())*
            }

            #[allow(non_snake_case)]
            fn __private_emit_plain(value: &($($name,)*), sink: &mut dyn PlainSink) -> Result<(), Error> {
                let ($($name,)*) = value;
                sink.seq_start(Self::container_shape(value))?;
                $($adapter::__private_emit_plain($name, sink)?;)*
                sink.seq_end()
            }

            #[allow(non_snake_case)]
            #[inline]
            fn __private_plain_cost(value: &($($name,)*), budget: usize) -> Option<usize> {
                let ($($name,)*) = value;
                let budget = budget.checked_sub(1)?;
                $(let budget = $adapter::__private_plain_cost($name, budget)?;)*
                Some(budget)
            }

            fn serialize<'a>(value: &'a ($($name,)*), state: &mut State) -> Result<Emit<'a>, Error> {
                // SAFETY: the wrapper is valid for 'a (it's the value), it only
                // holds a marker of the adapters
                let seq = unsafe { crate::ser::begin::indexed_unbounded(Adapted::<Self, _>::ptr(value)) };
                Ok(Emit::seq(crate::ser::IndexedSeqEmitter::new(seq), state))
            }
        }

        impl<$($name: Sync,)* $($adapter: Serialize<$name>),*> IndexedSeq
            for Adapted<($($adapter,)*), ($($name,)*)>
        {
            #[allow(non_snake_case)]
            fn element(&self, index: usize, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
                let ($($name,)*) = self.get();
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

            #[allow(non_snake_case)]
            fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
                if !<($($adapter,)*) as Serialize<($($name,)*)>>::__private_is_plain() {
                    return Ok(false);
                }
                let ($($name,)*) = self.get();
                $($adapter::__private_emit_plain($name, sink)?;)*
                Ok(true)
            }
        }

        serialize_for_tuple_peel!($(($name, $adapter),)*);
    )
}

macro_rules! serialize_for_tuple_peel {
    ($first:tt, $($other:tt,)*) => (serialize_for_tuple!($($other,)*);)
}

serialize_for_tuple! {
    (T1, A1), (T2, A2), (T3, A3), (T4, A4), (T5, A5), (T6, A6),
    (T7, A7), (T8, A8), (T9, A9), (T10, A10), (T11, A11), (T12, A12),
}

impl<T: Sync, A: Serialize<T>, const N: usize> Serialize<[T; N]> for [A; N] {
    #[inline]
    fn __private_begin<'a>(value: &'a [T; N], _state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(match A::__private_slice_as_bytes(&value[..]) {
            Some(bytes) => Begin::emit(
                Emit::Atom(Atom::Bytes(Bytes::new(bytes))),
                ContainerShape::new(),
                false,
            ),
            None => begin_indexed::<Self, _>(value, Self::container_shape(value)),
        })
    }

    fn container_shape(value: &[T; N]) -> ContainerShape {
        ContainerShape::new().with_len(value.len())
    }

    fn serialize<'a>(value: &'a [T; N], state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(match A::__private_slice_as_bytes(value) {
            Some(bytes) => Emit::Atom(Atom::Bytes(Bytes::new(bytes))),
            None => IterEmitter::<_, A>::emit(value.iter(), state),
        })
    }

    #[inline]
    fn __private_is_plain() -> bool {
        A::__private_is_plain()
    }

    #[inline]
    fn __private_is_plain_value(_value: &[T; N]) -> bool {
        A::__private_is_plain() || N == 0
    }

    fn __private_emit_plain(value: &[T; N], sink: &mut dyn PlainSink) -> Result<(), Error> {
        emit_plain_slice::<T, A>(value, Self::container_shape(value), sink)
    }

    #[inline]
    fn __private_plain_cost(value: &[T; N], budget: usize) -> Option<usize> {
        plain_cost_slice::<T, A>(value, budget)
    }
}

impl<T: Sync, A: Serialize<T>, const N: usize> IndexedSeq for Adapted<[A; N], [T; N]> {
    #[inline]
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.get().get(index).map(handle_as::<A, T>))
    }

    fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
        emit_plain_elements::<T, A>(self.get().iter(), sink)
    }

    fn emit_plain_chunk(
        &self,
        index: usize,
        budget: usize,
        sink: &mut dyn PlainSink,
    ) -> Result<usize, Error> {
        emit_plain_chunk::<T, A>(
            self.get().get(index..).unwrap_or_default().iter(),
            index,
            budget,
            sink,
        )
    }
}

/// Implements `Serialize` for pointers by forwarding to the pointee.
macro_rules! serialize_pointer {
    ($([$($gen:tt)*] $ty:ty => $adapter:ty;)*) => {
        $(
            impl<$($gen)*> Serialize<$ty> for $adapter {
                fn serialize<'a>(value: &'a $ty, state: &mut State) -> Result<Emit<'a>, Error> {
                    A::serialize(value, state)
                }

                fn finish(value: &$ty, state: &mut State) -> Result<(), Error> {
                    A::finish(value, state)
                }

                #[inline]
                fn __private_begin<'a>(value: &'a $ty, state: &mut State) -> Result<Begin<'a>, Error> {
                    A::__private_begin(value, state)
                }

                fn is_optional(value: &$ty) -> bool {
                    A::is_optional(value)
                }

                fn container_shape(value: &$ty) -> ContainerShape {
                    A::container_shape(value)
                }

                fn describe(value: &$ty, d: &mut dyn Describe) {
                    A::describe(value, d)
                }

                #[inline]
                fn __private_is_plain() -> bool {
                    A::__private_is_plain()
                }

                #[inline]
                fn __private_is_plain_value(value: &$ty) -> bool {
                    A::__private_is_plain_value(value)
                }

                #[inline]
                fn __private_emit_plain(value: &$ty, sink: &mut dyn PlainSink) -> Result<(), Error> {
                    A::__private_emit_plain(value, sink)
                }

                #[inline]
                fn __private_plain_cost(value: &$ty, budget: usize) -> Option<usize> {
                    A::__private_plain_cost(value, budget)
                }
            }
        )*
    };
}

serialize_pointer! {
    ['r, T: Sync + ?Sized, A: Serialize<T> + ?Sized] &'r T => &'r A;
    ['r, T: Sync + ?Sized, A: Serialize<T> + ?Sized] &'r mut T => &'r mut A;
    [T: Sync + ?Sized, A: Serialize<T> + ?Sized] Box<T> => Box<A>;
    [T: Send + Sync + ?Sized, A: Serialize<T> + Send + ?Sized] Arc<T> => Arc<A>;
}
