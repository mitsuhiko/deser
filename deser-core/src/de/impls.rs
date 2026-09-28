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
use crate::adapters::{DeserializeAs, Same};
use crate::de::lexical;
use crate::de::mapped::MappedSink;
use crate::de::update::Collection;
use crate::de::{CollectedErrors, DuplicateKeys};
use crate::de::{
    Deserialize, OwnedSink, Sink, SinkHandle, empty_lexical_or_none, is_empty_lexical, is_null_atom,
};
use crate::de::{atom_into_handle, borrowed_atom_into_handle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ImplicitValue};
use crate::ext::Number;

make_slot_wrapper!(SlotWrapper);

macro_rules! deserialize {
    ($ty:ty) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize_into<'out>(
                out: &'out mut Option<Self>,
                _state: &mut State,
            ) -> SinkHandle<'out, 'de> {
                SlotWrapper::make_handle(out)
            }

            __slot_wrapper_atom_into!();
        }
    };
}

/// Implements `__private_atom_into` for types using a slot wrapper.
macro_rules! __slot_wrapper_atom_into {
    () => {
        #[inline]
        fn __private_atom_into(
            out: &mut Option<Self>,
            atom: Atom,
            state: &mut State,
        ) -> Result<(), Error> {
            let sink = SlotWrapper::wrap(out);
            sink.atom(atom, state)?;
            sink.finish(state)
        }

        #[inline]
        fn __private_borrowed_atom_into(
            out: &mut Option<Self>,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            // the sink does not borrow
            Self::__private_atom_into(out, atom, state)
        }
    };
}

impl<'de> Sink<'de> for SlotWrapper<()> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("null")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Null => {
                **self = Some(());
                Ok(())
            }
            Atom::Lexical(ref value) if lexical::is_empty_null(value, state) => {
                **self = Some(());
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(());

impl<'de> Sink<'de> for SlotWrapper<bool> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("bool")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bool(value) => {
                **self = Some(value);
                Ok(())
            }
            Atom::Lexical(ref value) => {
                **self = Some(lexical::parse_bool(value, state)?);
                Ok(())
            }
            // the text of other values is not a bool either
            Atom::Implicit(ref value) => match value.value() {
                ImplicitValue::Bool(value) => {
                    **self = Some(value);
                    Ok(())
                }
                _ => self.unexpected_atom(atom, state),
            },
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(bool);

impl<'de> Sink<'de> for SlotWrapper<String> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("string")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) | Atom::Lexical(value) => {
                **self = Some(match value.into_cow() {
                    Cow::Borrowed(value) => copy_str(value),
                    Cow::Owned(value) => value,
                });
                Ok(())
            }
            Atom::Char(value) => {
                **self = Some(value.to_string());
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(String);

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

macro_rules! int_sink {
    ($ty:ty) => {
        impl<'de> Sink<'de> for SlotWrapper<$ty> {
            fn expecting(&self) -> Cow<'_, str> {
                Cow::Borrowed(stringify!($ty))
            }

            #[allow(clippy::useless_conversion)]
            fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
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
                        _ => return self.unexpected_atom(atom, state),
                    },
                    other => return self.unexpected_atom(other, state),
                };
                **self = Some(value);
                Ok(())
            }
        }
    };
}

int_sink!(u8);

impl<'de> Deserialize<'de> for u8 {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SlotWrapper::make_handle(out)
    }

    __slot_wrapper_atom_into!();

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
int_sink!(u128);
deserialize!(u128);
int_sink!(i128);
deserialize!(i128);

impl<'de> Sink<'de> for SlotWrapper<char> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("char")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Char(value) => {
                **self = Some(value);
                Ok(())
            }
            Atom::Str(ref s) => {
                let mut chars = s.chars();
                if let Some(first_char) = chars.next()
                    && chars.next().is_none()
                {
                    **self = Some(first_char);
                    return Ok(());
                }
                Err(atom.unexpected_error(&self.expecting()))
            }
            other => self.unexpected_atom(other, state),
        }
    }
}
deserialize!(char);

