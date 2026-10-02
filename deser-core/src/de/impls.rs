use alloc::borrow::Cow;
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap, LinkedList, VecDeque, btree_map};
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::hash::{BuildHasher, Hash};
use core::marker::PhantomData;
use core::mem::{MaybeUninit, take};
#[cfg(feature = "std")]
use std::collections::{HashMap, HashSet, hash_map};

use crate::State;
use crate::Text;
use crate::adapters::Same;
use crate::de::lexical;
use crate::de::mapped::MappedSink;
use crate::de::update::Collection;
use crate::de::{CollectedErrors, DuplicateKeys};
use crate::de::{
    Deserialize, InlineEvent, InlineSeq, OwnedSink, Sink, SinkHandle, Slot, default_atom,
    empty_lexical_or_none, is_empty_lexical, is_null_atom,
};
use crate::de::{atom_into_handle, borrowed_atom_into_handle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ImplicitValue};
use crate::ext::Number;

impl<'de> Deserialize<'de> for () {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Null => {
                slot.set(());
                Ok(())
            }
            Atom::Lexical(ref value) if lexical::is_empty_null(value, state) => {
                slot.set(());
                Ok(())
            }
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("null")
    }

    slot_atom_into!();
}

impl<'de> Deserialize<'de> for bool {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bool(value) => {
                slot.set(value);
                Ok(())
            }
            Atom::Lexical(ref value) => {
                slot.set(lexical::parse_bool(value, state)?);
                Ok(())
            }
            // the text of other values is not a bool either
            Atom::Implicit(ref value) => match value.value() {
                ImplicitValue::Bool(value) => {
                    slot.set(value);
                    Ok(())
                }
                _ => default_atom(slot, atom, state),
            },
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("bool")
    }

    slot_atom_into!();

    #[inline]
    fn __private_atom_default() -> Option<Self> {
        Some(false)
    }

    #[inline(always)]
    fn __private_rejects_empty_lexical() -> bool {
        true
    }
}

impl<'de> Deserialize<'de> for String {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) | Atom::Lexical(value) => {
                slot.set(match value.into_cow() {
                    Cow::Borrowed(value) => copy_str(value),
                    Cow::Owned(value) => value,
                });
                Ok(())
            }
            Atom::Char(value) => {
                slot.set(value.to_string());
                Ok(())
            }
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("string")
    }

    slot_atom_into!();
}

/// Copies a string into a new allocation.
///
/// Most strings are short, these are copied inline rather than by calling
/// into `memcpy`.
#[inline(always)]
fn copy_str(value: &str) -> String {
    let len = value.len();
    if len > 16 {
        return value.to_owned();
    }
    let mut rv = Vec::<u8>::with_capacity(len);
    let src = value.as_ptr();
    let dst = rv.as_mut_ptr();
    // SAFETY: both regions are valid for `len` bytes and do not overlap.
    // The copies of the head and tail overlap within the regions if the
    // length is not a power of two.
    unsafe {
        use core::ptr::{read_unaligned as read, write_unaligned as write};
        if len >= 8 {
            let a = read(src.cast::<u64>());
            let b = read(src.add(len - 8).cast::<u64>());
            write(dst.cast::<u64>(), a);
            write(dst.add(len - 8).cast::<u64>(), b);
        } else if len >= 4 {
            let a = read(src.cast::<u32>());
            let b = read(src.add(len - 4).cast::<u32>());
            write(dst.cast::<u32>(), a);
            write(dst.add(len - 4).cast::<u32>(), b);
        } else if len > 0 {
            *dst = *src;
            *dst.add(len / 2) = *src.add(len / 2);
            *dst.add(len - 1) = *src.add(len - 1);
        }
        rv.set_len(len);
        // the bytes were copied from a string
        String::from_utf8_unchecked(rv)
    }
}

/// Implements the deserialization of an integer.
macro_rules! int_atom {
    ($ty:ty) => {
        #[allow(clippy::useless_conversion)]
        fn deserialize_atom(
            slot: &mut Slot<Self>,
            atom: Atom,
            state: &mut State,
        ) -> Result<(), Error> {
            let out_of_range = |value: &dyn core::fmt::Display| {
                lexical::out_of_range(value, stringify!($ty), state)
            };
            let value = match atom {
                Atom::U64(value) => <$ty>::try_from(value).map_err(|_| out_of_range(&value))?,
                Atom::I64(value) => <$ty>::try_from(value).map_err(|_| out_of_range(&value))?,
                Atom::Ext(ref ext) if ext.is::<u128>() => {
                    let value = *ext.downcast_ref::<u128>().unwrap();
                    <$ty>::try_from(value).map_err(|_| out_of_range(&value))?
                }
                Atom::Ext(ref ext) if ext.is::<i128>() => {
                    let value = *ext.downcast_ref::<i128>().unwrap();
                    <$ty>::try_from(value).map_err(|_| out_of_range(&value))?
                }
                Atom::Lexical(ref value) => match value.parse::<$ty>() {
                    Ok(value) => value,
                    Err(err) => {
                        return Err(lexical::int_error(value, err, stringify!($ty), state));
                    }
                },
                // the text of other values is not a number either
                Atom::Implicit(ref value) => match value.value() {
                    ImplicitValue::U64(value) => {
                        <$ty>::try_from(value).map_err(|_| out_of_range(&value))?
                    }
                    ImplicitValue::I64(value) => {
                        <$ty>::try_from(value).map_err(|_| out_of_range(&value))?
                    }
                    _ => return default_atom(slot, atom, state),
                },
                other => return default_atom(slot, other, state),
            };
            slot.set(value);
            Ok(())
        }

        fn expecting() -> Cow<'static, str> {
            Cow::Borrowed(stringify!($ty))
        }

        slot_atom_into!();

        #[inline]
        fn __private_atom_default() -> Option<Self> {
            Some(0)
        }

        #[inline(always)]
        fn __private_rejects_empty_lexical() -> bool {
            true
        }
    };
}

/// Implements `Deserialize` for integers.
macro_rules! deserialize_int {
    ($($ty:ty),*) => {
        $(
            impl<'de> Deserialize<'de> for $ty {
                int_atom!($ty);
            }
        )*
    };
}

deserialize_int!(u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

impl<'de> Deserialize<'de> for u8 {
    int_atom!(u8);

    fn __private_is_bytes() -> bool {
        true
    }

    fn __private_vec_from_bytes(bytes: Vec<u8>) -> Option<Vec<u8>> {
        Some(bytes)
    }

    fn __private_array_from_bytes<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
        bytes.try_into().ok()
    }
}

impl<'de> Deserialize<'de> for char {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Char(value) => {
                slot.set(value);
                Ok(())
            }
            Atom::Str(ref s) => {
                let mut chars = s.chars();
                if let Some(first_char) = chars.next()
                    && chars.next().is_none()
                {
                    slot.set(first_char);
                    return Ok(());
                }
                Err(atom.unexpected_error(&Self::expecting()))
            }
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("char")
    }

    slot_atom_into!();
}

