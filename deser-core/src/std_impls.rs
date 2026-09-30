//! `Serialize` and `Deserialize` for leaf types of the standard library.
//!
//! The containers and pointers are implemented next to the primitives in
//! `ser::impls` and `de::impls`.
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cmp::Reverse;
use core::convert::Infallible;
use core::ffi::CStr;
use core::fmt::Display;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use core::num::{NonZero, Saturating, Wrapping};
use core::ops::{Bound, Range, RangeFrom, RangeInclusive, RangeTo};
use core::str::FromStr;

use crate::State;
use crate::Text;
use crate::de::duplicates::duplicate_field;
use crate::de::impls::{Via, deserialize_via};
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::error::{Error, ErrorKind, unknown_variant};
use crate::event::{Atom, Bytes};
use crate::ext::ExtValue;
use crate::ser::{
    Begin, Chunk, Describe, Serialize, SerializeHandle, SerializeRef, StructEmitter, Variant,
    VariantKind, VariantRepr,
};

make_slot_wrapper!(SlotWrapper);

// PhantomData

/// Serializes as null like `()`.
impl<T: ?Sized + Sync> Serialize for PhantomData<T> {
    begin_without_finish!();

    fn serialize<'a>(_value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Null))
    }

    fn is_optional(_value: &Self) -> bool {
        true
    }
}

impl<'de, T: ?Sized + Send> Sink<'de> for SlotWrapper<PhantomData<T>> {
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
impl<'de, T: ?Sized + Send> Deserialize<'de> for PhantomData<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
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
                fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
                    T::serialize(&value.0, state)
                }

                fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
                    T::finish(&value.0, state)
                }

                #[inline]
                fn __private_begin<'a>(value: &'a Self, state: &mut State) -> Result<Begin<'a>, Error> {
                    T::__private_begin(&value.0, state)
                }

                fn is_optional(value: &Self) -> bool {
                    T::is_optional(&value.0)
                }

                fn container_shape(value: &Self) -> crate::ContainerShape {
                    T::container_shape(&value.0)
                }

                fn describe(value: &Self, d: &mut dyn Describe) {
                    d.newtype(stringify!($ty));
                    T::describe(&value.0, d);
                }
            }

            impl<T: Send> Via<T> for $ty<T> {
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

// ManuallyDrop

/// Serializes as the inner value.
impl<T: Serialize> Serialize for ManuallyDrop<T> {
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        T::serialize(value, state)
    }

    fn finish(value: &Self, state: &mut State) -> Result<(), Error> {
        T::finish(value, state)
    }

    #[inline]
    fn __private_begin<'a>(value: &'a Self, state: &mut State) -> Result<Begin<'a>, Error> {
        T::__private_begin(value, state)
    }

    fn is_optional(value: &Self) -> bool {
        T::is_optional(value)
    }

    fn container_shape(value: &Self) -> crate::ContainerShape {
        T::container_shape(value)
    }

    fn describe(value: &Self, d: &mut dyn Describe) {
        T::describe(value, d);
    }
}

impl<T: Send> Via<T> for ManuallyDrop<T> {
    #[inline]
    fn convert(value: T) -> Result<Self, Error> {
        Ok(ManuallyDrop::new(value))
    }
}

deserialize_via! {
    [T: Deserialize<'de>] ManuallyDrop<T> => T;
}

// Infallible

/// Values of `Infallible` do not exist, they are never serialized.
impl Serialize for Infallible {
    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
        match *value {}
    }
}

impl<'de> Sink<'de> for SlotWrapper<Infallible> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("nothing")
    }
}

/// Deserializing `Infallible` always fails, for instance to rule out a
/// variant of a generic enum (`Result<T, Infallible>`).
impl<'de> Deserialize<'de> for Infallible {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SlotWrapper::make_handle(out)
    }
}

// NonZero

