//! The adapters provided by deser.
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::hash::{BuildHasher, Hash};
use std::marker::PhantomData;
use std::mem::take;
use std::str::FromStr;

use crate::adapters::{DeserializeAs, Same, SerializeAs};
use crate::de::impls::MapTarget;
use crate::de::mapped::MappedSink;
use crate::de::{Deserialize, OwnedSink, Recording, Sink, SinkHandle};
use crate::descriptors::{Descriptor, NamedDescriptor};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::{Begin, Chunk, Serialize, SerializeHandle};
use crate::State;

/// Deserializes a `Cow<str>` or `Cow<[u8]>` borrowed from the data.
///
/// `Cow` is deserialized owned by default so that `Cow<'static, str>` can
/// be deserialized from any data.  With this adapter the data is borrowed
/// if the data format passes it on borrowed (see
/// [`Sink::borrowed_atom`]).  Otherwise it's owned.  Serialization is not
/// affected.
///
/// ```
/// use std::borrow::Cow;
/// use deser::{Deserialize, Serialize};
/// use deser::adapters::Borrowed;
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Message<'a> {
///     #[deser(as = Borrowed)]
///     text: Cow<'a, str>,
///     #[deser(as = Vec<Borrowed>)]
///     tags: Vec<Cow<'a, str>>,
/// }
/// ```
pub struct Borrowed;

make_slot_wrapper!(BorrowedSlot);

impl<'de: 'a, 'a> Sink<'de> for BorrowedSlot<Cow<'a, str>> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "string" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                **self = Some(Cow::Owned(value.into_owned()));
                Ok(())
            }
            Atom::Char(value) => {
                **self = Some(Cow::Owned(value.to_string()));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                **self = Some(value);
                Ok(())
            }
            other => self.atom(other, state),
        }
    }
}

impl<'de: 'a, 'a> Sink<'de> for BorrowedSlot<Cow<'a, [u8]>> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "bytes" };
        &DESCRIPTOR
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(value) => {
                **self = Some(Cow::Owned(value.into_owned()));
                Ok(())
            }
            // formats without native bytes represent them as strings
            Atom::Str(ref value) => {
                **self = Some(Cow::Owned(crate::bytes::decode_str(value, state)?));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(value) => {
                **self = Some(value);
                Ok(())
            }
            other => self.atom(other, state),
        }
    }
}

impl<'de: 'a, 'a> DeserializeAs<'de, Cow<'a, str>> for Borrowed {
    fn deserialize_into_as<'b>(out: &'b mut Option<Cow<'a, str>>) -> SinkHandle<'b, 'de> {
        BorrowedSlot::make_handle(out)
    }
}

impl<'de: 'a, 'a> DeserializeAs<'de, Cow<'a, [u8]>> for Borrowed {
    fn deserialize_into_as<'b>(out: &'b mut Option<Cow<'a, [u8]>>) -> SinkHandle<'b, 'de> {
        BorrowedSlot::make_handle(out)
    }
}

impl<'a> SerializeAs<Cow<'a, str>> for Borrowed {
    fn serialize_as<'b>(value: &'b Cow<'a, str>, _state: &mut State) -> Result<Chunk<'b>, Error> {
        Ok(Chunk::Atom(Atom::Str(Cow::Borrowed(value))))
    }
}

impl<'a> SerializeAs<Cow<'a, [u8]>> for Borrowed {
    fn serialize_as<'b>(value: &'b Cow<'a, [u8]>, _state: &mut State) -> Result<Chunk<'b>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(Cow::Borrowed(value))))
    }
}

/// Serializes with [`Display`] and deserializes with [`FromStr`].
///
/// The value is represented as a string.  Only strings are accepted when
/// deserializing.
///
/// ```
/// use std::net::IpAddr;
/// use deser::{Deserialize, Serialize};
/// use deser::adapters::DisplayFromStr;
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Server {
///     #[deser(as = DisplayFromStr)]
///     addr: IpAddr,
/// }
/// ```
pub struct DisplayFromStr;

make_slot_wrapper!(FromStrSlot);

impl<'de, T> Sink<'de> for FromStrSlot<T>
where
    T: FromStr,
    T::Err: Display,
{
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref value) => match value.parse::<T>() {
                Ok(value) => {
                    **self = Some(value);
                    Ok(())
                }
                Err(err) => Err(Error::new(
                    ErrorKind::Unexpected,
                    format!("invalid value: {}", err),
                )),
            },
            other => self.unexpected_atom(other, state),
        }
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("string")
    }
}

