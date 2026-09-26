//! A serde deserializer that pulls deser events from a source.
use std::borrow::Cow;

use deser::{Atom, ErrorKind, Event};
use serde::de::{self, DeserializeSeed, Visitor};

use crate::error::Error;

/// A source of events for the deserializer.
///
/// The events of a value are pulled one after another.  Borrowed atoms
/// ([`Cow::Borrowed`]) borrow from the data that is deserialized.
pub(crate) trait Source<'de> {
    /// Returns the next event.
    fn next(&mut self) -> Result<Event<'de>, Error>;

    /// Returns the next event without consuming it.
    fn peek(&mut self) -> Result<&Event<'de>, Error>;
}

/// A source holding a single event.
pub(crate) struct Single<'de>(pub Option<Event<'de>>);

impl<'de> Source<'de> for Single<'de> {
    fn next(&mut self) -> Result<Event<'de>, Error> {
        self.0.take().ok_or_else(unexpected_end)
    }

    fn peek(&mut self) -> Result<&Event<'de>, Error> {
        self.0.as_ref().ok_or_else(unexpected_end)
    }
}

#[cold]
pub(crate) fn unexpected_end() -> Error {
    Error::new(ErrorKind::EndOfFile, "unexpected end of value")
}

#[cold]
fn unexpected_event(event: &Event, expecting: &str) -> Error {
    let got = match event {
        Event::Atom(atom) => atom.name(),
        Event::MapStart(_) => "map",
        Event::SeqStart(_) => "sequence",
        Event::MapEnd => "end of map",
        Event::SeqEnd => "end of sequence",
    };
    Error::new(
        ErrorKind::Unexpected,
        format!("unexpected {}, expected {}", got, expecting),
    )
}

/// Checks if an atom is a null (also an extension value that falls back
/// to null).
fn is_null(atom: &Atom) -> bool {
    match atom {
        Atom::Null => true,
        Atom::Ext(ext) => matches!(ext.fallback(), Atom::Null),
        _ => false,
    }
}

/// Invokes the visitor with an atom.
fn visit_atom<'de, V: Visitor<'de>>(atom: Atom<'de>, visitor: V) -> Result<V::Value, Error> {
    match atom {
        Atom::Null => visitor.visit_unit(),
        Atom::Bool(v) => visitor.visit_bool(v),
        Atom::Str(Cow::Borrowed(v)) | Atom::Lexical(Cow::Borrowed(v)) => {
            visitor.visit_borrowed_str(v)
        }
        Atom::Str(Cow::Owned(v)) | Atom::Lexical(Cow::Owned(v)) => visitor.visit_string(v),
        Atom::Bytes(v) => match v.into_data() {
            Cow::Borrowed(v) => visitor.visit_borrowed_bytes(v),
            Cow::Owned(v) => visitor.visit_byte_buf(v),
        },
        Atom::Char(v) => visitor.visit_char(v),
        Atom::U64(v) => visitor.visit_u64(v),
        Atom::I64(v) => visitor.visit_i64(v),
        Atom::F32(v) => visitor.visit_f32(v),
        Atom::F64(v) => visitor.visit_f64(v),
        Atom::Ext(ext) => {
            // 128 bit integers are extension values in deser.  Small values
            // are passed on as 64 bit integers which all visitors support.
            if let Some(&v) = ext.downcast_ref::<u128>() {
                return match u64::try_from(v) {
                    Ok(v) => visitor.visit_u64(v),
                    Err(_) => visitor.visit_u128(v),
                };
            }
            if let Some(&v) = ext.downcast_ref::<i128>() {
                return match i64::try_from(v) {
                    Ok(v) => visitor.visit_i64(v),
                    Err(_) => visitor.visit_i128(v),
                };
            }
            // serde does not know other extension types, lower them into
            // the core data model.
            match ext.fallback().to_static() {
                Atom::Ext(_) => Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("unsupported {}", ext.name()),
                )),
                fallback => visit_atom(fallback, visitor),
            }
        }
        other => Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("unsupported {}", other.name()),
        )),
    }
}

