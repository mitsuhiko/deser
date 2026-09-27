//! The adapters provided by deser.
use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt::Display;
#[cfg(feature = "std")]
use core::hash::{BuildHasher, Hash};
use core::marker::PhantomData;
use core::mem::take;
use core::str::FromStr;
#[cfg(feature = "std")]
use std::collections::HashMap;

use crate::State;
use crate::Text;
use crate::adapters::{DeserializeAs, Same, SerializeAs};
use crate::de::impls::MapTarget;
use crate::de::lexical::parse_bool_with;
use crate::de::mapped::MappedSink;
use crate::de::{Deserialize, DuplicateKeys, OwnedSink, Sink, SinkHandle};
use crate::error::{Error, ErrorKind, conversion_error};
use crate::event::{Atom, Bytes, ContainerShape};
use crate::ser::{Begin, Chunk, Describe, Serialize, SerializeHandle};

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
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("string")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) | Atom::Lexical(value) => {
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
            Atom::Str(value) | Atom::Lexical(value) => {
                **self = Some(value.into_cow());
                Ok(())
            }
            // strings take the text of values whose type was inferred
            Atom::Implicit(value) => {
                **self = Some(value.into_parts().0.into_cow());
                Ok(())
            }
            other => self.atom(other, state),
        }
    }
}

impl<'de: 'a, 'a> Sink<'de> for BorrowedSlot<Cow<'a, [u8]>> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("bytes")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(value) => {
                **self = Some(Cow::Owned(value.into_owned()));
                Ok(())
            }
            // formats without native bytes represent them as strings
            Atom::Str(ref value) => {
                **self = Some(Cow::Owned(crate::adapters::bytes::decode_str(
                    value, state,
                )?));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bytes(value) => {
                **self = Some(value.into_data());
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
        Ok(Chunk::Atom(Atom::Str(Text::borrowed(value))))
    }
}

impl<'a> SerializeAs<Cow<'a, [u8]>> for Borrowed {
    fn serialize_as<'b>(value: &'b Cow<'a, [u8]>, _state: &mut State) -> Result<Chunk<'b>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(Bytes::borrowed(value))))
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
    T: FromStr + Send,
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
    T: FromStr + Send,
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
        Ok(Chunk::Atom(Atom::Str(Text::owned(value.to_string()))))
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            ContainerShape::new(),
            false,
        ))
    }
}

/// A flag that is set if its key is given.
///
/// This is for `bool` fields that are switched on by giving their key,
/// like `?recursive` in a query string:
///
/// * if the key is missing, the flag is `false` (no `#[deser(default)]` is
///   needed)
/// * an empty value (`?recursive` or `?recursive=`) or null is `true`
/// * other values are booleans, strings are parsed like
///   [lexical atoms](crate::Atom::Lexical) (`?recursive=0` is `false`)
///
/// The flag is serialized as boolean.
///
/// ```
/// use deser::adapters::Flag;
///
/// #[derive(deser::Deserialize, deser::Serialize)]
/// pub struct Tree {
///     #[deser(as = Flag, skip_serializing_if = std::ops::Not::not)]
///     recursive: bool,
/// }
/// ```
pub struct Flag;

make_slot_wrapper!(FlagSlot);

impl<'de> Sink<'de> for FlagSlot<bool> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("flag")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let value = match atom {
            Atom::Bool(value) => value,
            Atom::Null => true,
            Atom::Str(ref value) | Atom::Lexical(ref value) if value.is_empty() => true,
            Atom::Str(ref value) | Atom::Lexical(ref value) => parse_bool_with(value, true, state)?,
            other => return self.unexpected_atom(other, state),
        };
        **self = Some(value);
        Ok(())
    }
}

impl<'de> DeserializeAs<'de, bool> for Flag {
    fn deserialize_into_as(out: &mut Option<bool>) -> SinkHandle<'_, 'de> {
        FlagSlot::make_handle(out)
    }

