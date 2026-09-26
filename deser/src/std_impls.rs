//! `Serialize` and `Deserialize` for leaf types of the standard library.
//!
//! The containers and pointers are implemented next to the primitives in
//! `ser::impls` and `de::impls`.
use std::borrow::Cow;
use std::cmp::Reverse;
use std::ffi::{CStr, CString};
use std::fmt::Display;
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::num::{NonZero, Saturating, Wrapping};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::State;
use crate::adapters::{DeserializeAs, Same, SerializeAs, SerializeAsRef};
use crate::de::impls::{Via, deserialize_via};
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Bytes};
use crate::ext::ExtValue;
use crate::ser::{
    Begin, Chunk, Describe, Serialize, SerializeHandle, StructEmitter, Variant, VariantKind,
    VariantRepr,
};

make_slot_wrapper!(SlotWrapper);

// PhantomData

/// Serializes as null like `()`.
impl<T: ?Sized> Serialize for PhantomData<T> {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Null))
    }

    fn is_optional(&self) -> bool {
        true
    }
}

impl<'de, T: ?Sized> Sink<'de> for SlotWrapper<PhantomData<T>> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("null")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Null => {
                **self = Some(PhantomData);
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

/// Deserializes from null.  Missing values are accepted as the value is
/// skipped by `#[deser(skip_serializing_optionals)]`.
impl<'de, T: ?Sized> Deserialize<'de> for PhantomData<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SlotWrapper::make_handle(out)
    }

    fn initial_value() -> Option<Self> {
        Some(PhantomData)
    }
}

// Wrapping, Saturating and Reverse

macro_rules! newtype_wrapper {
    ($($ty:ident),*) => {
        $(
            impl<T: Serialize> Serialize for $ty<T> {
                fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
                    self.0.serialize(state)
                }

                fn finish(&self, state: &mut State) -> Result<(), Error> {
                    self.0.finish(state)
                }

                #[inline]
                fn __private_begin(&self, state: &mut State) -> Result<Begin<'_>, Error> {
                    self.0.__private_begin(state)
                }

                fn is_optional(&self) -> bool {
                    self.0.is_optional()
                }

                fn container_shape(&self) -> crate::ContainerShape {
                    self.0.container_shape()
                }

                fn describe(&self, d: &mut dyn Describe) {
                    d.newtype(stringify!($ty));
                    self.0.describe(d);
                }
            }

            impl<T> Via<T> for $ty<T> {
                #[inline]
                fn convert(value: T) -> Result<Self, Error> {
                    Ok($ty(value))
                }
            }

            deserialize_via! {
                [T: Deserialize<'de>] $ty<T> => T;
            }
        )*
    };
}

newtype_wrapper!(Wrapping, Saturating, Reverse);

// NonZero

macro_rules! non_zero {
    ($($ty:ty => $atom:ident),* $(,)?) => {
        $(
            impl Serialize for NonZero<$ty> {
                __begin_without_finish!();

                fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                    Ok(Chunk::Atom(non_zero!(@atom $atom, self.get())))
                }
            }

            impl Via<$ty> for NonZero<$ty> {
                #[inline]
                fn convert(value: $ty) -> Result<Self, Error> {
                    NonZero::new(value).ok_or_else(|| {
                        Error::new(ErrorKind::OutOfRange, "value must be non-zero")
                    })
                }
            }

            deserialize_via! {
                [] NonZero<$ty> => $ty;
            }
        )*
    };
    (@atom Ext, $value:expr) => { Atom::Ext(ExtValue::owned($value)) };
    (@atom $atom:ident, $value:expr) => { Atom::$atom($value as _) };
}

non_zero! {
    u8 => U64,
    u16 => U64,
    u32 => U64,
    u64 => U64,
    usize => U64,
    u128 => Ext,
    i8 => I64,
    i16 => I64,
    i32 => I64,
    i64 => I64,
    isize => I64,
    i128 => Ext,
}