macro_rules! deserialize_float {
    ($ty:ty) => {
        impl<'de> Deserialize<'de> for $ty {
            fn expecting() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($ty))
            }

            slot_atom_into!();

            #[inline]
            fn __private_atom_default() -> Option<Self> {
                Some(0.0)
            }

            #[inline(always)]
            fn __private_rejects_empty_lexical() -> bool {
                true
            }

            #[inline]
            fn deserialize_atom(
                slot: &mut Slot<Self>,
                atom: Atom,
                state: &mut State,
            ) -> Result<(), Error> {
                // Keep the owning variants out of the inlined numeric path.
                // This is not cold: JSON commonly emits Number extensions.
                #[inline(never)]
                fn other(slot: &mut Slot<$ty>, atom: Atom, state: &mut State) -> Result<(), Error> {
                    let value = match atom {
                        Atom::Ext(ext) => match number_value(&ext) {
                            Some(value) => value as $ty,
                            None if ext.is::<u128>() => *ext.downcast_ref::<u128>().unwrap() as $ty,
                            None if ext.is::<i128>() => *ext.downcast_ref::<i128>().unwrap() as $ty,
                            None => return default_atom(slot, Atom::Ext(ext), state),
                        },
                        Atom::Lexical(value) => match value.parse::<$ty>() {
                            Ok(value) => value,
                            Err(_) => return Err(lexical::invalid(&value, stringify!($ty), state)),
                        },
                        // the text of other values is not a number either
                        Atom::Implicit(ref value) => match value.value() {
                            ImplicitValue::U64(value) => value as $ty,
                            ImplicitValue::I64(value) => value as $ty,
                            ImplicitValue::F64(value) => value as $ty,
                            _ => return default_atom(slot, atom, state),
                        },
                        other => return default_atom(slot, other, state),
                    };
                    slot.set(value);
                    Ok(())
                }

                let value = match atom {
                    Atom::U64(value) => value as $ty,
                    Atom::I64(value) => value as $ty,
                    Atom::F64(value) => value as $ty,
                    Atom::F32(value) => value as $ty,
                    atom => return other(slot, atom, state),
                };
                // Only variants with Copy payloads reach here. Avoid calling
                // Atom's out-of-line drop glue, which has nothing to drop.
                core::mem::forget(atom);
                slot.set(value);
                Ok(())
            }
        }
    };
}

/// Returns the value of a number extension value.
#[inline]
fn number_value(ext: &crate::ext::ExtValue) -> Option<f64> {
    ext.downcast_value_ref::<Number>().map(|x| x.value())
}

deserialize_float!(f32);
deserialize_float!(f64);

// The containers are implemented as adapters (see `crate::adapters`) that
// are generic over the adapters of their elements.  The `Deserialize`
// implementations use the containers with `Same` as element adapter which
// compiles to the same code as a direct implementation.

/// Sequences that are collected into a vector and then converted.
pub(crate) trait SeqTarget<T>: Sized + Send {
    /// The name of the type for error messages.
    const NAME: &'static str;

    /// Converts the vector into the sequence.
    ///
    /// This fails for sequences with a fixed capacity.
    fn from_vec(vec: Vec<T>) -> Result<Self, Error>;
}

impl<T: Send> SeqTarget<T> for Vec<T> {
    const NAME: &'static str = "vec";

    #[inline(always)]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(vec)
    }
}

impl<T: Send> SeqTarget<T> for VecDeque<T> {
    const NAME: &'static str = "VecDeque";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(VecDeque::from(vec))
    }
}

impl<T: Send> SeqTarget<T> for LinkedList<T> {
    const NAME: &'static str = "LinkedList";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(vec.into_iter().collect())
    }
}

impl<T: Ord + Send> SeqTarget<T> for BinaryHeap<T> {
    const NAME: &'static str = "BinaryHeap";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(BinaryHeap::from(vec))
    }
}

impl<T: Send> SeqTarget<T> for Box<[T]> {
    const NAME: &'static str = "slice";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(vec.into_boxed_slice())
    }
}

impl<T: Send + Sync> SeqTarget<T> for Arc<[T]> {
    const NAME: &'static str = "slice";

    #[inline]
    fn from_vec(vec: Vec<T>) -> Result<Self, Error> {
        Ok(Arc::from(vec))
    }
}

/// What a sequence expects (see [`Deserialize::expecting`]).
pub(crate) fn seq_expecting<'de, C: SeqTarget<T>, T: Send, A: Deserialize<'de, T>>()
-> Cow<'static, str> {
    Cow::Borrowed(if A::__private_is_bytes() {
        "bytes"
    } else {
        C::NAME
    })
}

/// Creates the sink for a sequence with an element adapter.
///
/// The elements are collected into a vector which is converted into the
/// sequence at the end.  For elements of type `u8` bytes are accepted.
pub(crate) fn seq_sink<'a, 'de, C, T, A>(
    out: &'a mut Option<C>,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    C: SeqTarget<T> + 'a,
    T: Send + 'a,
    A: Deserialize<'de, T>,
{
    struct SeqSink<'a, C, T, A> {
        slot: &'a mut Option<C>,
        vec: Vec<T>,
        element: Option<T>,
        is_seq: bool,
        errors: CollectedErrors,
        _marker: PhantomData<fn() -> A>,
    }

    impl<'a, C, T, A> SeqSink<'a, C, T, A> {
        fn flush(&mut self) {
            if let Some(element) = self.element.take() {
                self.vec.push(element);
            }
        }
    }

    impl<'de, 'a, C: SeqTarget<T>, T: Send, A: Deserialize<'de, T>> Sink<'de> for SeqSink<'a, C, T, A> {
        fn expecting(&self) -> Cow<'_, str> {
            seq_expecting::<C, T, A>()
        }

        fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            match atom {
                Atom::Bytes(value) => match A::__private_vec_from_bytes(value.into_owned()) {
                    Some(vec) => {
                        *self.slot = Some(C::from_vec(vec)?);
                        Ok(())
                    }
                    None => Err(Error::new(
                        ErrorKind::InvalidType,
                        format!("unexpected bytes, expected {}", self.expecting()),
                    )),
                },
                // formats without native bytes represent them as strings
                Atom::Str(ref value) if A::__private_is_bytes() => {
                    let bytes = crate::adapters::bytes::decode_str(value, state)?;
                    match A::__private_vec_from_bytes(bytes) {
                        Some(vec) => {
                            *self.slot = Some(C::from_vec(vec)?);
                            Ok(())
                        }
                        None => default_atom(self, atom, state),
                    }
                }
                other => default_atom(self, other, state),
            }
        }

        fn seq(&mut self, state: &mut State) -> Result<(), Error> {
            self.is_seq = true;
            self.vec
                .reserve(state.container_shape().cautious_capacity::<T>());
            // the first item is requested as raw value when it starts
            if let Some(format) = A::__private_raw() {
                state.__private_request_raw(format)?;
            }
            Ok(())
        }

        fn __private_seq(&mut self, state: &mut State) -> Result<bool, Error> {
            self.seq(state)?;
            Ok(A::__private_inline_seq().is_some())
        }

        fn __private_inline_atom(
            &mut self,
            index: usize,
            atom: Atom,
            state: &mut State,
        ) -> Result<(), Error> {
            match A::__private_inline_seq() {
                Some(inline) => (inline.atom)(&mut self.element, index, atom, state),
                None => unreachable!(),
            }
        }

        fn __private_inline_event(
            &mut self,
            event: InlineEvent,
            state: &mut State,
        ) -> Result<(), Error> {
            let Some(inline) = A::__private_inline_seq() else {
                unreachable!()
            };
            match event {
                InlineEvent::Start => {
                    self.flush();
                    (inline.start)(&mut self.element);
                    Ok(())
                }
                InlineEvent::End(len) => (inline.end)(&mut self.element, len),
                InlineEvent::Container(index, is_map) => {
                    Err((inline.container)(index, is_map, state))
                }
            }
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush();
            Ok(A::deserialize_into(&mut self.element, state))
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush();
            A::__private_atom_into(&mut self.element, atom, state)?;
            // raw values are atoms, the next item is requested after one
            match A::__private_raw() {
                Some(format) => state.__private_request_raw(format),
                None => Ok(()),
            }
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush();
            A::__private_borrowed_atom_into(&mut self.element, atom, state)?;
            match A::__private_raw() {
                Some(format) => state.__private_request_raw(format),
                None => Ok(()),
            }
        }

        fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
            self.element = None;
            self.errors.collect(err, state)
        }

        fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
            self.errors.finish()?;
            if self.is_seq {
                self.flush();
                *self.slot = Some(C::from_vec(take(&mut self.vec))?);
            }
            Ok(())
        }
    }

    // SAFETY: `A` is an adapter, the sink only holds a marker of it
    unsafe {
        SinkHandle::arena_unbounded(
            SeqSink::<C, T, A> {
                slot: out,
                vec: Vec::new(),
                element: None,
                is_seq: false,
                errors: CollectedErrors::new(),
                _marker: PhantomData,
            },
            state,
        )
    }
}