macro_rules! float_sink {
    ($ty:ty) => {
        impl<'de> Sink<'de> for SlotWrapper<$ty> {
            fn expecting(&self) -> Cow<'_, str> {
                Cow::Borrowed(stringify!($ty))
            }

            #[inline]
            fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                // Keep the owning variants out of the inlined numeric path.
                // This is not cold: JSON commonly emits Number extensions.
                #[inline(never)]
                fn other(
                    sink: &mut SlotWrapper<$ty>,
                    atom: Atom,
                    state: &mut State,
                ) -> Result<(), Error> {
                    let value = match atom {
                        Atom::Ext(ext) => match number_value(&ext) {
                            Some(value) => value as $ty,
                            None if ext.is::<u128>() => *ext.downcast_ref::<u128>().unwrap() as $ty,
                            None if ext.is::<i128>() => *ext.downcast_ref::<i128>().unwrap() as $ty,
                            None => return sink.unexpected_atom(Atom::Ext(ext), state),
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
                            _ => return sink.unexpected_atom(atom, state),
                        },
                        other => return sink.unexpected_atom(other, state),
                    };
                    **sink = Some(value);
                    Ok(())
                }

                let value = match atom {
                    Atom::U64(value) => value as $ty,
                    Atom::I64(value) => value as $ty,
                    Atom::F64(value) => value as $ty,
                    Atom::F32(value) => value as $ty,
                    atom => return other(self, atom, state),
                };
                // Only variants with Copy payloads reach here. Avoid calling
                // Atom's out-of-line drop glue, which has nothing to drop.
                core::mem::forget(atom);
                **self = Some(value);
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

float_sink!(f32);
deserialize!(f32);

float_sink!(f64);
deserialize!(f64);

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
    A: DeserializeAs<'de, T>,
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

    impl<'de, 'a, C: SeqTarget<T>, T: Send, A: DeserializeAs<'de, T>> Sink<'de>
        for SeqSink<'a, C, T, A>
    {
        fn expecting(&self) -> Cow<'_, str> {
            Cow::Borrowed(if A::__private_is_bytes_as() {
                "bytes"
            } else {
                C::NAME
            })
        }

        fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            match atom {
                Atom::Bytes(value) => match A::__private_vec_from_bytes_as(value.into_owned()) {
                    Some(vec) => {
                        *self.slot = Some(C::from_vec(vec)?);
                        Ok(())
                    }
                    None => Err(Error::new(
                        ErrorKind::Unexpected,
                        format!("unexpected bytes, expected {}", self.expecting()),
                    )),
                },
                // formats without native bytes represent them as strings
                Atom::Str(ref value) if A::__private_is_bytes_as() => {
                    let bytes = crate::adapters::bytes::decode_str(value, state)?;
                    match A::__private_vec_from_bytes_as(bytes) {
                        Some(vec) => {
                            *self.slot = Some(C::from_vec(vec)?);
                            Ok(())
                        }
                        None => self.unexpected_atom(atom, state),
                    }
                }
                other => self.unexpected_atom(other, state),
            }
        }

        fn seq(&mut self, state: &mut State) -> Result<(), Error> {
            self.is_seq = true;
            self.vec.reserve(cautious_capacity::<T>(state));
            Ok(())
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush();
            Ok(A::deserialize_into_as(&mut self.element, state))
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush();
            A::__private_atom_into_as(&mut self.element, atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush();
            A::__private_borrowed_atom_into_as(&mut self.element, atom, state)
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

    SinkHandle::arena(
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

/// The methods of `Deserialize` for collections that collect the values of
/// a repeated key (see [`Collection`]), with the adapter of the elements.
///
/// Sequences of bytes (like `Vec<u8>`) do not collect, the value of their
/// key is the bytes.
macro_rules! collection_methods {
    ($elem:ty) => {
        #[inline]
        fn __private_collects() -> bool {
            !<$elem as DeserializeAs<'de, T>>::__private_is_bytes_as()
        }

        collection_methods!(@common $elem);
    };
    (set $elem:ty) => {
        #[inline]
        fn __private_collects() -> bool {
            true
        }

        collection_methods!(@common $elem);
    };
    (@common $elem:ty) => {

        fn __private_collect_into<'out>(
            out: &'out mut Option<Self>,
            state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            crate::de::update::collect_into::<Self, T, $elem>(out, state)
        }

        fn __private_collect_update<'out>(
            value: &'out mut Self,
            first: bool,
            state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            crate::de::update::collect_update::<Self, T, $elem>(value, first, state)
        }

        fn __private_collect_empty() -> Option<Self> {
            Some(<Self as crate::de::update::Collection<T>>::empty())
        }
    };
}

/// The methods of `DeserializeAs` for collections that collect the values
/// of a repeated key, the adapter of the elements is `A`.
macro_rules! collection_methods_as {
    ($target:ty) => {
        #[inline]
        fn __private_collects_as() -> bool {
            !A::__private_is_bytes_as()
        }

        collection_methods_as!(@common $target);
    };
    (set $target:ty) => {
        #[inline]
        fn __private_collects_as() -> bool {
            true
        }

        collection_methods_as!(@common $target);
    };
    (@common $target:ty) => {

        fn __private_collect_into_as<'out>(
            out: &'out mut Option<$target>,
            state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            crate::de::update::collect_into::<$target, T, A>(out, state)
        }

        fn __private_collect_update_as<'out>(
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

        fn __private_collect_empty_as() -> Option<$target> {
            Some(<$target as crate::de::update::Collection<T>>::empty())
        }
    };
}

#[allow(unused_imports)]
pub(crate) use {collection_methods, collection_methods_as};

/// Implements `Deserialize` and `DeserializeAs` for sequences.
///
/// Sequences marked with `collect` collect the values of repeated keys.
macro_rules! deserialize_seq {
    ($($($collect:ident)? [$($bound:tt)*] $target:ty => $adapter:ty;)*) => {
        $(
            impl<'de, $($bound)*> Deserialize<'de> for $target
            where
                T: Deserialize<'de>,
            {
                #[inline]
                fn deserialize_into<'out>(out: &'out mut Option<Self>, state: &mut State) -> SinkHandle<'out, 'de> {
                    seq_sink::<Self, T, Same>(out, state)
                }

                $(deserialize_seq! { @$collect de })?
            }

            impl<'de, $($bound)*, A: DeserializeAs<'de, T>> DeserializeAs<'de, $target> for $adapter {
                fn deserialize_into_as<'out>(out: &'out mut Option<$target>, state: &mut State) -> SinkHandle<'out, 'de> {
                    seq_sink::<$target, T, A>(out, state)
                }

                $(deserialize_seq! { @$collect adapter $target })?
            }
        )*
    };
    (@collect de) => { collection_methods!(Same); };
    (@collect adapter $target:ty) => { collection_methods_as!($target); };
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