// Atomics

macro_rules! atomic {
    ($($cfg:literal: $ty:ident($prim:ty) => $atom:ident),* $(,)?) => {
        $(
            /// The value is loaded with relaxed ordering.
            #[cfg(target_has_atomic = $cfg)]
            impl Serialize for std::sync::atomic::$ty {
                __begin_without_finish!();

                fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                    let value = self.load(std::sync::atomic::Ordering::Relaxed);
                    Ok(Chunk::Atom(Atom::$atom(value as _)))
                }
            }

            #[cfg(target_has_atomic = $cfg)]
            impl Via<$prim> for std::sync::atomic::$ty {
                #[inline]
                fn convert(value: $prim) -> Result<Self, Error> {
                    Ok(std::sync::atomic::$ty::new(value))
                }
            }

            #[cfg(target_has_atomic = $cfg)]
            deserialize_via! {
                [] std::sync::atomic::$ty => $prim;
            }
        )*
    };
}

atomic! {
    "8": AtomicBool(bool) => Bool,
    "8": AtomicU8(u8) => U64,
    "8": AtomicI8(i8) => I64,
    "16": AtomicU16(u16) => U64,
    "16": AtomicI16(i16) => I64,
    "32": AtomicU32(u32) => U64,
    "32": AtomicI32(i32) => I64,
    "64": AtomicU64(u64) => U64,
    "64": AtomicI64(i64) => I64,
    "ptr": AtomicUsize(usize) => U64,
    "ptr": AtomicIsize(isize) => I64,
}

// Result

/// Emits the single entry of an externally tagged `Ok` or `Err`.
struct ResultEmitter<'a> {
    name: &'static str,
    value: Option<SerializeHandle<'a>>,
}

impl<'a> StructEmitter for ResultEmitter<'a> {
    fn next(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        Ok(self
            .value
            .take()
            .map(|value| (Cow::Borrowed(self.name), value)))
    }
}

/// Describes a variant of a `Result`.
fn describe_result<T, E>(value: &Result<T, E>, d: &mut dyn Describe) {
    d.variant(&Variant::new(
        "Result",
        result_name(value),
        VariantKind::Newtype,
        VariantRepr::External,
    ));
}

fn result_name<T, E>(value: &Result<T, E>) -> &'static str {
    match value {
        Ok(_) => "Ok",
        Err(_) => "Err",
    }
}

/// Serializes externally tagged: `{"Ok": value}` or `{"Err": error}`.
impl<T: Serialize, E: Serialize> Serialize for Result<T, E> {
    __begin_without_finish!();

    fn describe(&self, d: &mut dyn Describe) {
        describe_result(self, d);
    }

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        let value = match self {
            Ok(value) => SerializeHandle::to(value),
            Err(err) => SerializeHandle::to(err),
        };
        Ok(Chunk::Struct(Box::new(ResultEmitter {
            name: result_name(self),
            value: Some(value),
        })))
    }
}

impl<T, E, TA, EA> SerializeAs<Result<T, E>> for Result<TA, EA>
where
    TA: SerializeAs<T>,
    EA: SerializeAs<E>,
{
    fn describe_as(value: &Result<T, E>, d: &mut dyn Describe) {
        describe_result(value, d);
    }

    fn serialize_as<'a>(value: &'a Result<T, E>, _state: &mut State) -> Result<Chunk<'a>, Error> {
        let handle = match value {
            Ok(value) => SerializeHandle::to(SerializeAsRef::<TA, T>::new(value)),
            Err(err) => SerializeHandle::to(SerializeAsRef::<EA, E>::new(err)),
        };
        Ok(Chunk::Struct(Box::new(ResultEmitter {
            name: result_name(value),
            value: Some(handle),
        })))
    }

    #[inline]
    fn __private_begin_as<'a>(
        value: &'a Result<T, E>,
        state: &mut State,
    ) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize_as(value, state)?,
            crate::ContainerShape::new(),
            false,
        ))
    }
}