    fn initial_value_as() -> Option<bool> {
        Some(false)
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<bool>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        let sink = FlagSlot::wrap(out);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<bool>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        Self::__private_atom_into_as(out, atom, state)
    }
}

impl SerializeAs<bool> for Flag {
    fn serialize_as<'a>(value: &'a bool, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Bool(*value)))
    }
}

/// Converts from and into another type.
///
/// The value is deserialized as `U` and converted with [`Into`] and it's
/// serialized by cloning it and converting it into `U`.  The value is
/// [optional](Serialize::is_optional) if the converted value is, which means
/// that with `#[deser(skip_serializing_optionals)]` the value is cloned and
/// converted once more to find out.
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
    T: Send,
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
    U: Serialize + Send + 'static,
{
    fn serialize_as<'a>(value: &'a T, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Forward(SerializeHandle::boxed(Into::<U>::into(
            value.clone(),
        ))))
    }

    // this clones once more, but only `skip_serializing_optionals` asks.
    // Without converting there is no way to know as `()` is optional too.
    fn is_optional_as(value: &T) -> bool {
        Into::<U>::into(value.clone()).is_optional()
    }
}

/// Converts from and into another type with fallible conversions.
///
/// This is like [`FromInto`] but uses [`TryFrom`] and [`TryInto`].  Failed
/// conversions are reported as errors.  Like with [`FromInto`] the value is
/// cloned and converted once more with `#[deser(skip_serializing_optionals)]`
/// to find out if it's optional.
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

impl<'de, T, U> DeserializeAs<'de, T> for TryFromInto<U>
where
    U: Deserialize<'de> + 'static,
    T: TryFrom<U> + Send,
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
    U: Serialize + Send + 'static,
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
/// that fails, the default value is used instead and the rest of the value
/// is skipped (see [`Sink::recover`]).  Only errors of the value are
/// handled, errors of the data format and of layers (such as
/// [`Limits`](crate::de::Limits)) still fail the deserialization.  Missing
/// values are handled by the inner adapter.  Serialization uses the inner
/// adapter.
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

impl<'de, T: Default + Send, A: DeserializeAs<'de, T>> DeserializeAs<'de, T> for DefaultOnError<A> {
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(DefaultOnErrorSink {
            out,
            sink: Some(OwnedSink::deserialize_as::<A>()),
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
        let rv = state.discard_errors(|state| A::__private_atom_into_as(&mut value, atom, state));
        *out = Some(match (rv, value) {
            (Ok(()), Some(value)) => value,
            _ => T::default(),
        });
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let mut value = None;
        let rv = state
            .discard_errors(|state| A::__private_borrowed_atom_into_as(&mut value, atom, state));
        *out = Some(match (rv, value) {
            (Ok(()), Some(value)) => value,
            _ => T::default(),
        });
        Ok(())
    }
}

/// The sink of [`DefaultOnError`].
struct DefaultOnErrorSink<'a, 'de, T> {
    out: &'a mut Option<T>,
    // `None` once the value failed, the rest of it is ignored then
    sink: Option<OwnedSink<'de, T>>,
}

impl<'a, 'de, T> DefaultOnErrorSink<'a, 'de, T> {
    /// Returns the sink of the value unless it failed.
    fn sink(&mut self) -> Option<&mut (dyn Sink<'de> + '_)> {
        self.sink.as_mut().map(|sink| sink.borrow_mut())
    }

    /// Discards the value if the result is an error.
    fn check(&mut self, rv: Result<(), Error>) -> Result<(), Error> {
        if rv.is_err() {
            self.sink = None;
        }
        Ok(())
    }
}