macro_rules! non_zero {
    ($($ty:ty => $atom:ident),* $(,)?) => {
        $(
            impl Serialize for NonZero<$ty> {
                begin_without_finish!();

                fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
                    Ok(Chunk::Atom(non_zero!(@atom $atom, value.get())))
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
            impl Serialize for core::sync::atomic::$ty {
                begin_without_finish!();

                fn serialize<'a>(this: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
                    let value = this.load(core::sync::atomic::Ordering::Relaxed);
                    Ok(Chunk::Atom(Atom::$atom(value as _)))
                }
            }

            #[cfg(target_has_atomic = $cfg)]
            impl Via<$prim> for core::sync::atomic::$ty {
                #[inline]
                fn convert(value: $prim) -> Result<Self, Error> {
                    Ok(core::sync::atomic::$ty::new(value))
                }
            }

            #[cfg(target_has_atomic = $cfg)]
            deserialize_via! {
                [] core::sync::atomic::$ty => $prim;
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
impl<T, E, TA, EA> Serialize<Result<T, E>> for Result<TA, EA>
where
    T: Sync,
    E: Sync,
    TA: Serialize<T>,
    EA: Serialize<E>,
{
    fn describe(value: &Result<T, E>, d: &mut dyn Describe) {
        describe_result(value, d);
    }

    fn serialize<'a>(value: &'a Result<T, E>, state: &mut State) -> Result<Chunk<'a>, Error> {
        let handle = match value {
            Ok(value) => SerializeRef::with_adapter::<TA, T>(value).into(),
            Err(err) => SerializeRef::with_adapter::<EA, E>(err).into(),
        };
        Ok(Chunk::structure(
            ResultEmitter {
                name: result_name(value),
                value: Some(handle),
            },
            state,
        ))
    }

    #[inline]
    fn __private_begin<'a>(value: &'a Result<T, E>, state: &mut State) -> Result<Begin<'a>, Error> {
        Ok(Begin::chunk(
            Self::serialize(value, state)?,
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
                    other => return Err(unknown_variant(Some(other), "Result", &["Ok", "Err"])),
                });
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

/// Creates the sink for a `Result` with adapters.
fn result_sink<'a, 'de, T, E, TA, EA>(
    out: &'a mut Option<Result<T, E>>,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    T: Send + 'a,
    E: Send + 'a,
    TA: Deserialize<'de, T>,
    EA: Deserialize<'de, E>,
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
        T: Send,
        E: Send,
        TA: Deserialize<'de, T>,
        EA: Deserialize<'de, E>,
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

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            Ok(match self.variant {
                Some(ResultVariant::Ok) => TA::deserialize_into(&mut self.ok, state),
                Some(ResultVariant::Err) => EA::deserialize_into(&mut self.err, state),
                None => SinkHandle::null(),
            })
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            match self.variant {
                Some(ResultVariant::Ok) => TA::__private_atom_into(&mut self.ok, atom, state),
                Some(ResultVariant::Err) => EA::__private_atom_into(&mut self.err, atom, state),
                None => Ok(()),
            }
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            match self.variant {
                Some(ResultVariant::Ok) => {
                    TA::__private_borrowed_atom_into(&mut self.ok, atom, state)
                }
                Some(ResultVariant::Err) => {
                    EA::__private_borrowed_atom_into(&mut self.err, atom, state)
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

    // SAFETY: `TA` and `EA` are adapters, the sink only holds a marker of
    // them
    unsafe {
        SinkHandle::arena_unbounded(
            ResultSink::<T, E, TA, EA> {
                slot: out,
                variant: None,
                ok: None,
                err: None,
                _marker: PhantomData,
            },
            state,
        )
    }
}

impl<'de, T, E, TA, EA> Deserialize<'de, Result<T, E>> for Result<TA, EA>
where
    T: Send,
    E: Send,
    TA: Deserialize<'de, T>,
    EA: Deserialize<'de, E>,
{
    fn deserialize_into<'out>(
        out: &'out mut Option<Result<T, E>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        result_sink::<T, E, TA, EA>(out, state)
    }
}

// Types that are represented as strings

/// Types that are deserialized by parsing a string.
trait Parse: FromStr<Err: Display> + Send {
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
                begin_without_finish!();

                fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
                    Ok(Chunk::Atom(Atom::Str(Text::owned(value.to_string()))))
                }
            }

            impl<'de> Deserialize<'de> for $ty {
                fn deserialize_into<'out>(out: &'out mut Option<Self>, _state: &mut State) -> SinkHandle<'out, 'de> {
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

// C strings

/// Serializes as bytes (without the nul terminator).
impl Serialize for CStr {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(Chunk::Atom(Atom::Bytes(Bytes::new(value.to_bytes()))))
    }
}

impl Serialize for CString {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        CStr::serialize(value.as_c_str(), state)
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

// Ranges

/// Emits a fixed number of fields.
struct FieldsEmitter<'a, const N: usize> {
    fields: [(&'static str, SerializeRef<'a>); N],
    index: usize,
}

impl<'a, const N: usize> FieldsEmitter<'a, N> {
    fn chunk(fields: [(&'static str, SerializeRef<'a>); N], state: &mut State) -> Chunk<'a> {
        Chunk::structure(FieldsEmitter { fields, index: 0 }, state)
    }
}

impl<'a, const N: usize> StructEmitter for FieldsEmitter<'a, N> {
    fn next(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        let rv = self
            .fields
            .get(self.index)
            .map(|&(name, value)| (Cow::Borrowed(name), SerializeHandle::from(value)));
        self.index += 1;
        Ok(rv)
    }
}

/// Serializes as a struct with `start` and `end` like serde.
impl<T: Serialize> Serialize for Range<T> {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(FieldsEmitter::chunk(
            [
                ("start", SerializeRef::new(&value.start)),
                ("end", SerializeRef::new(&value.end)),
            ],
            state,
        ))
    }
}