/// The maximum number of bytes that are preallocated for the declared
/// length of a container.
///
/// The length comes from the input which is not trusted.
const MAX_PREALLOCATION: usize = 1024 * 1024;

/// Returns the number of elements to preallocate for a container.
#[inline]
fn cautious_capacity<T>(state: &State) -> usize {
    match state.container_shape().len() {
        Some(len) => len.min(MAX_PREALLOCATION / core::mem::size_of::<T>().max(1)),
        None => 0,
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
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
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
            VA: DeserializeAs<'de, V>,
            V: Send,
        {
            if let Some(ref key) = self.key
                && let Some(value) = self.map.entry_mut(key)
            {
                return VA::__private_collect_update_as(value, false, state);
            }
            VA::__private_collect_into_as(&mut self.value, state)
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
        KA: DeserializeAs<'de, K>,
        VA: DeserializeAs<'de, V>,
    {
        fn expecting(&self) -> Cow<'_, str> {
            Cow::Borrowed(M::NAME)
        }

        fn map(&mut self, state: &mut State) -> Result<(), Error> {
            self.map.reserve_entries(cautious_capacity::<(K, V)>(state));
            self.duplicate_keys = state.duplicate_keys();
            Ok(())
        }

        fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush_before(state)?;
            Ok(KA::deserialize_into_as(&mut self.key, state))
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            if VA::__private_collects_as() && state.is_multimap() {
                return Ok(self.collect_value(state));
            }
            Ok(VA::deserialize_into_as(&mut self.value, state))
        }

        fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush_before(state)?;
            KA::__private_atom_into_as(&mut self.key, atom, state)
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            if VA::__private_collects_as() && state.is_multimap() {
                return atom_into_handle(self.collect_value(state), atom, state);
            }
            VA::__private_atom_into_as(&mut self.value, atom, state)
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush_before(state)?;
            KA::__private_borrowed_atom_into_as(&mut self.key, atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            if VA::__private_collects_as() && state.is_multimap() {
                return borrowed_atom_into_handle(self.collect_value(state), atom, state);
            }
            VA::__private_borrowed_atom_into_as(&mut self.value, atom, state)
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
            self.duplicate_keys = state.duplicate_keys();
            self.flush_before(state)?;
            KA::__private_atom_into_as(&mut self.key, Atom::Lexical(Text::borrowed(key)), state)?;
            if VA::__private_collects_as() && state.is_multimap() {
                return Ok(Some(self.collect_value(state)));
            }
            Ok(Some(VA::deserialize_into_as(&mut self.value, state)))
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

    SinkHandle::arena(
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

impl<'de, K, V> Deserialize<'de> for BTreeMap<K, V>
where
    K: Ord + Deserialize<'de>,
    V: Deserialize<'de>,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Slot(out), state)
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).
    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Update(value), state)
    }
}