impl<'a, 'de, T: Default + Send> Sink<'de> for DefaultOnErrorSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let rv = match self.sink() {
            Some(sink) => sink.atom(atom, state),
            None => Ok(()),
        };
        self.check(rv)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let rv = match self.sink() {
            Some(sink) => sink.borrowed_atom(atom, state),
            None => Ok(()),
        };
        self.check(rv)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = match self.sink() {
            Some(sink) => sink.map(state),
            None => Ok(()),
        };
        self.check(rv)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = match self.sink() {
            Some(sink) => sink.seq(state),
            None => Ok(()),
        };
        match rv {
            // the values of a repeated key are delivered as a single value
            // to sinks that reject sequences, the driver needs the error to
            // do that (see `ContainerShape::with_repeated`)
            Err(err)
                if err.kind() == ErrorKind::Unexpected && state.container_shape().is_repeated() =>
            {
                Err(err)
            }
            rv => self.check(rv),
        }
    }

    // Errors of the items are handled in `recover`.

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match self.sink() {
            Some(sink) => sink.next_key(state),
            None => Ok(SinkHandle::null()),
        }
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match self.sink() {
            Some(sink) => sink.next_value(state),
            None => Ok(SinkHandle::null()),
        }
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_key_atom(atom, state),
            None => Ok(()),
        }
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_value_atom(atom, state),
            None => Ok(()),
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_borrowed_key_atom(atom, state),
            None => Ok(()),
        }
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match self.sink() {
            Some(sink) => sink.__private_borrowed_value_atom(atom, state),
            None => Ok(()),
        }
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        match self.sink() {
            Some(sink) => sink.value_for_key(key, state),
            None => Ok(None),
        }
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        // the value might recover itself, otherwise it failed
        if let Some(sink) = self.sink()
            && sink.recover(err, state).is_ok()
        {
            return Ok(());
        }
        self.sink = None;
        Ok(())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = match self.sink() {
            Some(sink) => sink.finish(state),
            None => Ok(()),
        };
        self.check(rv)?;
        let value = self.sink.as_mut().and_then(|sink| sink.take());
        *self.out = Some(value.unwrap_or_default());
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        match self.sink {
            Some(ref sink) => sink.borrow().expecting(),
            None => Cow::Borrowed("compatible type"),
        }
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

    fn container_shape_as(value: &T) -> ContainerShape {
        A::container_shape_as(value)
    }

    fn describe_as(value: &T, d: &mut dyn Describe) {
        A::describe_as(value, d)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a T, state: &mut State) -> Result<Begin<'a>, Error> {
        A::__private_begin_as(value, state)
    }
}

/// Deserializes an atom and returns the value unless it failed.
fn try_atom<'de, T, A: DeserializeAs<'de, T>>(atom: Atom, state: &mut State) -> Option<T> {
    let mut value = None;
    match state.discard_errors(|state| A::__private_atom_into_as(&mut value, atom, state)) {
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
    match state.discard_errors(|state| A::__private_borrowed_atom_into_as(&mut value, atom, state))
    {
        Ok(()) => value,
        Err(_) => None,
    }
}

/// Skips elements of a vector which cannot be deserialized.
///
/// The elements are deserialized with the adapter `A` (by default
/// [`Same`]).  If an element fails, the rest of it is skipped (see
/// [`Sink::recover`]).  Only errors of the elements are handled, errors of
/// the data format and of layers still fail the deserialization.
/// Serialization uses the inner adapter for the elements.
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

impl<'de, T: Send, A: DeserializeAs<'de, T>> DeserializeAs<'de, Vec<T>> for VecSkipError<A> {
    fn deserialize_into_as(out: &mut Option<Vec<T>>) -> SinkHandle<'_, 'de> {
        struct SkipSink<'a, T, A> {
            slot: &'a mut Option<Vec<T>>,
            vec: Vec<T>,
            // the current element, added once the next one starts
            item: Option<T>,
            _marker: PhantomData<fn() -> A>,
        }

        impl<'a, T, A> SkipSink<'a, T, A> {
            fn flush(&mut self) {
                if let Some(item) = self.item.take() {
                    self.vec.push(item);
                }
            }
        }

        impl<'a, 'de, T: Send, A: DeserializeAs<'de, T>> Sink<'de> for SkipSink<'a, T, A> {
            fn expecting(&self) -> Cow<'_, str> {
                Cow::Borrowed("vec")
            }

            fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
                self.flush();
                Ok(A::deserialize_into_as(&mut self.item))
            }

            fn recover(&mut self, _err: Error, _state: &mut State) -> Result<(), Error> {
                self.item = None;
                Ok(())
            }

            fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
                self.flush();
                if let Some(value) = try_atom::<T, A>(atom, state) {
                    self.vec.push(value);
                }
                Ok(())
            }

            fn __private_borrowed_value_atom(
                &mut self,
                atom: Atom<'de>,
                state: &mut State,
            ) -> Result<(), Error> {
                self.flush();
                if let Some(value) = try_borrowed_atom::<T, A>(atom, state) {
                    self.vec.push(value);
                }
                Ok(())
            }

            fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
                self.flush();
                *self.slot = Some(take(&mut self.vec));
                Ok(())
            }
        }

        SinkHandle::boxed(SkipSink::<T, A> {
            slot: out,
            vec: Vec::new(),
            item: None,
            _marker: PhantomData,
        })
    }
}