impl<'de, T> DeserializeAs<'de, T> for DisplayFromStr
where
    T: FromStr,
    T::Err: Display,
{
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        FromStrSlot::make_handle(out)
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let sink = FromStrSlot::wrap(out);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        // the value is parsed, it does not borrow
        Self::__private_atom_into_as(out, atom, state)
    }
}

impl<T: Display + ?Sized> SerializeAs<T> for DisplayFromStr {
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Str(Cow::Owned(value.to_string()))))
    }

    fn descriptor_as(_value: &T) -> &'static dyn Descriptor {
        static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "str" };
        &DESCRIPTOR
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            Self::descriptor_as(value),
            false,
        ))
    }
}

/// Converts from and into another type.
///
/// The value is deserialized as `U` and converted with [`Into`] and it's
/// serialized by cloning it and converting it into `U`.
///
/// ```
/// use deser::{Deserialize, Serialize};
/// use deser::adapters::FromInto;
///
/// #[derive(Clone)]
/// pub struct Rgb(u8, u8, u8);
///
/// impl From<(u8, u8, u8)> for Rgb {
///     fn from(value: (u8, u8, u8)) -> Rgb {
///         Rgb(value.0, value.1, value.2)
///     }
/// }
///
/// impl From<Rgb> for (u8, u8, u8) {
///     fn from(value: Rgb) -> (u8, u8, u8) {
///         (value.0, value.1, value.2)
///     }
/// }
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Theme {
///     #[deser(as = FromInto<(u8, u8, u8)>)]
///     background: Rgb,
/// }
/// ```
pub struct FromInto<U>(PhantomData<fn() -> U>);

impl<'de, T, U> DeserializeAs<'de, T> for FromInto<U>
where
    U: Deserialize<'de> + Into<T> + 'static,
{
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        MappedSink::handle(out, OwnedSink::<U>::deserialize(), |value| Ok(value.into()))
    }

    fn initial_value_as() -> Option<T> {
        U::initial_value().map(Into::into)
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut inner = None;
        U::__private_atom_into(&mut inner, atom, state)?;
        *out = inner.map(Into::into);
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut inner = None;
        U::__private_borrowed_atom_into(&mut inner, atom, state)?;
        *out = inner.map(Into::into);
        Ok(())
    }
}

impl<T, U> SerializeAs<T> for FromInto<U>
where
    T: Clone + Into<U>,
    U: Serialize + 'static,
{
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Forward(SerializeHandle::boxed(Into::<U>::into(
            value.clone(),
        ))))
    }

    fn is_optional_as(value: &T) -> bool {
        Into::<U>::into(value.clone()).is_optional()
    }
}

/// Converts from and into another type with fallible conversions.
///
/// This is like [`FromInto`] but uses [`TryFrom`] and [`TryInto`].  Failed
/// conversions are reported as errors.
///
/// ```
/// use deser::{Deserialize, Serialize};
/// use deser::adapters::TryFromInto;
///
/// #[derive(Clone)]
/// pub struct Percent(u8);
///
/// impl TryFrom<u64> for Percent {
///     type Error = &'static str;
///     fn try_from(value: u64) -> Result<Percent, Self::Error> {
///         if value <= 100 { Ok(Percent(value as u8)) } else { Err("out of range") }
///     }
/// }
///
/// impl TryFrom<Percent> for u64 {
///     type Error = std::convert::Infallible;
///     fn try_from(value: Percent) -> Result<u64, Self::Error> {
///         Ok(value.0 as u64)
///     }
/// }
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Progress {
///     #[deser(as = TryFromInto<u64>)]
///     done: Percent,
/// }
/// ```
pub struct TryFromInto<U>(PhantomData<fn() -> U>);

fn conversion_error<E: Display>(err: E) -> Error {
    Error::new(ErrorKind::Unexpected, format!("invalid value: {}", err))
}

impl<'de, T, U> DeserializeAs<'de, T> for TryFromInto<U>
where
    U: Deserialize<'de> + 'static,
    T: TryFrom<U>,
    T::Error: Display,
{
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        MappedSink::handle(out, OwnedSink::<U>::deserialize(), |value| {
            T::try_from(value).map_err(conversion_error)
        })
    }

    fn initial_value_as() -> Option<T> {
        U::initial_value().and_then(|value| T::try_from(value).ok())
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut inner = None;
        U::__private_atom_into(&mut inner, atom, state)?;
        *out = match inner {
            Some(value) => Some(T::try_from(value).map_err(conversion_error)?),
            None => None,
        };
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut inner = None;
        U::__private_borrowed_atom_into(&mut inner, atom, state)?;
        *out = match inner {
            Some(value) => Some(T::try_from(value).map_err(conversion_error)?),
            None => None,
        };
        Ok(())
    }
}