impl<'de, K, V, KA, VA> DeserializeAs<'de, BTreeMap<K, V>> for BTreeMap<KA, VA>
where
    K: Ord + Send,
    V: Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as<'out>(
        out: &'out mut Option<BTreeMap<K, V>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out), state)
    }
}

#[cfg(feature = "std")]
impl<'de, K, V, H> Deserialize<'de> for HashMap<K, V, H>
where
    K: Hash + Eq + Deserialize<'de>,
    V: Deserialize<'de>,
    H: BuildHasher + Default + Send,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Slot(out), state)
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).
    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Update(value), state)
    }
}

#[cfg(feature = "std")]
impl<'de, K, V, H, KA, VA> DeserializeAs<'de, HashMap<K, V, H>> for HashMap<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    H: BuildHasher + Default + Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as<'out>(
        out: &'out mut Option<HashMap<K, V, H>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out), state)
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
    A: DeserializeAs<'de, T>,
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

    impl<'de, 'a, S: SetTarget<T>, T: Send, A: DeserializeAs<'de, T>> Sink<'de>
        for SetSink<'a, S, T, A>
    {
        fn expecting(&self) -> Cow<'_, str> {
            Cow::Borrowed(S::NAME)
        }

        fn seq(&mut self, state: &mut State) -> Result<(), Error> {
            self.set.reserve_elements(cautious_capacity::<T>(state));
            Ok(())
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush();
            Ok(A::deserialize_into_as(&mut self.element, state))
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush();
            A::__private_atom_into_as(&mut self.element, atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush();
            A::__private_borrowed_atom_into_as(&mut self.element, atom, state)
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

    SinkHandle::arena(
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

impl<'de, T: Deserialize<'de> + Ord> Deserialize<'de> for BTreeSet<T> {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, Same>(out, state)
    }

    collection_methods!(set Same);
}

impl<'de, T: Ord + Send, A: DeserializeAs<'de, T>> DeserializeAs<'de, BTreeSet<T>> for BTreeSet<A> {
    fn deserialize_into_as<'out>(
        out: &'out mut Option<BTreeSet<T>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, A>(out, state)
    }

    collection_methods_as!(set BTreeSet<T>);
}