impl<T: Sync, A: SerializeAs<T>> SerializeAs<Vec<T>> for VecSkipError<A> {
    fn serialize_as<'a>(value: &'a Vec<T>, state: &mut State) -> Result<Chunk<'a>, Error> {
        <Vec<A> as SerializeAs<Vec<T>>>::serialize_as(value, state)
    }

    fn container_shape_as(value: &Vec<T>) -> ContainerShape {
        <Vec<A> as SerializeAs<Vec<T>>>::container_shape_as(value)
    }

    fn describe_as(value: &Vec<T>, d: &mut dyn Describe) {
        <Vec<A> as SerializeAs<Vec<T>>>::describe_as(value, d)
    }

    #[inline]
    fn __private_begin_as<'a>(value: &'a Vec<T>, state: &mut State) -> Result<Begin<'a>, Error> {
        <Vec<A> as SerializeAs<Vec<T>>>::__private_begin_as(value, state)
    }
}

/// Skips entries of a map which cannot be deserialized.
///
/// Keys are deserialized with the adapter `KA` and values with `VA` (both
/// [`Same`] by default).  If either of them fails, the rest of the entry is
/// skipped (see [`Sink::recover`]).  Only errors of the entries are
/// handled, errors of the data format and of layers still fail the
/// deserialization.  Supported are [`BTreeMap`] and [`HashMap`].
/// Serialization uses the inner adapters.
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

