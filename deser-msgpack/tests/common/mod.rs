//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use deser::State;
use deser::de::{Deserialize, DeserializeOwned, Sink, SinkHandle};
use deser::ext::ExtValue;
use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, Error};

/// A reader that returns the input in chunks of a fixed size.
pub struct Chunked<'a> {
    pub input: &'a [u8],
    pub size: usize,
}

impl std::io::Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.size.min(buf.len()).min(self.input.len());
        buf[..len].copy_from_slice(&self.input[..len]);
        self.input = &self.input[len..];
        Ok(len)
    }
}

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
    deser_msgpack::from_slice(&hex(s))
}

/// Serializes into a hex string.
pub fn ser<T: Serialize>(value: &T) -> String {
    to_hex(&deser_msgpack::to_vec(value).unwrap())
}

/// A dynamic MessagePack value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits and serializes it back.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    U128(u128),
    I128(i128),
    F32(f32),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    /// Extension values (timestamps and other extensions).
    Ext(ExtValue<'static>),
}

impl Value {
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

impl From<f32> for Value {
    fn from(value: f32) -> Value {
        Value::F32(value)
    }
}

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
            Value::F32(value) => Atom::F32(value),
            Value::F64(value) => Atom::F64(value),
            Value::Str(ref value) => Atom::Str(value.as_str().into()),
            Value::Bytes(ref value) => Atom::Bytes(value.as_slice().into()),
            Value::Ext(ref value) => Atom::Ext(value.as_borrowed()),
            Value::Array(ref items) => return Ok(Chunk::seq(ArrayEmitter(items.iter()), state)),
            Value::Map(ref items) => {
                return Ok(Chunk::map(MapEntryEmitter(items.iter(), None), state));
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
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            ValueSink {
                out,
                compound: None,
                key: None,
                value: None,
            },
            state,
        )
    }
}

enum Compound {
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

struct ValueSink<'a> {
    out: &'a mut Option<Value>,
    compound: Option<Compound>,
    key: Option<Value>,
    value: Option<Value>,
}

impl<'a> ValueSink<'a> {
    fn set(&mut self, value: Value) {
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
        let value = match atom {
            Atom::Null => Value::Null,
            Atom::Bool(value) => Value::Bool(value),
            Atom::Str(value) | Atom::Lexical(value) => Value::Str(value.into_owned()),
            Atom::Bytes(value) => Value::Bytes(value.into_owned()),
            Atom::Char(value) => Value::Str(value.to_string()),
            Atom::U64(value) => Value::U64(value),
            Atom::I64(value) => Value::I64(value),
            Atom::F32(value) => Value::F32(value),
            Atom::F64(value) => Value::F64(value),
            Atom::Ext(ref ext) if ext.is::<u128>() => Value::U128(*ext.downcast_ref().unwrap()),
            Atom::Ext(ref ext) if ext.is::<i128>() => Value::I128(*ext.downcast_ref().unwrap()),
            Atom::Ext(ref ext) => Value::Ext(ext.to_static()),
            other => return self.unexpected_atom(other, state),
        };
        self.set(value);
        Ok(())
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        self.compound = Some(Compound::Map(Vec::new()));
        Ok(())
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        self.compound = Some(Compound::Array(Vec::new()));
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(Deserialize::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(Compound::Array(_)) = self.compound {
            self.flush();
        }
        Ok(Deserialize::deserialize_into(&mut self.value, state))
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