#[cfg(feature = "std")]
impl<'de, T, H> Deserialize<'de> for HashSet<T, H>
where
    T: Deserialize<'de> + Hash + Eq,
    H: BuildHasher + Default + Send,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, Same>(out, state)
    }

    collection_methods!(set Same);
}

#[cfg(feature = "std")]
impl<'de, T, H, A> DeserializeAs<'de, HashSet<T, H>> for HashSet<A>
where
    T: Hash + Eq + Send,
    H: BuildHasher + Default + Send,
    A: DeserializeAs<'de, T>,
{
    fn deserialize_into_as<'out>(
        out: &'out mut Option<HashSet<T, H>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, A>(out, state)
    }

    collection_methods_as!(set HashSet<T, H>);
}

impl<'de, T> Deserialize<'de> for Option<T>
where
    T: Deserialize<'de>,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <Option<Same> as DeserializeAs<'de, Option<T>>>::deserialize_into_as(out, state)
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        <Option<Same> as DeserializeAs<'de, Option<T>>>::__private_atom_into_as(out, atom, state)
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        <Option<Same> as DeserializeAs<'de, Option<T>>>::__private_borrowed_atom_into_as(
            out, atom, state,
        )
    }

    fn initial_value() -> Option<Self> {
        Some(None)
    }

    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        crate::de::update::update_option(value, state)
    }

    #[inline]
    fn __private_collects() -> bool {
        T::__private_collects()
    }

    fn __private_collect_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <Option<Same> as DeserializeAs<'de, Option<T>>>::__private_collect_into_as(out, state)
    }

    fn __private_collect_update<'out>(
        value: &'out mut Self,
        first: bool,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <Option<Same> as DeserializeAs<'de, Option<T>>>::__private_collect_update_as(
            value, first, state,
        )
    }
}

impl<'de, T, A: DeserializeAs<'de, T>> DeserializeAs<'de, Option<T>> for Option<A> {
    #[inline]
    fn deserialize_into_as<'out>(
        out: &'out mut Option<Option<T>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        A::deserialize_into_as(out.insert(None), state).ignore_null()
    }

    // An optional collection collects into the collection, it's `None` if
    // its key is missing.  Values that are null (or empty text for types
    // that do not accept it) are not added.

    #[inline]
    fn __private_collects_as() -> bool {
        A::__private_collects_as()
    }

    fn __private_collect_into_as<'out>(
        out: &'out mut Option<Option<T>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        A::__private_collect_into_as(out.get_or_insert(None), state).ignore_null()
    }

    fn __private_collect_update_as<'out>(
        value: &'out mut Option<T>,
        first: bool,
        state: &mut State,
    ) -> SinkHandle<'out, 'de>
    where
        Option<T>: Send,
    {
        if first {
            *value = None;
        }
        A::__private_collect_into_as(value, state).ignore_null()
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<Option<T>>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let inner = out.insert(None);
        if is_null_atom(&atom) {
            // the sink is created (and dropped without being used) so that
            // this behaves exactly like `deserialize_into`.  This matters
            // for nested options where the inner one becomes `Some(None)`.
            drop(A::deserialize_into_as(inner, state));
            Ok(())
        } else if is_empty_lexical(&atom, state) {
            if !empty_lexical_or_none(atom, state, |atom, state| {
                A::__private_atom_into_as(inner, atom, state)
            })? {
                *inner = None;
            }
            Ok(())
        } else {
            A::__private_atom_into_as(inner, atom, state)
        }
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<Option<T>>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let inner = out.insert(None);
        if is_null_atom(&atom) {
            drop(A::deserialize_into_as(inner, state));
            Ok(())
        } else if is_empty_lexical(&atom, state) {
            if !empty_lexical_or_none(atom, state, |atom, state| {
                A::__private_borrowed_atom_into_as(inner, atom, state)
            })? {
                *inner = None;
            }
            Ok(())
        } else {
            A::__private_borrowed_atom_into_as(inner, atom, state)
        }
    }

    fn initial_value_as() -> Option<Option<T>> {
        Some(None)
    }
}