pub(crate) fn skip_map_sink<'a, 'de, M, K, V, KA, VA>(out: &'a mut Option<M>) -> SinkHandle<'a, 'de>
where
    M: MapTarget<K, V> + 'a,
    K: Send + 'a,
    V: Send + 'a,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    #[allow(clippy::type_complexity)]
    struct SkipMapSink<'a, M, K, V, KA, VA> {
        slot: &'a mut Option<M>,
        map: M,
        // the key of the current entry, `None` if it failed
        key: Option<K>,
        // the value of the current entry if it's not an atom, the entry is
        // added once the next one starts
        value: Option<V>,
        // if the values of duplicate keys replace earlier ones.  With
        // `DuplicateKeys::Error` duplicate entries fail and are skipped.
        replace: bool,
        _marker: PhantomData<fn() -> (V, KA, VA)>,
    }

    impl<'a, M: MapTarget<K, V>, K, V, KA, VA> SkipMapSink<'a, M, K, V, KA, VA> {
        fn flush(&mut self) {
            if let Some(value) = self.value.take()
                && let Some(key) = self.key.take()
            {
                self.map.insert_entry(key, value, self.replace);
            }
            self.key = None;
        }
    }

    impl<'a, 'de, M, K, V, KA, VA> Sink<'de> for SkipMapSink<'a, M, K, V, KA, VA>
    where
        M: MapTarget<K, V>,
        K: Send,
        V: Send,
        KA: DeserializeAs<'de, K>,
        VA: DeserializeAs<'de, V>,
    {
        fn map(&mut self, state: &mut State) -> Result<(), Error> {
            self.replace = state.duplicate_keys() == DuplicateKeys::Last;
            Ok(())
        }

        fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.flush();
            Ok(KA::deserialize_into_as(&mut self.key))
        }

        fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.flush();
            self.key = try_atom::<K, KA>(atom, state);
            Ok(())
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.flush();
            self.key = try_borrowed_atom::<K, KA>(atom, state);
            Ok(())
        }

        fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            if self.key.is_none() {
                return Ok(SinkHandle::null());
            }
            Ok(VA::deserialize_into_as(&mut self.value))
        }

        fn recover(&mut self, _err: Error, _state: &mut State) -> Result<(), Error> {
            // the key or the value of the entry failed
            self.key = None;
            self.value = None;
            Ok(())
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            if let Some(key) = self.key.take()
                && let Some(value) = try_atom::<V, VA>(atom, state)
            {
                self.map.insert_entry(key, value, self.replace);
            }
            Ok(())
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            if let Some(key) = self.key.take()
                && let Some(value) = try_borrowed_atom::<V, VA>(atom, state)
            {
                self.map.insert_entry(key, value, self.replace);
            }
            Ok(())
        }

        fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
            self.flush();
            *self.slot = Some(take(&mut self.map));
            Ok(())
        }
    }

    SinkHandle::boxed(SkipMapSink::<M, K, V, KA, VA> {
        slot: out,
        map: M::default(),
        key: None,
        value: None,
        replace: true,
        _marker: PhantomData,
    })
}

impl<'de, K, V, KA, VA> DeserializeAs<'de, BTreeMap<K, V>> for MapSkipError<KA, VA>
where
    K: Ord + Send,
    V: Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<BTreeMap<K, V>>) -> SinkHandle<'_, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out)
    }
}

#[cfg(feature = "std")]
impl<'de, K, V, H, KA, VA> DeserializeAs<'de, HashMap<K, V, H>> for MapSkipError<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    H: BuildHasher + Default + Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<HashMap<K, V, H>>) -> SinkHandle<'_, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out)
    }
}

impl<K, V, KA, VA> SerializeAs<BTreeMap<K, V>> for MapSkipError<KA, VA>
where
    K: Sync,
    V: Sync,
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(value: &'a BTreeMap<K, V>, state: &mut State) -> Result<Chunk<'a>, Error> {
        <BTreeMap<KA, VA> as SerializeAs<BTreeMap<K, V>>>::serialize_as(value, state)
    }

    fn container_shape_as(value: &BTreeMap<K, V>) -> ContainerShape {
        <BTreeMap<KA, VA> as SerializeAs<BTreeMap<K, V>>>::container_shape_as(value)
    }

    fn describe_as(value: &BTreeMap<K, V>, d: &mut dyn Describe) {
        <BTreeMap<KA, VA> as SerializeAs<BTreeMap<K, V>>>::describe_as(value, d)
    }
}

#[cfg(feature = "std")]
impl<K, V, H, KA, VA> SerializeAs<HashMap<K, V, H>> for MapSkipError<KA, VA>
where
    K: Sync,
    V: Sync,
    H: BuildHasher + Sync,
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(
        value: &'a HashMap<K, V, H>,
        state: &mut State,
    ) -> Result<Chunk<'a>, Error> {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, H>>>::serialize_as(value, state)
    }

    fn container_shape_as(value: &HashMap<K, V, H>) -> ContainerShape {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, H>>>::container_shape_as(value)
    }

    fn describe_as(value: &HashMap<K, V, H>, d: &mut dyn Describe) {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, H>>>::describe_as(value, d)
    }
}