impl<T, U> SerializeAs<T> for TryFromInto<U>
where
    T: Clone + TryInto<U>,
    <T as TryInto<U>>::Error: Display,
    U: Serialize + 'static,
{
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        let value: U = value.clone().try_into().map_err(conversion_error)?;
        Ok(Chunk::Forward(SerializeHandle::boxed(value)))
    }

    fn is_optional_as(value: &T) -> bool {
        TryInto::<U>::try_into(value.clone()).is_ok_and(|x| x.is_optional())
    }
}

/// Uses the [`Default`] if a value cannot be deserialized.
///
/// The value is deserialized with the adapter `A` (by default [`Same`]).  If
/// that fails, the default value is used instead.  Compound values are
/// buffered in a [`Recording`] as the error can only be detected once they
/// were seen fully.  Missing values are handled by the inner adapter.
/// Serialization uses the inner adapter.
///
/// ```
/// use deser::Deserialize;
/// use deser::adapters::DefaultOnError;
///
/// #[derive(Deserialize)]
/// pub enum Kind {
///     A,
///     B,
/// }
///
/// #[derive(Deserialize)]
/// pub struct Item {
///     // unknown kinds become `None`
///     #[deser(as = DefaultOnError)]
///     kind: Option<Kind>,
/// }
/// ```
pub struct DefaultOnError<A = Same>(PhantomData<fn() -> A>);

impl<'de, T: Default, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for DefaultOnError<A> {
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        Recording::capture(move |recording, state| {
            let mut value = None;
            let rv = recording.replay(A::deserialize_into_as(&mut value), state);
            *out = Some(match (rv, value) {
                (Ok(()), Some(value)) => value,
                _ => T::default(),
            });
            Ok(())
        })
    }

    fn initial_value_as() -> Option<T> {
        A::initial_value_as()
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut value = None;
        *out = Some(
            match (A::__private_atom_into_as(&mut value, atom, state), value) {
                (Ok(()), Some(value)) => value,
                _ => T::default(),
            },
        );
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut value = None;
        *out = Some(
            match (
                A::__private_borrowed_atom_into_as(&mut value, atom, state),
                value,
            ) {
                (Ok(()), Some(value)) => value,
                _ => T::default(),
            },
        );
        Ok(())
    }
}

impl<T: ?Sized, A: SerializeAs<T>> SerializeAs<T> for DefaultOnError<A> {
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
        A::serialize_as(value, state)
    }

    fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
        A::finish_as(value, state)
    }

    fn is_optional_as(value: &T) -> bool {
        A::is_optional_as(value)
    }

    fn descriptor_as(value: &T) -> &'static dyn Descriptor {
        A::descriptor_as(value)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        A::__private_begin_as(value, state)
    }
}

/// Deserializes into a slot and invokes a callback with the value.
///
/// Atoms are deserialized directly, compound values are recorded first.
/// Errors are not reported, in that case the callback is not invoked.
fn try_deserialize<'a, 'de, T: 'a, A: DeserializeAs<'de, T>>(
    then: impl FnOnce(T) + 'a,
) -> SinkHandle<'a, 'de> {
    Recording::capture(move |recording, state| {
        let mut value = None;
        if recording
            .replay(A::deserialize_into_as(&mut value), state)
            .is_ok()
        {
            if let Some(value) = value {
                then(value);
            }
        }
        Ok(())
    })
}