macro_rules! same_adapter {
    ($name:ident) => {
        Same
    };
}

macro_rules! deserialize_for_tuple {
    () => ();
    ($(($name:ident, $adapter:ident),)+) => (
        impl<'de, $($name: Deserialize<'de>),*> Deserialize<'de> for ($($name,)*) {
            #[inline]
            fn deserialize_into<'out>(out: &'out mut Option<Self>, state: &mut State) -> SinkHandle<'out, 'de> {
                <($(same_adapter!($name),)*) as DeserializeAs<'de, ($($name,)*)>>::deserialize_into_as(out, state)
            }
        }

        impl<'de, $($name: Send,)* $($adapter: DeserializeAs<'de, $name>),*> DeserializeAs<'de, ($($name,)*)> for ($($adapter,)*) {
            fn deserialize_into_as<'out>(out: &'out mut Option<($($name,)*)>, state: &mut State) -> SinkHandle<'out, 'de> {
                #![allow(non_snake_case)]

                struct TupleSink<'a, $($name,)* $($adapter,)*> {
                    slot: &'a mut Option<($($name,)*)>,
                    index: usize,
                    $(
                        $name: Option<$name>,
                    )*
                    _marker: PhantomData<fn() -> ($($adapter,)*)>,
                }

                impl<'de, 'a, $($name: Send,)* $($adapter: DeserializeAs<'de, $name>,)*> Sink<'de> for TupleSink<'a, $($name,)* $($adapter,)*> {
                    fn expecting(&self) -> Cow<'_, str> {
                        Cow::Borrowed("tuple")
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
                                return Ok($adapter::deserialize_into_as(&mut self.$name, state));
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
                                return $adapter::__private_atom_into_as(&mut self.$name, atom, state);
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
                                return $adapter::__private_borrowed_atom_into_as(&mut self.$name, atom, state);
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

                SinkHandle::arena(TupleSink::<$($name,)* $($adapter,)*> {
                    slot: out,
                    index: 0,
                    $(
                        $name: None,
                    )*
                    _marker: PhantomData,
                }, state)
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

impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for [T; N] {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        <[Same; N] as DeserializeAs<'de, [T; N]>>::deserialize_into_as(out, state)
    }
}