/// The methods of `Deserialize` for collections that collect the values
/// of a repeated key (see [`Collection`]), the adapter of the elements is
/// `A`.
///
/// Sequences of bytes (like `Vec<u8>`) do not collect, the value of their
/// key is the bytes.
macro_rules! collection_methods {
    ($target:ty) => {
        #[inline]
        fn __private_collects() -> bool {
            !A::__private_is_bytes()
        }

        collection_methods!(@common $target);
    };
    (set $target:ty) => {
        #[inline]
        fn __private_collects() -> bool {
            true
        }

        collection_methods!(@common $target);
    };
    (@common $target:ty) => {

        fn __private_collect_into<'out>(
            out: &'out mut Option<$target>,
            state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            crate::de::update::collect_into::<$target, T, A>(out, state)
        }

        fn __private_collect_update<'out>(
            value: &'out mut $target,
            first: bool,
            state: &mut State,
        ) -> SinkHandle<'out, 'de>
        where
            $target: Send,
            Self: Sized,
        {
            crate::de::update::collect_update::<$target, T, A>(value, first, state)
        }

        fn __private_collect_empty() -> Option<$target> {
            Some(<$target as crate::de::update::Collection<T>>::empty())
        }
    };
}

#[allow(unused_imports)]
pub(crate) use collection_methods;

/// Implements `Deserialize` for sequences, generic over the adapter of
/// the elements (the sequence of `T` itself is the adapter with `A = T`).
///
/// Sequences marked with `collect` collect the values of repeated keys.
macro_rules! deserialize_seq {
    ($($($collect:ident)? [$($bound:tt)*] $target:ty => $adapter:ty;)*) => {
        $(
            impl<'de, $($bound)*, A: Deserialize<'de, T>> Deserialize<'de, $target> for $adapter
            where
                $adapter: Send,
            {
                #[inline]
                fn deserialize_into<'out>(out: &'out mut Option<$target>, state: &mut State) -> SinkHandle<'out, 'de> {
                    seq_sink::<$target, T, A>(out, state)
                }

                fn expecting() -> Cow<'static, str> {
                    seq_expecting::<$target, T, A>()
                }

                $(deserialize_seq! { @$collect $target })?
            }
        )*
    };
    (@collect $target:ty) => { collection_methods!($target); };
}

deserialize_seq! {
    collect [T: Send] Vec<T> => Vec<A>;
    collect [T: Send] VecDeque<T> => VecDeque<A>;
    collect [T: Send] LinkedList<T> => LinkedList<A>;
    collect [T: Ord + Send] BinaryHeap<T> => BinaryHeap<A>;
    collect [T: Send] Box<[T]> => Box<[A]>;
    // the elements of a shared slice cannot be moved into a larger one
    [T: Send + Sync] Arc<[T]> => Arc<[A]>;
}

impl<T: Send> Collection<T> for Vec<T> {
    fn empty() -> Self {
        Vec::new()
    }

    #[inline]
    fn add(&mut self, value: T) -> Result<(), Error> {
        self.push(value);
        Ok(())
    }
}

impl<T: Send> Collection<T> for VecDeque<T> {
    fn empty() -> Self {
        VecDeque::new()
    }

    fn add(&mut self, value: T) -> Result<(), Error> {
        self.push_back(value);
        Ok(())
    }
}

impl<T: Send> Collection<T> for LinkedList<T> {
    fn empty() -> Self {
        LinkedList::new()
    }

    fn add(&mut self, value: T) -> Result<(), Error> {
        self.push_back(value);
        Ok(())
    }
}

impl<T: Ord + Send> Collection<T> for BinaryHeap<T> {
    fn empty() -> Self {
        BinaryHeap::new()
    }

    fn add(&mut self, value: T) -> Result<(), Error> {
        self.push(value);
        Ok(())
    }
}

impl<T: Send> Collection<T> for Box<[T]> {
    fn empty() -> Self {
        Box::default()
    }

    fn add(&mut self, value: T) -> Result<(), Error> {
        let mut vec = take(self).into_vec();
        vec.push(value);
        *self = vec.into_boxed_slice();
        Ok(())
    }
}

/// Maps that can be deserialized.
pub(crate) trait MapTarget<K, V>: Default + Send {
    /// The name of the type for error messages.
    const NAME: &'static str;
    /// Inserts an entry.
    ///
    /// If the key exists, the value is only replaced if `replace` is set.
    /// Returns `true` if the key existed.
    fn insert_entry(&mut self, key: K, value: V, replace: bool) -> bool;
    fn reserve_entries(&mut self, additional: usize) {
        let _ = additional;
    }
    /// Returns the value of a key.
    fn entry_mut(&mut self, key: &K) -> Option<&mut V>;
    /// Moves the entries of another map into this one, the values of
    /// `other` replace existing values.
    fn merge(&mut self, other: Self);
}

impl<K: Ord + Send, V: Send> MapTarget<K, V> for BTreeMap<K, V> {
    const NAME: &'static str = "BTreeMap";

