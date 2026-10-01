//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use deser::State;
use deser::de::{Deserialize, Sink, SinkHandle, default_atom};
use deser::ext::{ExtValue, Timestamp};
use deser::ser::{Emit, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, Error};
use deser_plist::{Format, SerializerConfig, Uid};

/// A dynamic property list value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits and serializes it back.  Dictionaries keep their order and
/// duplicate keys.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    I128(i128),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
    Date(Timestamp),
    Uid(u64),
    Array(Vec<Value>),
    Map(Vec<(String, Value)>),
    /// Other extension values.
    Ext(ExtValue<'static>),
}

impl Value {
    /// Returns the value with the entries of all dictionaries sorted by
    /// key (as `plutil` writes them).
    pub fn sorted(&self) -> Value {
        match self {
            Value::Array(items) => Value::Array(items.iter().map(Value::sorted).collect()),
            Value::Map(entries) => {
                let mut entries: Vec<_> = entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.sorted()))
                    .collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Map(entries)
            }
            other => other.clone(),
        }
    }

    /// Returns the value of a dictionary entry.
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Map(entries) => {
                &entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .unwrap_or_else(|| panic!("missing key {key:?}"))
                    .1
            }
            other => panic!("not a map: {other:?}"),
        }
    }
}

/// Deserializes a value, panicking on errors.
pub fn parse(input: &[u8]) -> Value {
    deser_plist::from_slice(input).unwrap()
}

/// Returns the message of the error of deserializing a value.
pub fn parse_err(input: &[u8]) -> String {
    let err = deser_plist::from_slice::<Value>(input).unwrap_err();
    err.to_string()
}

/// Serializes a value in the given format.
pub fn write(value: &impl Serialize, format: Format) -> Vec<u8> {
    SerializerConfig::new()
        .format(format)
        .to_vec(value)
        .unwrap()
}

/// Serializes a value in a text format.
pub fn write_str(value: &impl Serialize, format: Format) -> String {
    SerializerConfig::new()
        .format(format)
        .to_string(value)
        .unwrap()
}

macro_rules! from_value {
    ($($ty:ty => $variant:ident),*) => {
        $(impl From<$ty> for Value {
            fn from(value: $ty) -> Value {
                Value::$variant(value.into())
            }
        })*
    };
}

from_value!(u8 => U64, u32 => U64, u64 => U64, i32 => I64, i64 => I64, f64 => F64, bool => Bool);

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::Str(value.into())
    }
}

impl From<Uid> for Value {
    fn from(value: Uid) -> Value {
        Value::Uid(value.get())
    }
}

impl From<Timestamp> for Value {
    fn from(value: Timestamp) -> Value {
        Value::Date(value)
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
            $((String::from($key), $crate::common::Value::from($value))),*
        ])
    };
}

impl Serialize for Value {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(match *this {
            Value::Null => Atom::Null,
            Value::Bool(value) => Atom::Bool(value),
            Value::U64(value) => Atom::U64(value),
            Value::I64(value) => Atom::I64(value),
            Value::I128(ref value) => Atom::Ext(ExtValue::borrowed(value)),
            Value::F64(value) => Atom::F64(value),
            Value::Str(ref value) => Atom::Str(value.as_str().into()),
            Value::Bytes(ref value) => Atom::Bytes(value.as_slice().into()),
            Value::Date(ref value) => Atom::Ext(ExtValue::borrowed(value)),
            Value::Uid(value) => Atom::Ext(ExtValue::owned(Uid::new(value))),
            Value::Ext(ref value) => Atom::Ext(value.as_borrowed()),
            Value::Array(ref items) => return Ok(Emit::seq(ArrayEmitter(items.iter()), state)),
            Value::Map(ref items) => {
                return Ok(Emit::map(MapEntryEmitter(items.iter(), None), state));
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

struct MapEntryEmitter<'a>(std::slice::Iter<'a, (String, Value)>, Option<&'a Value>);

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
    Map(Vec<(String, Value)>),
}

struct ValueSink<'a> {
    out: &'a mut Option<Value>,
    compound: Option<Compound>,
    key: Option<String>,
    value: Option<Value>,
}

impl<'a> ValueSink<'a> {
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
            Atom::U64(value) => Value::U64(value),
            Atom::I64(value) => Value::I64(value),
            Atom::F64(value) => Value::F64(value),
            Atom::Ext(ref ext) if ext.is::<i128>() => Value::I128(*ext.downcast_ref().unwrap()),
            Atom::Ext(ref ext) if ext.is::<Timestamp>() => {
                Value::Date(*ext.downcast_ref().unwrap())
            }
            Atom::Ext(ref ext) if ext.is::<Uid>() => {
                Value::Uid(ext.downcast_ref::<Uid>().unwrap().get())
            }
            Atom::Ext(ref ext) => Value::Ext(ext.to_static()),
            other => return default_atom(self, other, state),
        };
        *self.out = Some(value);
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
        Ok(String::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(Compound::Array(_)) = self.compound {
            self.flush();
        }
        Ok(Value::deserialize_into(&mut self.value, state))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        self.flush();
        match self.compound.take() {
            Some(Compound::Array(items)) => *self.out = Some(Value::Array(items)),
            Some(Compound::Map(items)) => *self.out = Some(Value::Map(items)),
            None => {}
        }
        Ok(())
    }
}
