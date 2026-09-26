//! A serde serializer that emits deser events.
use std::borrow::Cow;

use deser::ext::ExtValue;
use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, Bytes, ContainerShape, ErrorKind, Event, State};
use serde::ser::{self, Impossible};

use crate::error::Error;

/// Receives the events produced by the [`EventSerializer`].
pub(crate) trait Emit {
    fn emit(&mut self, event: Event<'_>) -> Result<(), Error>;
}

fn shape(len: Option<usize>) -> ContainerShape {
    match len {
        Some(len) => ContainerShape::new().with_len(len),
        None => ContainerShape::new(),
    }
}

/// Serializes a serde value into events.
///
/// Enums are externally tagged which is also the default of deser.
pub(crate) struct EventSerializer<'e, E: ?Sized> {
    out: &'e mut E,
}

impl<'e, E: Emit + ?Sized> EventSerializer<'e, E> {
    pub(crate) fn new(out: &'e mut E) -> EventSerializer<'e, E> {
        EventSerializer { out }
    }

    fn atom(self, atom: Atom<'_>) -> Result<(), Error> {
        self.out.emit(Event::Atom(atom))
    }

    fn begin_variant(&mut self, variant: &'static str) -> Result<(), Error> {
        self.out
            .emit(Event::MapStart(ContainerShape::new().with_len(1)))?;
        self.out
            .emit(Event::Atom(Atom::Str(Cow::Borrowed(variant))))
    }
}

impl<'e, E: Emit + ?Sized> ser::Serializer for EventSerializer<'e, E> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Compound<'e, E>;
    type SerializeTuple = Compound<'e, E>;
    type SerializeTupleStruct = Compound<'e, E>;
    type SerializeTupleVariant = Compound<'e, E>;
    type SerializeMap = Compound<'e, E>;
    type SerializeStruct = Compound<'e, E>;
    type SerializeStructVariant = Compound<'e, E>;

    fn serialize_bool(self, v: bool) -> Result<(), Error> {
        self.atom(Atom::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<(), Error> {
        self.atom(Atom::I64(v.into()))
    }

    fn serialize_i16(self, v: i16) -> Result<(), Error> {
        self.atom(Atom::I64(v.into()))
    }

    fn serialize_i32(self, v: i32) -> Result<(), Error> {
        self.atom(Atom::I64(v.into()))
    }

    fn serialize_i64(self, v: i64) -> Result<(), Error> {
        self.atom(Atom::I64(v))
    }

    fn serialize_i128(self, v: i128) -> Result<(), Error> {
        // like deser's own `i128`
        self.atom(Atom::Ext(ExtValue::owned(v)))
    }

    fn serialize_u8(self, v: u8) -> Result<(), Error> {
        self.atom(Atom::U64(v.into()))
    }

    fn serialize_u16(self, v: u16) -> Result<(), Error> {
        self.atom(Atom::U64(v.into()))
    }

    fn serialize_u32(self, v: u32) -> Result<(), Error> {
        self.atom(Atom::U64(v.into()))
    }

    fn serialize_u64(self, v: u64) -> Result<(), Error> {
        self.atom(Atom::U64(v))
    }

    fn serialize_u128(self, v: u128) -> Result<(), Error> {
        self.atom(Atom::Ext(ExtValue::owned(v)))
    }

    fn serialize_f32(self, v: f32) -> Result<(), Error> {
        self.atom(Atom::F64(v.into()))
    }

    fn serialize_f64(self, v: f64) -> Result<(), Error> {
        self.atom(Atom::F64(v))
    }

    fn serialize_char(self, v: char) -> Result<(), Error> {
        self.atom(Atom::Char(v))
    }

    fn serialize_str(self, v: &str) -> Result<(), Error> {
        self.atom(Atom::Str(Cow::Borrowed(v)))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), Error> {
        self.atom(Atom::Bytes(Bytes::borrowed(v)))
    }

    fn serialize_none(self) -> Result<(), Error> {
        self.atom(Atom::Null)
    }

    fn serialize_some<T: ser::Serialize + ?Sized>(self, value: &T) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        self.atom(Atom::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        self.atom(Atom::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.atom(Atom::Str(Cow::Borrowed(variant)))
    }

    fn serialize_newtype_struct<T: ser::Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ser::Serialize + ?Sized>(
        mut self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.begin_variant(variant)?;
        value.serialize(EventSerializer::new(&mut *self.out))?;
        self.out.emit(Event::MapEnd)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Compound<'e, E>, Error> {
        self.out.emit(Event::SeqStart(shape(len)))?;
        Ok(Compound::new(self.out, false))
    }

    fn serialize_tuple(self, len: usize) -> Result<Compound<'e, E>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Compound<'e, E>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        mut self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Compound<'e, E>, Error> {
        self.begin_variant(variant)?;
        self.out.emit(Event::SeqStart(shape(Some(len))))?;
        Ok(Compound::new(self.out, true))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Compound<'e, E>, Error> {
        self.out.emit(Event::MapStart(shape(len)))?;
        Ok(Compound::new(self.out, false))
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Compound<'e, E>, Error> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        mut self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Compound<'e, E>, Error> {
        self.begin_variant(variant)?;
        self.out.emit(Event::MapStart(shape(Some(len))))?;
        Ok(Compound::new(self.out, true))
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

/// Serializes the contents of maps and sequences.
pub(crate) struct Compound<'e, E: ?Sized> {
    out: &'e mut E,
    /// the compound is the content of an enum variant.
    variant: bool,
}

impl<'e, E: Emit + ?Sized> Compound<'e, E> {
    fn new(out: &'e mut E, variant: bool) -> Compound<'e, E> {
        Compound { out, variant }
    }

    fn value<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(EventSerializer::new(&mut *self.out))
    }

    fn end(self, end: Event<'static>) -> Result<(), Error> {
        self.out.emit(end)?;
        if self.variant {
            self.out.emit(Event::MapEnd)?;
        }
        Ok(())
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeSeq for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::SeqEnd)
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeTuple for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::SeqEnd)
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeTupleStruct for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::SeqEnd)
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeTupleVariant for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::SeqEnd)
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeMap for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ser::Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.value(key)
    }