/// Deserializes an atom and returns the value unless it failed.
fn try_atom<'de, T, A: DeserializeAs<'de, T>>(atom: Atom, state: &mut State) -> Option<T> {
    let mut value = None;
    match A::__private_atom_into_as(&mut value, atom, state) {
        Ok(()) => value,
        Err(_) => None,
    }
}

/// Deserializes a borrowed atom and returns the value unless it failed.
fn try_borrowed_atom<'de, T, A: DeserializeAs<'de, T>>(
    atom: Atom<'de>,
    state: &mut State,
) -> Option<T> {
    let mut value = None;
    match A::__private_borrowed_atom_into_as(&mut value, atom, state) {
        Ok(()) => value,
        Err(_) => None,
    }
}

/// Skips elements of a vector which cannot be deserialized.
///
/// The elements are deserialized with the adapter `A` (by default
/// [`Same`]).  Compound elements are buffered in a [`Recording`] as the
/// error can only be detected once they were seen fully.  Serialization
/// uses the inner adapter for the elements.
///
/// ```
/// use deser::Deserialize;
/// use deser::adapters::VecSkipError;
///
/// #[derive(Deserialize)]
/// pub enum Kind {
///     A,
///     B,
/// }
///
/// #[derive(Deserialize)]
/// pub struct Item {
///     // unknown kinds are skipped
///     #[deser(as = VecSkipError)]
///     kinds: Vec<Kind>,
/// }
/// ```
pub struct VecSkipError<A = Same>(PhantomData<fn() -> A>);

impl<'de, T, A: DeserializeAs<'de, T>> DeserializeAs<'de, Vec<T>> for VecSkipError<A> {
    fn deserialize_into_as(out: &mut Option<Vec<T>>) -> SinkHandle<'_, 'de> {
        struct SkipSink<'a, T, A> {
            slot: &'a mut Option<Vec<T>>,
            vec: Vec<T>,
            _marker: PhantomData<fn() -> A>,
        }

        impl<'a, 'de, T, A: DeserializeAs<'de, T>> Sink<'de> for SkipSink<'a, T, A> {
            fn descriptor(&self) -> &'static dyn Descriptor {
                static DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "vec" };
                &DESCRIPTOR
            }

            fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
                let vec = &mut self.vec;
                Ok(try_deserialize::<T, A>(move |value| vec.push(value)))
            }

            fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                if let Some(value) = try_atom::<T, A>(atom, state) {
                    self.vec.push(value);
                }
                Ok(())
            }

            fn borrowed_value_atom(
                &mut self,
                atom: Atom<'de>,
                state: &mut State,
            ) -> Result<(), Error> {
                if let Some(value) = try_borrowed_atom::<T, A>(atom, state) {
                    self.vec.push(value);
                }
                Ok(())
            }

            fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
                *self.slot = Some(take(&mut self.vec));
                Ok(())
            }
        }

        SinkHandle::boxed(SkipSink::<T, A> {
            slot: out,
            vec: Vec::new(),
            _marker: PhantomData,
        })
    }
}

impl<T, A: SerializeAs<T>> SerializeAs<Vec<T>> for VecSkipError<A> {
    fn serialize_as<'a>(value: &'a Vec<T>, state: &mut State) -> Result<Chunk<'a>, Error> {
        <Vec<A> as SerializeAs<Vec<T>>>::serialize_as(value, state)
    }

    fn descriptor_as(value: &Vec<T>) -> &'static dyn Descriptor {
        <Vec<A> as SerializeAs<Vec<T>>>::descriptor_as(value)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a Vec<T>, state: &mut State) -> Result<Begin<'a>, Error> {
        <Vec<A> as SerializeAs<Vec<T>>>::__private_begin_as(value, state)
    }
}

/// Skips entries of a map which cannot be deserialized.
///
/// Keys are deserialized with the adapter `KA` and values with `VA` (both
/// [`Same`] by default).  If either of them fails, the entry is skipped.
/// Compound keys and values are buffered in a [`Recording`] as the error
/// can only be detected once they were seen fully.  Supported are
/// [`BTreeMap`] and [`HashMap`].  Serialization uses the inner adapters.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::Deserialize;
/// use deser::adapters::MapSkipError;
///
/// #[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
/// pub enum Kind {
///     A,
///     B,
/// }
///
/// #[derive(Deserialize)]
/// pub struct Weights {
///     // entries with unknown kinds are skipped
///     #[deser(as = MapSkipError)]
///     weights: BTreeMap<Kind, f64>,
/// }
/// ```
pub struct MapSkipError<KA = Same, VA = Same>(PhantomData<fn() -> (KA, VA)>);