impl<'de, T: Send, A: DeserializeAs<'de, T>, const N: usize> DeserializeAs<'de, [T; N]> for [A; N] {
    fn deserialize_into_as<'out>(
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

        impl<'de, 'a, T: Send + 'a, A: DeserializeAs<'de, T>, const N: usize> Sink<'de>
            for ArraySink<'a, T, A, N>
        {
            fn expecting(&self) -> Cow<'_, str> {
                Cow::Borrowed("array")
            }

            fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                match atom {
                    Atom::Bytes(value) => match A::__private_array_from_bytes_as::<N>(&value) {
                        Some(array) => {
                            *self.slot = Some(array);
                            Ok(())
                        }
                        None if A::__private_is_bytes_as() => Err(Error::new(
                            ErrorKind::WrongLength,
                            "byte array of wrong length",
                        )),
                        None => Err(Error::new(
                            ErrorKind::Unexpected,
                            format!("unexpected bytes, expected {}", self.expecting()),
                        )),
                    },
                    // formats without native bytes represent them as strings
                    Atom::Str(ref value) if A::__private_is_bytes_as() => {
                        let bytes = crate::adapters::bytes::decode_str(value, state)?;
                        match A::__private_array_from_bytes_as::<N>(&bytes) {
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
                    other => self.unexpected_atom(other, state),
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
                    Ok(A::deserialize_into_as(&mut self.element, state))
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
                    A::__private_atom_into_as(&mut self.element, atom, state)
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
                    A::__private_borrowed_atom_into_as(&mut self.element, atom, state)
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

        SinkHandle::arena(
            ArraySink::<T, A, N> {
                slot: out,
                // SAFETY: an array of `MaybeUninit` does not require initialization
                buffer: Some(unsafe { MaybeUninit::uninit().assume_init() }),
                element: None,
                index: 0,
                is_seq: false,
                _marker: PhantomData,
            },
            state,
        )
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
    A: DeserializeAs<'de, T>,
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
    U: Via<T>,
    A: DeserializeAs<'de, T>,
{
    let mut inner = None;
    A::__private_atom_into_as(&mut inner, atom, state)?;
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
    U: Via<T>,
    A: DeserializeAs<'de, T>,
{
    let mut inner = None;
    A::__private_borrowed_atom_into_as(&mut inner, atom, state)?;
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
            }
        )*
    };
}

pub(crate) use deserialize_via;

/// Implements `DeserializeAs` for wrappers of a single value.
macro_rules! deserialize_as_via {
    ($([$($bound:tt)*] $wrapper:ident),*) => {
        $(
            impl<'de, T: $($bound)*, A: DeserializeAs<'de, T>> DeserializeAs<'de, $wrapper<T>> for $wrapper<A> {
                #[inline]
                fn deserialize_into_as<'out>(out: &'out mut Option<$wrapper<T>>, state: &mut State) -> SinkHandle<'out, 'de> {
                    via_handle::<T, $wrapper<T>, A>(out, state)
                }

                #[inline]
                fn __private_atom_into_as(
                    out: &mut Option<$wrapper<T>>,
                    atom: Atom,
                    state: &mut State,
                ) -> Result<(), Error> {
                    via_atom_into::<T, $wrapper<T>, A>(out, atom, state)
                }

                #[inline]
                fn __private_borrowed_atom_into_as(
                    out: &mut Option<$wrapper<T>>,
                    atom: Atom<'de>,
                    state: &mut State,
                ) -> Result<(), Error> {
                    via_borrowed_atom_into::<T, $wrapper<T>, A>(out, atom, state)
                }
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

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Box<T> {
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        via_handle::<T, Self, Same>(out, state)
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        via_atom_into::<T, Self, Same>(out, atom, state)
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        via_borrowed_atom_into::<T, Self, Same>(out, atom, state)
    }

    /// Updates the value in the box.
    #[inline]
    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        T::deserialize_update(value, state)
    }
}

deserialize_via! {
    [T: Deserialize<'de> + Sync] Arc<T> => T;
    [] Box<str> => String;
    [] Arc<str> => String;
}

deserialize_as_via!([Send] Box, [Send + Sync] Arc);

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
        ErrorKind::Unexpected,
        format!(
            "unexpected owned {what}, expected a borrowed {what} (the data format \
             or the type buffering the value does not support borrowing)"
        ),
    )
}

impl<'de: 'a, 'a> Sink<'de> for SlotWrapper<&'a str> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("str")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(_) | Atom::Lexical(_) | Atom::Implicit(_) => Err(expected_borrowed("string")),
            other => self.unexpected_atom(other, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref text) | Atom::Lexical(ref text) if text.is_borrowed() => {
                **self = text.borrowed_str();
                Ok(())
            }
            // strings take the text of values whose type was inferred
            Atom::Implicit(ref value) if value.text().is_borrowed() => {
                **self = value.text().borrowed_str();
                Ok(())
            }
            other => self.atom(other, state),
        }
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for &'a str {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SlotWrapper::make_handle(out)
    }
}

impl<'de: 'a, 'a> Sink<'de> for SlotWrapper<&'a [u8]> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("bytes")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(_) => Err(expected_borrowed("bytes")),
            Atom::Str(_) => Err(Error::new(
                ErrorKind::Unexpected,
                "unexpected string, expected borrowed bytes (bytes cannot be borrowed \
                 from strings, use Vec<u8> or Cow<[u8]> instead)",
            )),
            other => self.unexpected_atom(other, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(ref value) if value.is_borrowed() => {
                **self = value.borrowed_data();
                Ok(())
            }
            other => self.atom(other, state),
        }
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for &'a [u8] {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SlotWrapper::make_handle(out)
    }
}