    #[inline]
    fn insert_entry(&mut self, key: K, value: V, replace: bool) -> bool {
        match self.entry(key) {
            btree_map::Entry::Vacant(entry) => {
                entry.insert(value);
                false
            }
            btree_map::Entry::Occupied(mut entry) => {
                if replace {
                    entry.insert(value);
                }
                true
            }
        }
    }

    #[inline]
    fn entry_mut(&mut self, key: &K) -> Option<&mut V> {
        self.get_mut(key)
    }

    fn merge(&mut self, mut other: Self) {
        if self.is_empty() {
            *self = other;
        } else {
            // this merges the sorted entries in a single pass
            self.append(&mut other);
        }
    }
}

#[cfg(feature = "std")]
impl<K: Hash + Eq + Send, V: Send, H: BuildHasher + Default + Send> MapTarget<K, V>
    for HashMap<K, V, H>
{
    const NAME: &'static str = "HashMap";

    #[inline]
    fn insert_entry(&mut self, key: K, value: V, replace: bool) -> bool {
        match self.entry(key) {
            hash_map::Entry::Vacant(entry) => {
                entry.insert(value);
                false
            }
            hash_map::Entry::Occupied(mut entry) => {
                if replace {
                    entry.insert(value);
                }
                true
            }
        }
    }

    #[inline]
    fn reserve_entries(&mut self, additional: usize) {
        self.reserve(additional);
    }

    #[inline]
    fn entry_mut(&mut self, key: &K) -> Option<&mut V> {
        self.get_mut(key)
    }

    fn merge(&mut self, mut other: Self) {
        // the smaller map is moved into the larger one
        if other.len() > self.len() {
            core::mem::swap(self, &mut other);
            for (key, value) in other {
                self.entry(key).or_insert(value);
            }
        } else {
            self.extend(other);
        }
    }
}

/// Where a map sink puts the map.
pub(crate) enum MapOut<'a, M> {
    /// The map is stored in the slot.
    Slot(&'a mut Option<M>),
    /// The entries are merged into an existing map.
    Update(&'a mut M),
}

/// Creates the sink for a map with key and value adapters.
pub(crate) fn map_sink<'a, 'de, M, K, V, KA, VA>(
    out: MapOut<'a, M>,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    M: MapTarget<K, V> + 'a,
    K: Send + 'a,
    V: Send + 'a,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    // Updates deserialize a new map which is merged into the existing one
    // when it's complete.  This keeps the detection of duplicate keys (which
    // are duplicates in the data, not keys of the existing map) and moves
    // the smaller map into the larger one.
    struct MapSink<'a, M, K, V, KA, VA> {
        out: MapOut<'a, M>,
        map: M,
        key: Option<K>,
        value: Option<V>,
        duplicate_keys: DuplicateKeys,
        errors: CollectedErrors,
        _marker: PhantomData<fn() -> (KA, VA)>,
    }

    impl<'a, M: MapTarget<K, V>, K, V, KA, VA> MapSink<'a, M, K, V, KA, VA> {
        #[inline]
        fn flush(&mut self) -> Result<(), Error> {
            if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                let replace = self.duplicate_keys != DuplicateKeys::First;
                if self.map.insert_entry(key, value, replace) {
                    self.duplicate_keys
                        .resolve(|| "duplicate key in map".into())?;
                }
            }
            Ok(())
        }

        /// Returns the sink of a value that its key's collection collects.
        ///
        /// In a multimap the values of collections (see
        /// [`Deserialize::__private_collects`]) collect the values of all
        /// occurrences of their key.
        #[cold]
        fn collect_value<'de>(&mut self, state: &mut State) -> SinkHandle<'_, 'de>
        where
            VA: Deserialize<'de, V>,
            V: Send,
        {
            if let Some(ref key) = self.key
                && let Some(value) = self.map.entry_mut(key)
            {
                return VA::__private_collect_update(value, false, state);
            }
            VA::__private_collect_into(&mut self.value, state)
        }

        /// Adds the previous entry before the next one starts.
        ///
        /// If it's a duplicate that is rejected, the error is collected
        /// here as it's not the error of the next entry.
        #[inline]
        fn flush_before(&mut self, state: &mut State) -> Result<(), Error> {
            match self.flush() {
                Ok(()) => Ok(()),
                Err(err) => self.errors.collect(err, state),
            }
        }
    }

    impl<'de, 'a, M, K, V, KA, VA> Sink<'de> for MapSink<'a, M, K, V, KA, VA>
    where
        M: MapTarget<K, V>,
        K: Send,
        V: Send,
        KA: Deserialize<'de, K>,
        VA: Deserialize<'de, V>,
    {
        fn expecting(&self) -> Cow<'_, str> {
            Cow::Borrowed(M::NAME)
        }

        fn map(&mut self, state: &mut State) -> Result<(), Error> {
            self.map
                .reserve_entries(state.container_shape().cautious_capacity::<(K, V)>());
            self.duplicate_keys = DuplicateKeys::of(state);
            Ok(())
        }

        fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush_before(state)?;
            Ok(KA::deserialize_into(&mut self.key, state))
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            if VA::__private_collects() && state.is_multimap() {
                return Ok(self.collect_value(state));
            }
            Ok(VA::deserialize_into(&mut self.value, state))
        }

        fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush_before(state)?;
            KA::__private_atom_into(&mut self.key, atom, state)?;
            // the value is requested as raw value after its key
            match VA::__private_raw() {
                Some(format) => state.__private_request_raw(format),
                None => Ok(()),
            }
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            if VA::__private_collects() && state.is_multimap() {
                return atom_into_handle(self.collect_value(state), atom, state);
            }
            VA::__private_atom_into(&mut self.value, atom, state)
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush_before(state)?;
            KA::__private_borrowed_atom_into(&mut self.key, atom, state)?;
            match VA::__private_raw() {
                Some(format) => state.__private_request_raw(format),
                None => Ok(()),
            }
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            if VA::__private_collects() && state.is_multimap() {
                return borrowed_atom_into_handle(self.collect_value(state), atom, state);
            }
            VA::__private_borrowed_atom_into(&mut self.value, atom, state)
        }

        /// Takes all keys when the map is flattened into a struct.
        ///
        /// The key is parsed like the keys of formats that only have string
        /// keys.
        fn value_for_key(
            &mut self,
            key: &str,
            state: &mut State,
        ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
            // `map` is not invoked for flattened maps
            self.duplicate_keys = DuplicateKeys::of(state);
            self.flush_before(state)?;
            KA::__private_atom_into(&mut self.key, Atom::Lexical(Text::borrowed(key)), state)?;
            if VA::__private_collects() && state.is_multimap() {
                return Ok(Some(self.collect_value(state)));
            }
            Ok(Some(VA::deserialize_into(&mut self.value, state)))
        }

        fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
            self.key = None;
            self.value = None;
            self.errors.collect(err, state)
        }

        fn finish(&mut self, state: &mut State) -> Result<(), Error> {
            self.flush_before(state)?;
            self.errors.finish()?;
            let map = take(&mut self.map);
            match self.out {
                MapOut::Slot(ref mut slot) => **slot = Some(map),
                MapOut::Update(ref mut target) => target.merge(map),
            }
            Ok(())
        }
    }

    // SAFETY: `KA` and `VA` are adapters, the sink only holds a marker of
    // them
    unsafe {
        SinkHandle::arena_unbounded(
            MapSink::<M, K, V, KA, VA> {
                out,
                map: M::default(),
                key: None,
                value: None,
                duplicate_keys: DuplicateKeys::Error,
                errors: CollectedErrors::new(),
                _marker: PhantomData,
            },
            state,
        )
    }
}