/// Serializes as a struct with `start` and `end` like serde.
impl<T: Serialize> Serialize for RangeInclusive<T> {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(FieldsEmitter::chunk(
            [
                ("start", SerializeRef::new(value.start())),
                ("end", SerializeRef::new(value.end())),
            ],
            state,
        ))
    }
}

/// Serializes as a struct with `start` like serde.
impl<T: Serialize> Serialize for RangeFrom<T> {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(FieldsEmitter::chunk(
            [("start", SerializeRef::new(&value.start))],
            state,
        ))
    }
}

/// Serializes as a struct with `end` like serde.
impl<T: Serialize> Serialize for RangeTo<T> {
    begin_without_finish!();

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(FieldsEmitter::chunk(
            [("end", SerializeRef::new(&value.end))],
            state,
        ))
    }
}

/// Deserializes the fields of a range.
struct RangeSink<'a, T, R> {
    slot: &'a mut Option<R>,
    key: Option<String>,
    start: Option<T>,
    end: Option<T>,
    // the fields of the range, `start` and/or `end`
    fields: &'static [&'static str],
    make: fn(Option<T>, Option<T>) -> R,
}

impl<'a, T, R> RangeSink<'a, T, R> {
    fn handle<'de>(
        slot: &'a mut Option<R>,
        fields: &'static [&'static str],
        make: fn(Option<T>, Option<T>) -> R,
        state: &mut State,
    ) -> SinkHandle<'a, 'de>
    where
        T: Deserialize<'de> + 'a,
        R: Send + 'a,
    {
        SinkHandle::arena(
            RangeSink {
                slot,
                key: None,
                start: None,
                end: None,
                fields,
                make,
            },
            state,
        )
    }
}