fn skip_map_sink<'a, 'de, M, K, V, KA, VA>(out: &'a mut Option<M>) -> SinkHandle<'a, 'de>
where
    M: MapTarget<K, V> + 'a,
    K: 'a,
    V: 'a,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    #[allow(clippy::type_complexity)]
    struct SkipMapSink<'a, M, K, V, KA, VA> {
        slot: &'a mut Option<M>,
        map: M,
        // the key of the current entry, `None` if it failed
        key: Option<K>,
        _marker: PhantomData<fn() -> (V, KA, VA)>,
    }

    impl<'a, 'de, M, K, V, KA, VA> Sink<'de> for SkipMapSink<'a, M, K, V, KA, VA>
    where
        M: MapTarget<K, V>,
        KA: DeserializeAs<'de, K>,
        VA: DeserializeAs<'de, V>,
    {
        fn map(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.key = None;
            let key = &mut self.key;
            Ok(try_deserialize::<K, KA>(move |value| *key = Some(value)))
        }

        fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.key = try_atom::<K, KA>(atom, state);
            Ok(())
        }

        fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
            self.key = try_borrowed_atom::<K, KA>(atom, state);
            Ok(())
        }

        fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            let key = match self.key.take() {
                Some(key) => key,
                None => return Ok(SinkHandle::null()),
            };
            let map = &mut self.map;
            Ok(try_deserialize::<V, VA>(move |value| {
                map.insert_entry(key, value)
            }))
        }

        fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            if let Some(key) = self.key.take() {
                if let Some(value) = try_atom::<V, VA>(atom, state) {
                    self.map.insert_entry(key, value);
                }
            }
            Ok(())
        }

        fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
            if let Some(key) = self.key.take() {
                if let Some(value) = try_borrowed_atom::<V, VA>(atom, state) {
                    self.map.insert_entry(key, value);
                }
            }
            Ok(())
        }

        fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
            *self.slot = Some(take(&mut self.map));
            Ok(())
        }
    }

    SinkHandle::boxed(SkipMapSink::<M, K, V, KA, VA> {
        slot: out,
        map: M::default(),
        key: None,
        _marker: PhantomData,
    })
}

impl<'de, K, V, KA, VA> DeserializeAs<'de, BTreeMap<K, V>> for MapSkipError<KA, VA>
where
    K: Ord,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<BTreeMap<K, V>>) -> SinkHandle<'_, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out)
    }
}

impl<'de, K, V, H, KA, VA> DeserializeAs<'de, HashMap<K, V, H>> for MapSkipError<KA, VA>
where
    K: Hash + Eq,
    H: BuildHasher + Default,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<HashMap<K, V, H>>) -> SinkHandle<'_, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out)
    }
}

impl<K, V, KA, VA> SerializeAs<BTreeMap<K, V>> for MapSkipError<KA, VA>
where
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(value: &'a BTreeMap<K, V>, state: &mut State) -> Result<Chunk<'a>, Error> {
        <BTreeMap<KA, VA> as SerializeAs<BTreeMap<K, V>>>::serialize_as(value, state)
    }

    fn descriptor_as(value: &BTreeMap<K, V>) -> &'static dyn Descriptor {
        <BTreeMap<KA, VA> as SerializeAs<BTreeMap<K, V>>>::descriptor_as(value)
    }
}

impl<K, V, H, KA, VA> SerializeAs<HashMap<K, V, H>> for MapSkipError<KA, VA>
where
    H: BuildHasher,
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(
        value: &'a HashMap<K, V, H>,
        state: &mut State,
    ) -> Result<Chunk<'a>, Error> {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, H>>>::serialize_as(value, state)
    }

    fn descriptor_as(value: &HashMap<K, V, H>) -> &'static dyn Descriptor {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, H>>>::descriptor_as(value)
    }
}