impl<'de, K, V, KA, VA> Deserialize<'de, BTreeMap<K, V>> for BTreeMap<KA, VA>
where
    K: Ord + Send,
    V: Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<BTreeMap<K, V>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<BTreeMap<K, V> as MapTarget<K, V>>::NAME)
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).
    fn deserialize_update<'out>(
        value: &'out mut BTreeMap<K, V>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Update(value), state)
    }
}

// The hasher of the adapter is not used (in `HashMap<_, DisplayFromStr>` it's
// the default).
#[cfg(feature = "std")]
impl<'de, K, V, H, KA, VA, S> Deserialize<'de, HashMap<K, V, H>> for HashMap<KA, VA, S>
where
    K: Hash + Eq + Send,
    V: Send,
    H: BuildHasher + Default + Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
    S: Send,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<HashMap<K, V, H>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<HashMap<K, V, H> as MapTarget<K, V>>::NAME)
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).
    fn deserialize_update<'out>(
        value: &'out mut HashMap<K, V, H>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Update(value), state)
    }
}

/// Sets that can be deserialized.
pub(crate) trait SetTarget<T>: Default + Send {
    /// The name of the type for error messages.
    const NAME: &'static str;
    fn insert_element(&mut self, value: T);
    fn reserve_elements(&mut self, additional: usize) {
        let _ = additional;
    }
}

impl<T: Ord + Send> SetTarget<T> for BTreeSet<T> {
    const NAME: &'static str = "BTreeSet";

    #[inline]
    fn insert_element(&mut self, value: T) {
        self.insert(value);
    }
}

#[cfg(feature = "std")]
impl<T: Hash + Eq + Send, H: BuildHasher + Default + Send> SetTarget<T> for HashSet<T, H> {
    const NAME: &'static str = "HashSet";

    #[inline]
    fn insert_element(&mut self, value: T) {
        self.insert(value);
    }

    #[inline]
    fn reserve_elements(&mut self, additional: usize) {
        self.reserve(additional);
    }
}

/// Implements [`Collection`] for sets.
macro_rules! set_collection {
    ($([$($bound:tt)*] $target:ty;)*) => {
        $(
            impl<$($bound)*> crate::de::update::Collection<T> for $target {
                fn empty() -> Self {
                    Default::default()
                }

                fn add(&mut self, value: T) -> Result<(), crate::Error> {
                    crate::de::impls::SetTarget::insert_element(self, value);
                    Ok(())
                }
            }
        )*
    };
}

#[allow(unused_imports)]
pub(crate) use set_collection;

set_collection! {
    [T: Ord + Send] BTreeSet<T>;
}

#[cfg(feature = "std")]
set_collection! {
    [T: Hash + Eq + Send, H: BuildHasher + Default + Send] HashSet<T, H>;
}

/// Creates the sink for a set with an element adapter.
pub(crate) fn set_sink<'a, 'de, S, T, A>(
    out: &'a mut Option<S>,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    S: SetTarget<T> + 'a,
    T: Send + 'a,
    A: Deserialize<'de, T>,
{
    struct SetSink<'a, S, T, A> {
        slot: &'a mut Option<S>,
        set: S,
        element: Option<T>,
        errors: CollectedErrors,
        _marker: PhantomData<fn() -> A>,
    }

    impl<'a, S: SetTarget<T>, T, A> SetSink<'a, S, T, A> {
        fn flush(&mut self) {
            if let Some(element) = self.element.take() {
                self.set.insert_element(element);
            }
        }
    }

    impl<'de, 'a, S: SetTarget<T>, T: Send, A: Deserialize<'de, T>> Sink<'de> for SetSink<'a, S, T, A> {
        fn expecting(&self) -> Cow<'_, str> {
            Cow::Borrowed(S::NAME)
        }

        fn seq(&mut self, state: &mut State) -> Result<(), Error> {
            self.set
                .reserve_elements(state.container_shape().cautious_capacity::<T>());
            Ok(())
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush();
            Ok(A::deserialize_into(&mut self.element, state))
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush();
            A::__private_atom_into(&mut self.element, atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush();
            A::__private_borrowed_atom_into(&mut self.element, atom, state)
        }

        fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
            self.element = None;
            self.errors.collect(err, state)
        }

        fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
            self.errors.finish()?;
            self.flush();
            *self.slot = Some(take(&mut self.set));
            Ok(())
        }
    }

    // SAFETY: `A` is an adapter, the sink only holds a marker of it
    unsafe {
        SinkHandle::arena_unbounded(
            SetSink::<S, T, A> {
                slot: out,
                set: S::default(),
                element: None,
                errors: CollectedErrors::new(),
                _marker: PhantomData,
            },
            state,
        )
    }
}

impl<'de, T: Ord + Send, A: Deserialize<'de, T>> Deserialize<'de, BTreeSet<T>> for BTreeSet<A> {
    fn deserialize_into<'out>(
        out: &'out mut Option<BTreeSet<T>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, A>(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<BTreeSet<T> as SetTarget<T>>::NAME)
    }

    collection_methods!(set BTreeSet<T>);
}

// the hasher of the adapter is not used
#[cfg(feature = "std")]
impl<'de, T, H, A, S> Deserialize<'de, HashSet<T, H>> for HashSet<A, S>
where
    T: Hash + Eq + Send,
    H: BuildHasher + Default + Send,
    A: Deserialize<'de, T>,
    S: Send,
{
    fn deserialize_into<'out>(
        out: &'out mut Option<HashSet<T, H>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, A>(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<HashSet<T, H> as SetTarget<T>>::NAME)
    }

    collection_methods!(set HashSet<T, H>);
}