/// Parses a lexical atom into a type with the rules of deser.
fn parse_lexical<T: deser::de::DeserializeOwned>(value: &str) -> Result<T, Error> {
    let mut out = None;
    let mut state = deser::State::new();
    {
        let mut sink = T::deserialize_into(&mut out);
        sink.atom(Atom::Lexical(Cow::Borrowed(value)), &mut state)?;
        sink.finish(&mut state)?;
    }
    out.ok_or_else(|| Error::new(ErrorKind::Unexpected, "lexical value was not parsed"))
}

/// Deserializes a value from a [`Source`].
///
/// Numbers and booleans are parsed from lexical atoms (see
/// [`Atom::Lexical`]) with the rules of deser.
pub(crate) struct ValueDe<'s, S> {
    src: &'s mut S,
}

impl<'s, S> ValueDe<'s, S> {
    pub(crate) fn new(src: &'s mut S) -> ValueDe<'s, S> {
        ValueDe { src }
    }
}

macro_rules! parse_lexical {
    ($($method:ident => $ty:ty, $visit:ident;)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                if let Event::Atom(Atom::Lexical(s)) = self.src.peek()? {
                    let value = parse_lexical::<$ty>(s)?;
                    self.src.next()?;
                    return visitor.$visit(value);
                }
                self.deserialize_any(visitor)
            }
        )*
    };
}

impl<'de, 's, S: Source<'de>> de::Deserializer<'de> for ValueDe<'s, S> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.src.next()? {
            Event::Atom(atom) => visit_atom(atom, visitor),
            Event::MapStart(shape) => {
                let mut access = MapAccess {
                    src: self.src,
                    len: shape.len(),
                    done: false,
                };
                let value = visitor.visit_map(&mut access)?;
                access.end()?;
                Ok(value)
            }
            Event::SeqStart(shape) => {
                let mut access = SeqAccess {
                    src: self.src,
                    len: shape.len(),
                    done: false,
                };
                let value = visitor.visit_seq(&mut access)?;
                access.end()?;
                Ok(value)
            }
            event => Err(unexpected_event(&event, "value")),
        }
    }

    parse_lexical! {
        deserialize_bool => bool, visit_bool;
        deserialize_i8 => i8, visit_i8;
        deserialize_i16 => i16, visit_i16;
        deserialize_i32 => i32, visit_i32;
        deserialize_i64 => i64, visit_i64;
        deserialize_i128 => i128, visit_i128;
        deserialize_u8 => u8, visit_u8;
        deserialize_u16 => u16, visit_u16;
        deserialize_u32 => u32, visit_u32;
        deserialize_u64 => u64, visit_u64;
        deserialize_u128 => u128, visit_u128;
        deserialize_f32 => f32, visit_f32;
        deserialize_f64 => f64, visit_f64;
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if matches!(self.src.peek()?, Event::Atom(atom) if is_null(atom)) {
            self.src.next()?;
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        // enums are externally tagged: unit variants are just the tag,
        // all others a map with the tag as single key.
        match self.src.next()? {
            Event::Atom(variant) => visitor.visit_enum(EnumAccess {
                src: self.src,
                variant,
                has_content: false,
            }),
            Event::MapStart(_) => {
                let variant = match self.src.next()? {
                    Event::Atom(variant) => variant,
                    event => return Err(unexpected_event(&event, "enum variant")),
                };
                let value = visitor.visit_enum(EnumAccess {
                    src: &mut *self.src,
                    variant,
                    has_content: true,
                })?;
                match self.src.next()? {
                    Event::MapEnd => Ok(value),
                    event => Err(unexpected_event(&event, "end of enum")),
                }
            }
            event => Err(unexpected_event(&event, "enum")),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let mut depth = 0usize;
        loop {
            match self.src.next()? {
                Event::Atom(_) => {}
                Event::MapStart(_) | Event::SeqStart(_) => depth += 1,
                event @ (Event::MapEnd | Event::SeqEnd) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| unexpected_event(&event, "value"))?;
                }
            }
            if depth == 0 {
                return visitor.visit_unit();
            }
        }
    }

    fn is_human_readable(&self) -> bool {
        true
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple
        tuple_struct map struct identifier
    }
}