    fn serialize_value<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::MapEnd)
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeStruct for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ser::Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.out.emit(Event::Atom(Atom::Str(Cow::Borrowed(key))))?;
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::MapEnd)
    }
}

impl<'e, E: Emit + ?Sized> ser::SerializeStructVariant for Compound<'e, E> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ser::Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.out.emit(Event::Atom(Atom::Str(Cow::Borrowed(key))))?;
        self.value(value)
    }

    fn end(self) -> Result<(), Error> {
        Compound::end(self, Event::MapEnd)
    }
}

/// Checks if a serde value is `None`.
///
/// This serializes the value with a serializer that stops at the first
/// call.  For values other than options that's the only call.
pub(crate) fn is_none<T: ser::Serialize + ?Sized>(value: &T) -> bool {
    matches!(value.serialize(NoneProbe), Ok(true))
}

struct NoneProbe;

macro_rules! not_none {
    ($($method:ident($($ty:ty),*);)*) => {
        $(
            fn $method(self, $(_: $ty),*) -> Result<bool, Error> {
                Ok(false)
            }
        )*
    };
}

macro_rules! not_none_compound {
    ($($method:ident($($ty:ty),*) -> $rv:ty;)*) => {
        $(
            fn $method(self, $(_: $ty),*) -> Result<$rv, Error> {
                Err(Error::cancelled())
            }
        )*
    };
}

impl ser::Serializer for NoneProbe {
    type Ok = bool;
    type Error = Error;
    type SerializeSeq = Impossible<bool, Error>;
    type SerializeTuple = Impossible<bool, Error>;
    type SerializeTupleStruct = Impossible<bool, Error>;
    type SerializeTupleVariant = Impossible<bool, Error>;
    type SerializeMap = Impossible<bool, Error>;
    type SerializeStruct = Impossible<bool, Error>;
    type SerializeStructVariant = Impossible<bool, Error>;