impl<'a, 'de, T: Deserialize<'de>, R: Send> Sink<'de> for RangeSink<'a, T, R> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("range")
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(String::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let key = self.key.take().unwrap_or_default();
        Ok(self
            .value_for_key(&key, state)?
            .unwrap_or_else(SinkHandle::null))
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        let slot = match key {
            "start" if self.fields.contains(&"start") => &mut self.start,
            "end" if self.fields.contains(&"end") => &mut self.end,
            _ => return Ok(None),
        };
        if slot.is_some() && !duplicate_field(key, state)? {
            return Ok(Some(SinkHandle::null()));
        }
        Ok(Some(T::deserialize_into(slot, state)))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        for (name, value) in [("start", self.start.is_some()), ("end", self.end.is_some())] {
            if !value && self.fields.contains(&name) {
                return Err(Error::new(
                    ErrorKind::MissingField,
                    format!("missing field `{}`", name),
                ));
            }
        }
        *self.slot = Some((self.make)(self.start.take(), self.end.take()));
        Ok(())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Range<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RangeSink::handle(
            out,
            &["start", "end"],
            |start, end| Range {
                start: start.unwrap(),
                end: end.unwrap(),
            },
            state,
        )
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RangeInclusive<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RangeSink::handle(
            out,
            &["start", "end"],
            |start, end| RangeInclusive::new(start.unwrap(), end.unwrap()),
            state,
        )
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RangeFrom<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RangeSink::handle(
            out,
            &["start"],
            |start, _| RangeFrom {
                start: start.unwrap(),
            },
            state,
        )
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RangeTo<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RangeSink::handle(out, &["end"], |_, end| RangeTo { end: end.unwrap() }, state)
    }
}

// Bound

/// Serializes externally tagged like serde: `"Unbounded"`, `{"Included":
/// value}` or `{"Excluded": value}`.
impl<T: Serialize> Serialize for Bound<T> {
    begin_without_finish!();

    fn describe(value: &Self, d: &mut dyn Describe) {
        let (name, kind) = match value {
            Bound::Included(_) => ("Included", VariantKind::Newtype),
            Bound::Excluded(_) => ("Excluded", VariantKind::Newtype),
            Bound::Unbounded => ("Unbounded", VariantKind::Unit),
        };
        d.variant(&Variant::new("Bound", name, kind, VariantRepr::External));
    }

    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Chunk<'a>, Error> {
        Ok(match this {
            Bound::Included(value) => {
                FieldsEmitter::chunk([("Included", SerializeRef::new(value))], state)
            }
            Bound::Excluded(value) => {
                FieldsEmitter::chunk([("Excluded", SerializeRef::new(value))], state)
            }
            Bound::Unbounded => Chunk::Atom(Atom::Str(Text::borrowed("Unbounded"))),
        })
    }
}

/// Deserializes a `Bound`.
struct BoundSink<'a, T> {
    slot: &'a mut Option<Bound<T>>,
    // the variant, `true` for `Included`
    included: Option<bool>,
    key: Option<String>,
    value: Option<T>,
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for BoundSink<'a, T> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("Bound")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(ref name) | Atom::Lexical(ref name) => {
                if &**name != "Unbounded" {
                    return Err(unknown_variant(Some(name), "Bound", BOUND_VARIANTS));
                }
                *self.slot = Some(Bound::Unbounded);
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if self.included.is_some() {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "expected a map with a single key for Bound",
            ));
        }
        Ok(String::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let key = self.key.take().unwrap_or_default();
        self.included = Some(match &*key {
            "Included" => true,
            "Excluded" => false,
            other => return Err(unknown_variant(Some(other), "Bound", BOUND_VARIANTS)),
        });
        Ok(T::deserialize_into(&mut self.value, state))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        if self.slot.is_some() {
            return Ok(());
        }
        *self.slot = Some(match (self.included, self.value.take()) {
            (Some(true), Some(value)) => Bound::Included(value),
            (Some(false), Some(value)) => Bound::Excluded(value),
            _ => {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "expected a map with a single key for Bound",
                ));
            }
        });
        Ok(())
    }
}

const BOUND_VARIANTS: &[&str] = &["Unbounded", "Included", "Excluded"];

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Bound<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            BoundSink {
                slot: out,
                included: None,
                key: None,
                value: None,
            },
            state,
        )
    }
}