impl<'de, T: Send, A: Deserialize<'de, T>> Deserialize<'de, Option<T>> for Option<A> {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Option<T>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        A::deserialize_into(out.insert(None), state).ignore_null()
    }

    // the handle passes everything on to the value
    fn expecting() -> Cow<'static, str> {
        A::expecting()
    }

    fn deserialize_update<'out>(
        value: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        crate::de::update::update_option::<T, A>(value, state)
    }

    // An optional collection collects into the collection, it's `None` if
    // its key is missing.  Values that are null (or empty text for types
    // that do not accept it) are not added.

    #[inline(always)]
    fn __private_raw() -> Option<&'static crate::ext::RawFormatInfo> {
        A::__private_raw()
    }

    #[inline]
    fn __private_collects() -> bool {
        A::__private_collects()
    }

    fn __private_collect_into<'out>(
        out: &'out mut Option<Option<T>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        A::__private_collect_into(out.get_or_insert(None), state).ignore_null()
    }

    fn __private_collect_update<'out>(
        value: &'out mut Option<T>,
        first: bool,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        if first {
            *value = None;
        }
        A::__private_collect_into(value, state).ignore_null()
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Option<T>>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let inner = out.insert(None);
        if is_null_atom(&atom) {
            // the sink is created (and dropped without being used) so that
            // this behaves exactly like `deserialize_into`.  This matters
            // for nested options where the inner one becomes `Some(None)`.
            drop(A::deserialize_into(inner, state));
            Ok(())
        } else if is_empty_lexical(&atom, state) {
            if A::__private_rejects_empty_lexical() {
                return Ok(());
            }
            if !empty_lexical_or_none(atom, state, |atom, state| {
                A::__private_atom_into(inner, atom, state)
            })? {
                *inner = None;
            }
            Ok(())
        } else {
            A::__private_atom_into(inner, atom, state)
        }
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Option<T>>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let inner = out.insert(None);
        if is_null_atom(&atom) {
            drop(A::deserialize_into(inner, state));
            Ok(())
        } else if is_empty_lexical(&atom, state) {
            if A::__private_rejects_empty_lexical() {
                return Ok(());
            }
            if !empty_lexical_or_none(atom, state, |atom, state| {
                A::__private_borrowed_atom_into(inner, atom, state)
            })? {
                *inner = None;
            }
            Ok(())
        } else {
            A::__private_borrowed_atom_into(inner, atom, state)
        }
    }

    fn initial_value() -> Option<Option<T>> {
        Some(None)
    }
}

/// What tuples expect.
const TUPLE_NAME: &str = "tuple";

/// What arrays expect.
const ARRAY_NAME: &str = "array";

macro_rules! deserialize_for_tuple {
    () => ();
    ($(($name:ident, $adapter:ident),)+) => (
        impl<'de, $($name: Send,)* $($adapter: Deserialize<'de, $name>),*> Deserialize<'de, ($($name,)*)> for ($($adapter,)*) {
            fn expecting() -> Cow<'static, str> {
                Cow::Borrowed(TUPLE_NAME)
            }

            // tuples of atoms are built inline, this behaves like the sink
            // below
            #[inline]
            fn __private_inline_seq() -> Option<InlineSeq<($($name,)*)>> {
                #![allow(non_snake_case)]
                $(
                    $adapter::__private_atom_default()?;
                )*
                Some(InlineSeq {
                    start: |out| {
                        *out = Some(($($adapter::__private_atom_default().unwrap(),)*));
                    },
                    atom: |out, index, atom, state| {
                        let Some(($($name,)*)) = out else {
                            unreachable!()
                        };
                        let mut __counter = 0;
                        $(
                            if index == __counter {
                                let mut value = None;
                                $adapter::__private_atom_into(&mut value, atom, state)?;
                                if let Some(value) = value {
                                    *$name = value;
                                }
                                return Ok(());
                            }
                            __counter += 1;
                        )*
                        Err(Error::new(ErrorKind::WrongLength, "too many elements in tuple"))
                    },
                    end: |_, len| {
                        if len == [$(stringify!($name)),*].len() {
                            Ok(())
                        } else {
                            Err(Error::new(ErrorKind::WrongLength, "not enough elements in tuple"))
                        }
                    },
                    container: |index, is_map, state| {
                        let mut __counter = 0;
                        $(
                            if index == __counter {
                                let mut value = None;
                                let mut sink = $adapter::deserialize_into(&mut value, state);
                                let rv = if is_map { sink.map(state) } else { sink.seq(state) };
                                return rv.expect_err("atoms do not accept containers");
                            }
                            __counter += 1;
                        )*
                        Error::new(ErrorKind::WrongLength, "too many elements in tuple")
                    },
                })
            }

            fn deserialize_into<'out>(out: &'out mut Option<($($name,)*)>, state: &mut State) -> SinkHandle<'out, 'de> {
                #![allow(non_snake_case)]

                struct TupleSink<'a, $($name,)* $($adapter,)*> {
                    slot: &'a mut Option<($($name,)*)>,
                    index: usize,
                    $(
                        $name: Option<$name>,
                    )*
                    _marker: PhantomData<fn() -> ($($adapter,)*)>,
                }

                impl<'de, 'a, $($name: Send,)* $($adapter: Deserialize<'de, $name>,)*> Sink<'de> for TupleSink<'a, $($name,)* $($adapter,)*> {
                    fn expecting(&self) -> Cow<'_, str> {
                        Cow::Borrowed(TUPLE_NAME)
                    }

                    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
                        Ok(())
                    }

                    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
                        let __index = self.index;
                        self.index += 1;
                        let mut __counter = 0;
                        $(
                            if __index == __counter {
                                return Ok($adapter::deserialize_into(&mut self.$name, state));
                            }
                            __counter += 1;
                        )*
                        Err(Error::new(ErrorKind::WrongLength, "too many elements in tuple"))
                    }

                    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                        let __index = self.index;
                        self.index += 1;
                        let mut __counter = 0;
                        $(
                            if __index == __counter {
                                return $adapter::__private_atom_into(&mut self.$name, atom, state);
                            }
                            __counter += 1;
                        )*
                        Err(Error::new(ErrorKind::WrongLength, "too many elements in tuple"))
                    }

                    fn __private_borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
                        let __index = self.index;
                        self.index += 1;
                        let mut __counter = 0;
                        $(
                            if __index == __counter {
                                return $adapter::__private_borrowed_atom_into(&mut self.$name, atom, state);
                            }
                            __counter += 1;
                        )*
                        Err(Error::new(ErrorKind::WrongLength, "too many elements in tuple"))
                    }

                    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
                        *self.slot = Some(($(
                            self.$name
                                .take()
                                .ok_or_else(|| Error::new(ErrorKind::WrongLength, "not enough elements in tuple"))?,
                        )*));
                        Ok(())
                    }
                }

                // SAFETY: the adapters are adapters, the sink only holds a
                // marker of them
                unsafe {
                    SinkHandle::arena_unbounded(TupleSink::<$($name,)* $($adapter,)*> {
                        slot: out,
                        index: 0,
                        $(
                            $name: None,
                        )*
                        _marker: PhantomData,
                    }, state)
                }
            }
        }

        deserialize_for_tuple_peel!($(($name, $adapter),)*);
    )
}

macro_rules! deserialize_for_tuple_peel {
    ($first:tt, $($other:tt,)*) => (deserialize_for_tuple!($($other,)*);)
}

deserialize_for_tuple! {
    (T1, A1), (T2, A2), (T3, A3), (T4, A4), (T5, A5), (T6, A6),
    (T7, A7), (T8, A8), (T9, A9), (T10, A10), (T11, A11), (T12, A12),
}