/// The variant of a `Result`.
#[derive(Clone, Copy)]
enum ResultVariant {
    Ok,
    Err,
}

make_slot_wrapper!(ResultVariantSlot);

impl<'de> Sink<'de> for ResultVariantSlot<ResultVariant> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("Ok or Err")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref name) => {
                **self = Some(match &**name {
                    "Ok" => ResultVariant::Ok,
                    "Err" => ResultVariant::Err,
                    other => {
                        return Err(Error::new(
                            ErrorKind::Unexpected,
                            format!("unknown variant {other:?}, expected Ok or Err"),
                        ));
                    }
                });
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

/// Creates the sink for a `Result` with adapters.
fn result_sink<'a, 'de, T, E, TA, EA>(out: &'a mut Option<Result<T, E>>) -> SinkHandle<'a, 'de>
where
    T: 'a,
    E: 'a,
    TA: DeserializeAs<'de, T>,
    EA: DeserializeAs<'de, E>,
{
    struct ResultSink<'a, T, E, TA, EA> {
        slot: &'a mut Option<Result<T, E>>,
        variant: Option<ResultVariant>,
        ok: Option<T>,
        err: Option<E>,
        _marker: PhantomData<fn() -> (TA, EA)>,
    }

    impl<'de, 'a, T, E, TA, EA> Sink<'de> for ResultSink<'a, T, E, TA, EA>
    where
        TA: DeserializeAs<'de, T>,
        EA: DeserializeAs<'de, E>,
    {
        fn expecting(&self) -> Cow<'_, str> {
            Cow::Borrowed("Result")
        }

        fn map(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            if self.variant.is_some() {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "expected a single entry for Result",
                ));
            }
            Ok(ResultVariantSlot::make_handle(&mut self.variant))
        }

        fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            Ok(match self.variant {
                Some(ResultVariant::Ok) => TA::deserialize_into_as(&mut self.ok),
                Some(ResultVariant::Err) => EA::deserialize_into_as(&mut self.err),
                None => SinkHandle::null(),
            })
        }

        fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            match self.variant {
                Some(ResultVariant::Ok) => TA::__private_atom_into_as(&mut self.ok, atom, state),
                Some(ResultVariant::Err) => EA::__private_atom_into_as(&mut self.err, atom, state),
                None => Ok(()),
            }
        }

        fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
            match self.variant {
                Some(ResultVariant::Ok) => {
                    TA::__private_borrowed_atom_into_as(&mut self.ok, atom, state)
                }
                Some(ResultVariant::Err) => {
                    EA::__private_borrowed_atom_into_as(&mut self.err, atom, state)
                }
                None => Ok(()),
            }
        }

        fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
            *self.slot = Some(match (self.ok.take(), self.err.take()) {
                (Some(value), _) => Ok(value),
                (None, Some(err)) => Err(err),
                (None, None) => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "expected an entry with Ok or Err",
                    ));
                }
            });
            Ok(())
        }
    }

    SinkHandle::boxed(ResultSink::<T, E, TA, EA> {
        slot: out,
        variant: None,
        ok: None,
        err: None,
        _marker: PhantomData,
    })
}

impl<'de, T: Deserialize<'de>, E: Deserialize<'de>> Deserialize<'de> for Result<T, E> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        result_sink::<T, E, Same, Same>(out)
    }
}

impl<'de, T, E, TA, EA> DeserializeAs<'de, Result<T, E>> for Result<TA, EA>
where
    TA: DeserializeAs<'de, T>,
    EA: DeserializeAs<'de, E>,
{
    fn deserialize_into_as(out: &mut Option<Result<T, E>>) -> SinkHandle<'_, 'de> {
        result_sink::<T, E, TA, EA>(out)
    }
}

// Types that are represented as strings

/// Types that are deserialized by parsing a string.
trait Parse: FromStr<Err: Display> {
    /// What the type expects, for error messages.
    const EXPECTING: &'static str;
}