    not_none! {
        serialize_bool(bool);
        serialize_i8(i8);
        serialize_i16(i16);
        serialize_i32(i32);
        serialize_i64(i64);
        serialize_i128(i128);
        serialize_u8(u8);
        serialize_u16(u16);
        serialize_u32(u32);
        serialize_u64(u64);
        serialize_u128(u128);
        serialize_f32(f32);
        serialize_f64(f64);
        serialize_char(char);
        serialize_str(&str);
        serialize_bytes(&[u8]);
        serialize_unit();
        serialize_unit_struct(&'static str);
        serialize_unit_variant(&'static str, u32, &'static str);
    }

    not_none_compound! {
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(&'static str, u32, &'static str, usize)
            -> Self::SerializeTupleVariant;
        serialize_map(Option<usize>) -> Self::SerializeMap;
        serialize_struct(&'static str, usize) -> Self::SerializeStruct;
        serialize_struct_variant(&'static str, u32, &'static str, usize)
            -> Self::SerializeStructVariant;
    }

    fn serialize_none(self) -> Result<bool, Error> {
        Ok(true)
    }

    fn serialize_some<T: ser::Serialize + ?Sized>(self, _value: &T) -> Result<bool, Error> {
        Ok(false)
    }

    fn serialize_newtype_struct<T: ser::Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<bool, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ser::Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<bool, Error> {
        Ok(false)
    }

    fn collect_str<T: std::fmt::Display + ?Sized>(self, _value: &T) -> Result<bool, Error> {
        Ok(false)
    }
}

/// The events of a buffered compound value.
///
/// The events form a single map or sequence (see [`Events::new`]).  Values
/// within it are slices of the events, so no further buffering is needed
/// to serialize them.
pub(crate) struct Events(Vec<Event<'static>>);

impl Events {
    /// Wraps the events of a map or sequence.
    ///
    /// Fails if the events are not a single map or sequence.
    pub(crate) fn new(events: Vec<Event<'static>>) -> Result<Events, deser::Error> {
        match events.first() {
            Some(Event::MapStart(_) | Event::SeqStart(_)) if value_len(&events) == events.len() => {
                Ok(Events(events))
            }
            _ => Err(malformed()),
        }
    }
}

impl Serialize for Events {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, deser::Error> {
        EventsValue(&self.0).chunk()
    }

    fn container_shape(&self) -> ContainerShape {
        EventsValue(&self.0).container_shape()
    }
}

#[cold]
fn malformed() -> deser::Error {
    deser::Error::new(ErrorKind::Unexpected, "malformed serde value")
}

/// Returns the number of events of the value the events start with.
fn value_len(events: &[Event<'static>]) -> usize {
    let mut depth = 0usize;
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::MapStart(_) | Event::SeqStart(_) => depth += 1,
            Event::MapEnd | Event::SeqEnd => depth = depth.saturating_sub(1),
            Event::Atom(_) => {}
        }
        if depth == 0 {
            return index + 1;
        }
    }
    events.len()
}

/// A value within the events, the slice holds exactly its events.
struct EventsValue<'a>(&'a [Event<'static>]);

impl<'a> EventsValue<'a> {
    fn chunk(&self) -> Result<Chunk<'a>, deser::Error> {
        let events = self.0;
        // the content is everything between the start and the end event
        let content = events.get(1..events.len().saturating_sub(1)).unwrap_or(&[]);
        Ok(match events.first() {
            Some(Event::Atom(atom)) => Chunk::Atom(atom.as_borrowed()),
            Some(Event::MapStart(_)) => Chunk::Map(Box::new(EventsEmitter::new(content))),
            Some(Event::SeqStart(_)) => Chunk::Seq(Box::new(EventsEmitter::new(content))),
            _ => return Err(malformed()),
        })
    }

    fn container_shape(&self) -> ContainerShape {
        match self.0.first() {
            Some(Event::MapStart(shape) | Event::SeqStart(shape)) => *shape,
            _ => ContainerShape::new(),
        }
    }
}

impl Serialize for EventsValue<'_> {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, deser::Error> {
        self.chunk()
    }

    fn container_shape(&self) -> ContainerShape {
        EventsValue::container_shape(self)
    }
}

/// Emits the values of a map or sequence within the events.
struct EventsEmitter<'a> {
    rest: &'a [Event<'static>],
    current: EventsValue<'a>,
}

impl<'a> EventsEmitter<'a> {
    fn new(content: &'a [Event<'static>]) -> EventsEmitter<'a> {
        EventsEmitter {
            rest: content,
            current: EventsValue(&[]),
        }
    }

    fn next_value(&mut self) -> Option<SerializeHandle<'_>> {
        if self.rest.is_empty() {
            return None;
        }
        let (value, rest) = self.rest.split_at(value_len(self.rest));
        self.current = EventsValue(value);
        self.rest = rest;
        Some(SerializeHandle::to(&self.current))
    }
}

impl SeqEmitter for EventsEmitter<'_> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, deser::Error> {
        Ok(self.next_value())
    }
}

impl MapEmitter for EventsEmitter<'_> {
    fn next_key(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, deser::Error> {
        Ok(self.next_value())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, deser::Error> {
        EventsEmitter::next_value(self).ok_or_else(malformed)
    }
}