impl<'de, T: Send, A: Deserialize<'de, T>, const N: usize> Deserialize<'de, [T; N]> for [A; N] {
    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(ARRAY_NAME)
    }

    // arrays of atoms are built inline, this behaves like the sink below
    #[inline]
    fn __private_inline_seq() -> Option<InlineSeq<[T; N]>> {
        A::__private_atom_default()?;
        Some(InlineSeq {
            start: |out| {
                *out = Some(core::array::from_fn(|_| {
                    A::__private_atom_default().unwrap()
                }));
            },
            atom: |out, index, atom, state| {
                if index >= N {
                    return Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ));
                }
                let mut value = None;
                A::__private_atom_into(&mut value, atom, state)?;
                if let (Some(array), Some(value)) = (out, value) {
                    array[index] = value;
                }
                Ok(())
            },
            end: |_, len| {
                if len == N {
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "not enough elements in array",
                    ))
                }
            },
            container: |index, is_map, state| {
                if index >= N {
                    return Error::new(ErrorKind::WrongLength, "too many elements in array");
                }
                let mut value = None;
                let mut sink = A::deserialize_into(&mut value, state);
                let rv = if is_map {
                    sink.map(state)
                } else {
                    sink.seq(state)
                };
                rv.expect_err("atoms do not accept containers")
            },
        })
    }

    fn deserialize_into<'out>(
        out: &'out mut Option<[T; N]>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        // Invariant: if `buffer` is `Some`, the first `index` elements of it
        // are initialized.  Once the buffer was moved into the slot, `buffer`
        // is `None`.
        struct ArraySink<'a, T, A, const N: usize> {
            slot: &'a mut Option<[T; N]>,
            buffer: Option<[MaybeUninit<T>; N]>,
            element: Option<T>,
            index: usize,
            is_seq: bool,
            _marker: PhantomData<fn() -> A>,
        }

        impl<'a, T, A, const N: usize> ArraySink<'a, T, A, N> {
            fn flush(&mut self) {
                if let Some(element) = self.element.take() {
                    // indexing panics if the sink is misused and too many
                    // elements are pushed or the buffer is already gone.
                    let buffer = self.buffer.as_mut().expect("array already finished");
                    buffer[self.index].write(element);
                    self.index += 1;
                }
            }
        }

        impl<'a, T, A, const N: usize> Drop for ArraySink<'a, T, A, N> {
            fn drop(&mut self) {
                if let Some(ref mut buffer) = self.buffer {
                    for elem in &mut buffer[..self.index] {
                        // SAFETY: the first `index` elements are initialized
                        unsafe { elem.assume_init_drop() };
                    }
                }
            }
        }

        impl<'de, 'a, T: Send + 'a, A: Deserialize<'de, T>, const N: usize> Sink<'de>
            for ArraySink<'a, T, A, N>
        {
            fn expecting(&self) -> Cow<'_, str> {
                Cow::Borrowed(ARRAY_NAME)
            }

            fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                match atom {
                    Atom::Bytes(value) => match A::__private_array_from_bytes::<N>(&value) {
                        Some(array) => {
                            *self.slot = Some(array);
                            Ok(())
                        }
                        None if A::__private_is_bytes() => Err(Error::new(
                            ErrorKind::WrongLength,
                            "byte array of wrong length",
                        )),
                        None => Err(Error::new(
                            ErrorKind::InvalidType,
                            format!("unexpected bytes, expected {}", self.expecting()),
                        )),
                    },
                    // formats without native bytes represent them as strings
                    Atom::Str(ref value) if A::__private_is_bytes() => {
                        let bytes = crate::adapters::bytes::decode_str(value, state)?;
                        match A::__private_array_from_bytes::<N>(&bytes) {
                            Some(array) => {
                                *self.slot = Some(array);
                                Ok(())
                            }
                            None => Err(Error::new(
                                ErrorKind::WrongLength,
                                "byte array of wrong length",
                            )),
                        }
                    }
                    other => default_atom(self, other, state),
                }
            }

            fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
                self.is_seq = true;
                Ok(())
            }

            fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
                self.flush();
                if self.index >= N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ))
                } else {
                    Ok(A::deserialize_into(&mut self.element, state))
                }
            }

            fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                self.flush();
                if self.index >= N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ))
                } else {
                    A::__private_atom_into(&mut self.element, atom, state)
                }
            }

            fn __private_borrowed_value_atom(
                &mut self,
                atom: Atom<'de>,
                state: &mut State,
            ) -> Result<(), Error> {
                self.flush();
                if self.index >= N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "too many elements in array",
                    ))
                } else {
                    A::__private_borrowed_atom_into(&mut self.element, atom, state)
                }
            }

            fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
                if !self.is_seq {
                    return Ok(());
                }
                self.flush();
                if self.index != N {
                    Err(Error::new(
                        ErrorKind::WrongLength,
                        "not enough elements in array",
                    ))
                } else if let Some(buffer) = self.buffer.take() {
                    // SAFETY: all `N` elements are initialized and ownership
                    // is transferred as the buffer was taken out of the sink.
                    // `MaybeUninit<T>` has the same layout as `T`.
                    let array = unsafe {
                        (&buffer as *const [MaybeUninit<T>; N])
                            .cast::<[T; N]>()
                            .read()
                    };
                    *self.slot = Some(array);
                    Ok(())
                } else {
                    Ok(())
                }
            }
        }

        // SAFETY: `A` is an adapter, the sink only holds a marker of it
        unsafe {
            SinkHandle::arena_unbounded(
                ArraySink::<T, A, N> {
                    slot: out,
                    // an array of `MaybeUninit` does not require initialization
                    buffer: Some(MaybeUninit::uninit().assume_init()),
                    element: None,
                    index: 0,
                    is_seq: false,
                    _marker: PhantomData,
                },
                state,
            )
        }
    }
}

/// A type that is deserialized as `T` and converted.
pub(crate) trait Via<T>: Sized + Send {
    /// Converts the deserialized value.
    fn convert(value: T) -> Result<Self, Error>;
}

