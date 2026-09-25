//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use deser::de::{Deserialize, DeserializeOwned, Sink, SinkHandle};
use deser::ext::ExtValue;
use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::State;
use deser::{Atom, Error};
use deser_cbor::Simple;

/// Decodes a hex string.  Spaces are ignored.
pub fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(s.len().is_multiple_of(2), "odd hex string {:?}", s);
    (0..s.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&s[idx..idx + 2], 16).unwrap())
        .collect()
}

/// Encodes bytes as hex string.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Deserializes from a hex string.
pub fn de<T: DeserializeOwned>(s: &str) -> Result<T, Error> {
    deser_cbor::from_slice(&hex(s))
}

/// Serializes into a hex string.
pub fn ser<T: Serialize>(value: &T) -> String {
    to_hex(&deser_cbor::to_vec(value).unwrap())
}

/// A dynamic CBOR value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits (including tags) and serializes it back.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    U128(u128),
    I128(i128),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Tag(u64, Box<Value>),
    Simple(u8),
    /// Other extension values (the well-known types).
    Ext(ExtValue<'static>),
}

impl Value {
    pub fn tag(tag: u64, value: Value) -> Value {
        Value::Tag(tag, Box::new(value))
    }

    pub fn bytes(s: &str) -> Value {
        Value::Bytes(hex(s))
    }

    pub fn ext<T: deser::ext::Extension>(value: T) -> Value {
        Value::Ext(ExtValue::owned(value))
    }
}

macro_rules! from_int {
    ($($ty:ty => $variant:ident),*) => {
        $(impl From<$ty> for Value {
            fn from(value: $ty) -> Value {
                Value::$variant(value as _)
            }
        })*
    };
}

from_int!(u8 => U64, u32 => U64, u64 => U64, i32 => I64, i64 => I64, u128 => U128, i128 => I128);

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        Value::F64(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Bool(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::Str(value.into())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(value: Vec<T>) -> Value {
        Value::Array(value.into_iter().map(Into::into).collect())
    }
}

/// Creates an array value.
#[macro_export]
macro_rules! array {
    ($($value:expr),* $(,)?) => {
        $crate::common::Value::Array(vec![$($crate::common::Value::from($value)),*])
    };
}

/// Creates a map value.
#[macro_export]
macro_rules! map {
    ($($key:expr => $value:expr),* $(,)?) => {
        $crate::common::Value::Map(vec![
            $(($crate::common::Value::from($key), $crate::common::Value::from($value))),*
        ])
    };
}

impl Serialize for Value {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(match *self {
            Value::Null => Atom::Null,
            Value::Bool(value) => Atom::Bool(value),
            Value::U64(value) => Atom::U64(value),
            Value::I64(value) => Atom::I64(value),
            Value::U128(ref value) => Atom::Ext(ExtValue::borrowed(value)),
            Value::I128(ref value) => Atom::Ext(ExtValue::borrowed(value)),
            Value::F64(value) => Atom::F64(value),
            Value::Str(ref value) => Atom::Str(value.as_str().into()),
            Value::Bytes(ref value) => Atom::Bytes(value.as_slice().into()),
            Value::Simple(value) => Atom::Ext(ExtValue::owned(Simple::new(value).unwrap())),
            Value::Ext(ref value) => Atom::Ext(value.as_borrowed()),
            Value::Array(ref items) => return Ok(Chunk::Seq(Box::new(ArrayEmitter(items.iter())))),
            Value::Map(ref items) => {
                return Ok(Chunk::Map(Box::new(MapEntryEmitter(items.iter(), None))))
            }
            Value::Tag(tag, ref value) => {
                deser_cbor::tag::push_tag(state, tag);
                return value.serialize(state);
            }
        }))
    }
}

struct ArrayEmitter<'a>(std::slice::Iter<'a, Value>);

impl<'a> SeqEmitter for ArrayEmitter<'a> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

struct MapEntryEmitter<'a>(std::slice::Iter<'a, (Value, Value)>, Option<&'a Value>);

impl<'a> MapEmitter for MapEntryEmitter<'a> {
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(|(key, value)| {
            self.1 = Some(value);
            SerializeHandle::to(key)
        }))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        Ok(SerializeHandle::to(self.1.unwrap()))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ValueSink {
            out,
            tags: Vec::new(),
            compound: None,
            key: None,
            value: None,
        })
    }
}

enum Compound {
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

struct ValueSink<'a> {
    out: &'a mut Option<Value>,
    tags: Vec<u64>,
    compound: Option<Compound>,
    key: Option<Value>,
    value: Option<Value>,
}

impl<'a> ValueSink<'a> {
    fn take_tags(&mut self, state: &mut State) {
        self.tags = std::iter::from_fn(|| deser_cbor::take_tag(state)).collect();
    }

    fn set(&mut self, mut value: Value) {
        for &tag in self.tags.iter().rev() {
            value = Value::Tag(tag, Box::new(value));
        }
        *self.out = Some(value);
    }

    fn flush(&mut self) {
        match self.compound {
            Some(Compound::Array(ref mut items)) => items.extend(self.value.take()),
            Some(Compound::Map(ref mut items)) => {
                if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                    items.push((key, value));
                }
            }
            None => {}
        }
    }
}

impl<'a, 'de> Sink<'de> for ValueSink<'a> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.take_tags(state);
        let value = match atom {
            Atom::Null => Value::Null,
            Atom::Bool(value) => Value::Bool(value),
            Atom::Str(value) => Value::Str(value.into_owned()),
            Atom::Bytes(value) => Value::Bytes(value.into_owned()),
            Atom::Char(value) => Value::Str(value.to_string()),
            Atom::U64(value) => Value::U64(value),
            Atom::I64(value) => Value::I64(value),
            Atom::F64(value) => Value::F64(value),
            Atom::Ext(ref ext) if ext.is::<u128>() => Value::U128(*ext.downcast_ref().unwrap()),
            Atom::Ext(ref ext) if ext.is::<i128>() => Value::I128(*ext.downcast_ref().unwrap()),
            Atom::Ext(ref ext) if ext.is::<Simple>() => {
                Value::Simple(ext.downcast_ref::<Simple>().unwrap().value())
            }
            Atom::Ext(ref ext) => Value::Ext(ext.to_static()),
            other => return self.unexpected_atom(other, state),
        };
        self.set(value);
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.take_tags(state);
        self.compound = Some(Compound::Map(Vec::new()));
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.take_tags(state);
        self.compound = Some(Compound::Array(Vec::new()));
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(Deserialize::deserialize_into(&mut self.key))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(Compound::Array(_)) = self.compound {
            self.flush();
        }
        Ok(Deserialize::deserialize_into(&mut self.value))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        self.flush();
        match self.compound.take() {
            Some(Compound::Array(items)) => self.set(Value::Array(items)),
            Some(Compound::Map(items)) => self.set(Value::Map(items)),
            None => {}
        }
        Ok(())
    }
}