make_slot_wrapper!(ParseSlot);

impl<'de, T: Parse> Sink<'de> for ParseSlot<T> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(T::EXPECTING)
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref value) => match value.parse::<T>() {
                Ok(value) => {
                    **self = Some(value);
                    Ok(())
                }
                Err(err) => Err(Error::new(
                    ErrorKind::Unexpected,
                    format!("invalid {}: {}", T::EXPECTING, err),
                )),
            },
            other => self.unexpected_atom(other, state),
        }
    }
}

macro_rules! parse_from_str {
    ($($ty:ty => $expecting:literal),* $(,)?) => {
        $(
            impl Parse for $ty {
                const EXPECTING: &'static str = $expecting;
            }

            /// Serializes as a string (with `Display`).
            impl Serialize for $ty {
                __begin_without_finish!();

                fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
                    Ok(Chunk::Atom(Atom::Str(Cow::Owned(self.to_string()))))
                }
            }

            impl<'de> Deserialize<'de> for $ty {
                fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
                    ParseSlot::make_handle(out)
                }

                #[inline]
                fn __private_atom_into(
                    out: &mut Option<Self>,
                    atom: Atom,
                    state: &mut State,
                ) -> Result<(), Error> {
                    let sink = ParseSlot::wrap(out);
                    sink.atom(atom, state)?;
                    sink.finish(state)
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    out: &mut Option<Self>,
                    atom: Atom<'de>,
                    state: &mut State,
                ) -> Result<(), Error> {
                    Self::__private_atom_into(out, atom, state)
                }
            }
        )*
    };
}

parse_from_str! {
    IpAddr => "IP address",
    Ipv4Addr => "IPv4 address",
    Ipv6Addr => "IPv6 address",
    SocketAddr => "socket address",
    SocketAddrV4 => "IPv4 socket address",
    SocketAddrV6 => "IPv6 socket address",
}

// Paths

/// Serializes as a string.  Paths which are not valid UTF-8 fail to
/// serialize.
impl Serialize for Path {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.to_str() {
            Some(value) => Ok(Chunk::Atom(Atom::Str(Cow::Borrowed(value)))),
            None => Err(Error::new(
                ErrorKind::Unexpected,
                "path contains invalid UTF-8 characters",
            )),
        }
    }
}

impl Serialize for PathBuf {
    __begin_without_finish!();

    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.as_path().serialize(state)
    }
}

impl<'de> Sink<'de> for SlotWrapper<PathBuf> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("path")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(value) => {
                **self = Some(PathBuf::from(value.into_owned()));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

impl<'de> Deserialize<'de> for PathBuf {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SlotWrapper::make_handle(out)
    }

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
        Self::__private_atom_into(out, atom, state)
    }
}

impl Via<PathBuf> for Box<Path> {
    #[inline]
    fn convert(value: PathBuf) -> Result<Self, Error> {
        Ok(value.into_boxed_path())
    }
}

deserialize_via! {
    [] Box<Path> => PathBuf;
}

// C strings

/// Serializes as bytes (without the nul terminator).
impl Serialize for CStr {
    __begin_without_finish!();

    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(Bytes::new(self.to_bytes()))))
    }
}

impl Serialize for CString {
    __begin_without_finish!();

    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.as_c_str().serialize(state)
    }
}

impl Via<Vec<u8>> for CString {
    #[inline]
    fn convert(value: Vec<u8>) -> Result<Self, Error> {
        CString::new(value).map_err(|err| Error::new(ErrorKind::Unexpected, err.to_string()))
    }
}

impl Via<CString> for Box<CStr> {
    #[inline]
    fn convert(value: CString) -> Result<Self, Error> {
        Ok(value.into_boxed_c_str())
    }
}

deserialize_via! {
    [] CString => Vec<u8>;
    [] Box<CStr> => CString;
}
