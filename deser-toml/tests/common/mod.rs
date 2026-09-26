//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use std::borrow::Cow;

use deser::State;
use deser::de::{Deserialize, Sink, SinkHandle};
use deser::ext::ExtValue;
use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, Error};
use deser_toml::{Datetime, Offset};

/// A dynamic TOML value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits including date-times.
#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Int(i128),
    Float(f64),
    Bool(bool),
    Datetime(Datetime),
    Array(Vec<Value>),
    Table(Vec<(String, Value)>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            // NaN is equal to itself for the purpose of the tests
            (Value::Float(a), Value::Float(b)) => {
                (a == b && a.is_sign_negative() == b.is_sign_negative())
                    || (a.is_nan() && b.is_nan())
            }
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Datetime(a), Value::Datetime(b)) => datetime_eq(a, b),
            (Value::Array(a), Value::Array(b)) => a == b,
            // tables are compared without considering the order
            (Value::Table(a), Value::Table(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
            }
            _ => false,
        }
    }
}

/// Compares date-times.  Offset date-times are compared as instants.
fn datetime_eq(a: &Datetime, b: &Datetime) -> bool {
    match (a.offset, b.offset) {
        (Some(_), Some(_)) => instant(a) == instant(b),
        _ => a == b,
    }
}

/// Returns the seconds since the epoch and the nanoseconds.
fn instant(dt: &Datetime) -> (i64, u32) {
    let date = dt.date.unwrap();
    let time = dt.time.unwrap();
    // days from civil, see http://howardhinnant.github.io/date_algorithms.html
    let (y, m, d) = (
        i64::from(date.year),
        i64::from(date.month),
        i64::from(date.day),
    );
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let offset = match dt.offset.unwrap() {
        Offset::Z => 0,
        Offset::Custom { minutes } => i64::from(minutes),
    };
    let secs = days * 86400
        + i64::from(time.hour) * 3600
        + i64::from(time.minute) * 60
        + i64::from(time.second)
        - offset * 60;
    (secs, time.nanosecond)
}

impl Value {
    /// Converts a value parsed from the tagged JSON of toml-test.
    ///
    /// Values are represented as `{"type": "integer", "value": "42"}`.
    pub fn from_tagged_json(json: Value) -> Result<Value, String> {
        match json {
            Value::Table(items) => {
                if items.len() == 2 {
                    let get = |key: &str| {
                        items.iter().find_map(|(k, v)| match v {
                            Value::Str(v) if k == key => Some(v.as_str()),
                            _ => None,
                        })
                    };
                    if let (Some(ty), Some(value)) = (get("type"), get("value")) {
                        return Value::from_tagged(ty, value);
                    }
                }
                items
                    .into_iter()
                    .map(|(k, v)| Ok((k, Value::from_tagged_json(v)?)))
                    .collect::<Result<_, String>>()
                    .map(Value::Table)
            }
            Value::Array(items) => items
                .into_iter()
                .map(Value::from_tagged_json)
                .collect::<Result<_, String>>()
                .map(Value::Array),
            other => Err(format!("unexpected value in tagged JSON: {:?}", other)),
        }
    }

    fn from_tagged(ty: &str, value: &str) -> Result<Value, String> {
        let datetime = |date: bool, time: bool, offset: bool| {
            let dt: Datetime = value
                .parse()
                .map_err(|err| format!("invalid {} {:?}: {}", ty, value, err))?;
            if dt.date.is_some() != date
                || dt.time.is_some() != time
                || dt.offset.is_some() != offset
            {
                return Err(format!("wrong kind of date-time for {}: {:?}", ty, value));
            }
            Ok(Value::Datetime(dt))
        };
        match ty {
            "string" => Ok(Value::Str(value.into())),
            "integer" => value
                .parse()
                .map(Value::Int)
                .map_err(|_| format!("invalid integer {:?}", value)),
            "float" => value
                .parse()
                .map(Value::Float)
                .map_err(|_| format!("invalid float {:?}", value)),
            "bool" => match value {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err(format!("invalid bool {:?}", value)),
            },
            "datetime" => datetime(true, true, true),
            "datetime-local" => datetime(true, true, false),
            "date-local" => datetime(true, false, false),
            "time-local" => datetime(false, true, false),
            _ => Err(format!("unknown type {:?}", ty)),
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::Str(value.into())
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Value {
        Value::Int(value.into())
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        Value::Float(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Bool(value)
    }
}

impl From<Datetime> for Value {
    fn from(value: Datetime) -> Value {
        Value::Datetime(value)
    }
}

/// Creates an array value.
#[macro_export]
macro_rules! array {
    ($($value:expr),* $(,)?) => {
        $crate::common::Value::Array(vec![$($crate::common::Value::from($value)),*])
    };
}

/// Creates a table value.
#[macro_export]
macro_rules! table {
    ($($key:expr => $value:expr),* $(,)?) => {
        $crate::common::Value::Table(vec![
            $((String::from($key), $crate::common::Value::from($value))),*
        ])
    };
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ValueSink {
            out,
            compound: None,
            key: None,
            value: None,
        })
    }
}

enum Compound {
    Array(Vec<Value>),
    Table(Vec<(String, Value)>),
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
            Some(Compound::Table(ref mut items)) => {
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
        *self.out = Some(match atom {
            Atom::Bool(value) => Value::Bool(value),
            Atom::Str(value) => Value::Str(value.into_owned()),
            Atom::U64(value) => Value::Int(value.into()),
            Atom::I64(value) => Value::Int(value.into()),
            Atom::Float(value) => Value::Float(value.value()),
            Atom::Ext(ref ext) if ext.is::<Datetime>() => {
                Value::Datetime(*ext.downcast_ref::<Datetime>().unwrap())
            }
            other => return self.unexpected_atom(other, state),
        });
        Ok(())
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        self.compound = Some(Compound::Table(Vec::new()));
        Ok(())
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
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
            Some(Compound::Array(items)) => *self.out = Some(Value::Array(items)),
            Some(Compound::Table(items)) => *self.out = Some(Value::Table(items)),
            None => {}
        }
        Ok(())
    }
}

impl Serialize for Value {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(match *self {
            Value::Str(ref value) => Chunk::Atom(Atom::Str(Cow::Borrowed(value))),
            Value::Int(value) => match i64::try_from(value) {
                Ok(value) => Chunk::Atom(Atom::I64(value)),
                Err(_) => Chunk::Atom(Atom::U64(value as u64)),
            },
            Value::Float(value) => Chunk::Atom(Atom::Float(deser::Float::new(value))),
            Value::Bool(value) => Chunk::Atom(Atom::Bool(value)),
            Value::Datetime(ref value) => Chunk::Atom(Atom::Ext(ExtValue::borrowed(value))),
            Value::Array(ref items) => Chunk::Seq(Box::new(ArrayEmitter(items.iter()))),
            Value::Table(ref items) => Chunk::Map(Box::new(TableEmitter {
                iter: items.iter(),
                value: None,
            })),
        })
    }
}

struct ArrayEmitter<'a>(std::slice::Iter<'a, Value>);

impl<'a> SeqEmitter for ArrayEmitter<'a> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

struct TableEmitter<'a> {
    iter: std::slice::Iter<'a, (String, Value)>,
    value: Option<&'a Value>,
}

impl<'a> MapEmitter for TableEmitter<'a> {
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.iter.next().map(|(key, value)| {
            self.value = Some(value);
            SerializeHandle::to(key)
        }))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        Ok(SerializeHandle::to(self.value.take().unwrap()))
    }
}