/// Creates the sink for a type that is deserialized as `T` with an adapter.
#[inline]
pub(crate) fn via_handle<'a, 'de, T, U, A>(
    out: &'a mut Option<U>,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    T: Send + 'a,
    U: Via<T> + 'a,
    A: Deserialize<'de, T>,
{
    MappedSink::handle(
        out,
        OwnedSink::deserialize_as::<A>(state),
        U::convert,
        state,
    )
}

/// Deserializes an atom into a type that is deserialized as `T`.
#[inline]
pub(crate) fn via_atom_into<'de, T, U, A>(
    out: &mut Option<U>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error>
where
    T: Send,
    U: Via<T>,
    A: Deserialize<'de, T>,
{
    let mut inner = None;
    A::__private_atom_into(&mut inner, atom, state)?;
    if let Some(value) = inner {
        *out = Some(U::convert(value)?);
    }
    Ok(())
}

/// Deserializes a borrowed atom into a type that is deserialized as `T`.
#[inline]
pub(crate) fn via_borrowed_atom_into<'de, T, U, A>(
    out: &mut Option<U>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error>
where
    T: Send,
    U: Via<T>,
    A: Deserialize<'de, T>,
{
    let mut inner = None;
    A::__private_borrowed_atom_into(&mut inner, atom, state)?;
    if let Some(value) = inner {
        *out = Some(U::convert(value)?);
    }
    Ok(())
}

/// Implements `Deserialize` for types that implement [`Via`].
macro_rules! deserialize_via {
    ($([$($gen:tt)*] $ty:ty => $via:ty;)*) => {
        $(
            impl<'de, $($gen)*> $crate::de::Deserialize<'de> for $ty {
                #[inline]
                fn deserialize_into<'out>(out: &'out mut Option<Self>, state: &mut $crate::State) -> $crate::de::SinkHandle<'out, 'de> {
                    $crate::de::impls::via_handle::<$via, Self, $crate::adapters::Same>(out, state)
                }

                // the sink passes everything on to the sink of the value
                fn expecting() -> alloc::borrow::Cow<'static, str> {
                    <$via as $crate::de::Deserialize<'de>>::expecting()
                }

                #[inline]
                fn __private_atom_into(
                    out: &mut Option<Self>,
                    atom: $crate::Atom,
                    state: &mut $crate::State,
                ) -> Result<(), $crate::Error> {
                    $crate::de::impls::via_atom_into::<$via, Self, $crate::adapters::Same>(
                        out, atom, state,
                    )
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    out: &mut Option<Self>,
                    atom: $crate::Atom<'de>,
                    state: &mut $crate::State,
                ) -> Result<(), $crate::Error> {
                    $crate::de::impls::via_borrowed_atom_into::<$via, Self, $crate::adapters::Same>(
                        out, atom, state,
                    )
                }

                #[inline(always)]
                fn __private_raw() -> Option<&'static $crate::ext::RawFormatInfo> {
                    <$via as $crate::de::Deserialize<'de>>::__private_raw()
                }
            }
        )*
    };
}

pub(crate) use deserialize_via;

/// Implements `Deserialize` for wrappers of a single value, generic over
/// the adapter of the value.
macro_rules! deserialize_as_via {
    ($([$($bound:tt)*] $wrapper:ident $({ $($update:tt)* })?),*) => {
        $(
            impl<'de, T: $($bound)*, A: Deserialize<'de, T>> Deserialize<'de, $wrapper<T>> for $wrapper<A>
            where
                $wrapper<A>: Send,
            {
                #[inline]
                fn deserialize_into<'out>(out: &'out mut Option<$wrapper<T>>, state: &mut State) -> SinkHandle<'out, 'de> {
                    via_handle::<T, $wrapper<T>, A>(out, state)
                }

                fn expecting() -> Cow<'static, str> {
                    A::expecting()
                }

                #[inline]
                fn __private_atom_into(
                    out: &mut Option<$wrapper<T>>,
                    atom: Atom,
                    state: &mut State,
                ) -> Result<(), Error> {
                    via_atom_into::<T, $wrapper<T>, A>(out, atom, state)
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    out: &mut Option<$wrapper<T>>,
                    atom: Atom<'de>,
                    state: &mut State,
                ) -> Result<(), Error> {
                    via_borrowed_atom_into::<T, $wrapper<T>, A>(out, atom, state)
                }

                #[inline(always)]
                fn __private_raw() -> Option<&'static crate::ext::RawFormatInfo> {
                    A::__private_raw()
                }

                $($($update)*)?
            }
        )*
    };
}

impl<T: Send> Via<T> for Box<T> {
    #[inline]
    fn convert(value: T) -> Result<Self, Error> {
        Ok(Box::new(value))
    }
}

impl<T: Send + Sync> Via<T> for Arc<T> {
    #[inline]
    fn convert(value: T) -> Result<Self, Error> {
        Ok(Arc::new(value))
    }
}

impl Via<String> for Box<str> {
    #[inline]
    fn convert(value: String) -> Result<Self, Error> {
        Ok(value.into_boxed_str())
    }
}

impl Via<String> for Arc<str> {
    #[inline]
    fn convert(value: String) -> Result<Self, Error> {
        Ok(Arc::from(value))
    }
}

deserialize_via! {
    [] Box<str> => String;
    [] Arc<str> => String;
}

deserialize_as_via!(
    [Send] Box {
        /// Updates the value in the box.
        #[inline]
        fn deserialize_update<'out>(
            value: &'out mut Box<T>,
            state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            A::deserialize_update(&mut **value, state)
        }
    },
    [Send + Sync] Arc
);

impl<'a, T: ToOwned + Sync + ?Sized> Via<T::Owned> for Cow<'a, T>
where
    T::Owned: Send,
{
    #[inline]
    fn convert(value: T::Owned) -> Result<Self, Error> {
        Ok(Cow::Owned(value))
    }
}

/// `Cow` is always deserialized owned so that for instance
/// `Cow<'static, str>` can be deserialized from any data.  To borrow use
/// the [`Borrowed`](crate::adapters::Borrowed) adapter.
impl<'de, 'a, T> Deserialize<'de> for Cow<'a, T>
where
    T: ToOwned + Sync + ?Sized,
    T::Owned: Deserialize<'de>,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        via_handle::<T::Owned, Self, Same>(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        <T::Owned as Deserialize<'de>>::expecting()
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        via_atom_into::<T::Owned, Self, Same>(out, atom, state)
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        via_atom_into::<T::Owned, Self, Same>(out, atom, state)
    }
}

/// Creates the error for owned data where borrowed data is expected.
#[cold]
fn expected_borrowed(what: &str) -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        format!(
            "unexpected owned {what}, expected a borrowed {what} (the data format \
             or the type buffering the value does not support borrowing)"
        ),
    )
}

impl<'de: 'a, 'a> Deserialize<'de> for &'a str {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(_) | Atom::Lexical(_) | Atom::Implicit(_) => Err(expected_borrowed("string")),
            other => default_atom(slot, other, state),
        }
    }

    fn deserialize_borrowed_atom(
        slot: &mut Slot<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            Atom::Str(ref text) | Atom::Lexical(ref text) if text.is_borrowed() => {
                **slot = text.borrowed_str();
                Ok(())
            }
            // strings take the text of values whose type was inferred
            Atom::Implicit(ref value) if value.text().is_borrowed() => {
                **slot = value.text().borrowed_str();
                Ok(())
            }
            other => Self::deserialize_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("str")
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for &'a [u8] {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(_) => Err(expected_borrowed("bytes")),
            Atom::Str(_) => Err(Error::new(
                ErrorKind::UnsupportedType,
                "unexpected string, expected borrowed bytes (bytes cannot be borrowed \
                 from strings, use Vec<u8> or Cow<[u8]> instead)",
            )),
            other => default_atom(slot, other, state),
        }
    }

    fn deserialize_borrowed_atom(
        slot: &mut Slot<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            Atom::Bytes(ref value) if value.is_borrowed() => {
                **slot = value.borrowed_data();
                Ok(())
            }
            other => Self::deserialize_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("bytes")
    }
}