struct MapAccess<'s, S> {
    src: &'s mut S,
    len: Option<usize>,
    done: bool,
}

impl<'de, 's, S: Source<'de>> MapAccess<'s, S> {
    fn end(self) -> Result<(), Error> {
        if self.done {
            return Ok(());
        }
        match self.src.next()? {
            Event::MapEnd => Ok(()),
            _ => Err(Error::new(
                ErrorKind::WrongLength,
                "map has more entries than expected",
            )),
        }
    }
}

impl<'de, 's, S: Source<'de>> de::MapAccess<'de> for MapAccess<'s, S> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.done {
            return Ok(None);
        }
        if let Event::MapEnd = self.src.peek()? {
            self.src.next()?;
            self.done = true;
            return Ok(None);
        }
        self.len = self.len.map(|x| x.saturating_sub(1));
        seed.deserialize(ValueDe::new(&mut *self.src)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(ValueDe::new(&mut *self.src))
    }

    fn size_hint(&self) -> Option<usize> {
        self.len
    }
}

struct SeqAccess<'s, S> {
    src: &'s mut S,
    len: Option<usize>,
    done: bool,
}

impl<'de, 's, S: Source<'de>> SeqAccess<'s, S> {
    fn end(self) -> Result<(), Error> {
        if self.done {
            return Ok(());
        }
        match self.src.next()? {
            Event::SeqEnd => Ok(()),
            _ => Err(Error::new(
                ErrorKind::WrongLength,
                "sequence has more elements than expected",
            )),
        }
    }
}

impl<'de, 's, S: Source<'de>> de::SeqAccess<'de> for SeqAccess<'s, S> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.done {
            return Ok(None);
        }
        if let Event::SeqEnd = self.src.peek()? {
            self.src.next()?;
            self.done = true;
            return Ok(None);
        }
        self.len = self.len.map(|x| x.saturating_sub(1));
        seed.deserialize(ValueDe::new(&mut *self.src)).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        self.len
    }
}

struct EnumAccess<'s, 'de, S> {
    src: &'s mut S,
    variant: Atom<'de>,
    has_content: bool,
}

impl<'de, 's, S: Source<'de>> de::EnumAccess<'de> for EnumAccess<'s, 'de, S> {
    type Error = Error;
    type Variant = VariantAccess<'s, S>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        let mut variant = Single(Some(Event::Atom(self.variant)));
        let value = seed.deserialize(ValueDe::new(&mut variant))?;
        Ok((
            value,
            VariantAccess {
                src: self.src,
                has_content: self.has_content,
            },
        ))
    }
}

struct VariantAccess<'s, S> {
    src: &'s mut S,
    has_content: bool,
}

impl<'s, S> VariantAccess<'s, S> {
    fn require_content(&self, expected: &str) -> Result<(), Error> {
        if self.has_content {
            Ok(())
        } else {
            Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &expected,
            ))
        }
    }
}

impl<'de, 's, S: Source<'de>> de::VariantAccess<'de> for VariantAccess<'s, S> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        if self.has_content {
            <() as de::Deserialize>::deserialize(ValueDe::new(self.src))?;
        }
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        self.require_content("newtype variant")?;
        seed.deserialize(ValueDe::new(self.src))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        self.require_content("tuple variant")?;
        de::Deserializer::deserialize_seq(ValueDe::new(self.src), visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.require_content("struct variant")?;
        de::Deserializer::deserialize_map(ValueDe::new(self.src), visitor)
    }
}

/// Deserializes a value that is missing.
///
/// Like serde's derive this produces `None` for optional values.  All other
/// values fail with a (non allocating) missing error.
pub(crate) struct MissingDe;

impl<'de> de::Deserializer<'de> for MissingDe {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::missing())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_none()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}
