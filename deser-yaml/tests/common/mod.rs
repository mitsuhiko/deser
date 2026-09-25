//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use deser::de::{Deserialize, DeserializerState, Sink, SinkHandle};
use deser::{Atom, Error};

/// A dynamic YAML value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits including tags.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Seq(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Tagged(String, Box<Value>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            // NaN is equal to itself for the purpose of the tests
            (Value::Float(a), Value::Float(b)) => a == b || (a.is_nan() && b.is_nan()),
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Seq(a), Value::Seq(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Tagged(a, x), Value::Tagged(b, y)) => a == b && x == y,
            _ => false,
        }
    }
}

impl Value {
    /// Removes all tags.
    pub fn untagged(self) -> Value {
        match self {
            Value::Tagged(_, value) => value.untagged(),
            Value::Seq(items) => Value::Seq(items.into_iter().map(Value::untagged).collect()),
            Value::Map(items) => Value::Map(
                items
                    .into_iter()
                    .map(|(k, v)| (k.untagged(), v.untagged()))
                    .collect(),
            ),
            other => other,
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

impl From<()> for Value {
    fn from(_: ()) -> Value {
        Value::Null
    }
}

/// Creates a sequence value.
#[macro_export]
macro_rules! seq {
    ($($value:expr),* $(,)?) => {
        $crate::common::Value::Seq(vec![$($crate::common::Value::from($value)),*])
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

impl Deserialize for Value {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SinkHandle::boxed(ValueSink {
            out,
            tag: None,
            compound: None,
            key: None,
            value: None,
        })
    }
}

enum Compound {
    Seq(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

struct ValueSink<'a> {
    out: &'a mut Option<Value>,
    tag: Option<String>,
    compound: Option<Compound>,
    key: Option<Value>,
    value: Option<Value>,
}

impl<'a> ValueSink<'a> {
    fn set(&mut self, value: Value) {
        *self.out = Some(match self.tag.take() {
            Some(tag) => Value::Tagged(tag, Box::new(value)),
            None => value,
        });
    }

    fn flush(&mut self) {
        match self.compound {
            Some(Compound::Seq(ref mut items)) => items.extend(self.value.take()),
            Some(Compound::Map(ref mut items)) => {
                if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                    items.push((key, value));
                }
            }
            None => {}
        }
    }
}

impl<'a> Sink for ValueSink<'a> {
    fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
        self.tag = deser_yaml::take_tag(state);
        let value = match atom {
            Atom::Null => Value::Null,
            Atom::Bool(value) => Value::Bool(value),
            Atom::Str(value) => Value::Str(value.into_owned()),
            Atom::Bytes(value) => Value::Bytes(value.into_owned()),
            Atom::Char(value) => Value::Str(value.to_string()),
            Atom::U64(value) => Value::Int(value.into()),
            Atom::I64(value) => Value::Int(value.into()),
            Atom::F64(value) => Value::Float(value),
            Atom::Ext(ref ext) if ext.is::<u128>() => {
                Value::Int(*ext.downcast_ref::<u128>().unwrap() as i128)
            }
            Atom::Ext(ref ext) if ext.is::<i128>() => Value::Int(*ext.downcast_ref().unwrap()),
            other => return self.unexpected_atom(other, state),
        };
        self.set(value);
        Ok(())
    }

    fn map(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
        self.tag = deser_yaml::take_tag(state);
        self.compound = Some(Compound::Map(Vec::new()));
        Ok(())
    }

    fn seq(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
        self.tag = deser_yaml::take_tag(state);
        self.compound = Some(Compound::Seq(Vec::new()));
        Ok(())
    }

    fn next_key(&mut self, _state: &mut DeserializerState) -> Result<SinkHandle<'_>, Error> {
        self.flush();
        Ok(Deserialize::deserialize_into(&mut self.key))
    }

    fn next_value(&mut self, _state: &mut DeserializerState) -> Result<SinkHandle<'_>, Error> {
        if let Some(Compound::Seq(_)) = self.compound {
            self.flush();
        }
        Ok(Deserialize::deserialize_into(&mut self.value))
    }

    fn finish(&mut self, _state: &mut DeserializerState) -> Result<(), Error> {
        self.flush();
        match self.compound.take() {
            Some(Compound::Seq(items)) => self.set(Value::Seq(items)),
            Some(Compound::Map(items)) => self.set(Value::Map(items)),
            None => {}
        }
        Ok(())
    }
}

/// Parses a stream of JSON values (as in the `in.json` files of the YAML
/// test suite).
///
/// This is a minimal parser for well formed test data.
pub fn parse_json_stream(input: &str) -> Vec<Value> {
    let mut parser = JsonParser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    let mut rv = Vec::new();
    loop {
        parser.skip_ws();
        if parser.pos == parser.bytes.len() {
            return rv;
        }
        rv.push(parser.value());
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, s: &str) {
        assert!(
            self.bytes[self.pos..].starts_with(s.as_bytes()),
            "expected {:?} at {}",
            s,
            self.pos
        );
        self.pos += s.len();
    }

    fn value(&mut self) -> Value {
        self.skip_ws();
        match self.bytes[self.pos] {
            b'n' => {
                self.expect("null");
                Value::Null
            }
            b't' => {
                self.expect("true");
                Value::Bool(true)
            }
            b'f' => {
                self.expect("false");
                Value::Bool(false)
            }
            b'"' => Value::Str(self.string()),
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.bytes[self.pos] == b']' {
                        self.pos += 1;
                        return Value::Seq(items);
                    }
                    if !items.is_empty() {
                        self.expect(",");
                    }
                    items.push(self.value());
                }
            }
            b'{' => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.bytes[self.pos] == b'}' {
                        self.pos += 1;
                        return Value::Map(items);
                    }
                    if !items.is_empty() {
                        self.expect(",");
                        self.skip_ws();
                    }
                    let key = Value::Str(self.string());
                    self.skip_ws();
                    self.expect(":");
                    items.push((key, self.value()));
                }
            }
            _ => {
                let start = self.pos;
                while self.pos < self.bytes.len()
                    && matches!(
                        self.bytes[self.pos],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.pos += 1;
                }
                let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
                match text.parse::<i128>() {
                    Ok(value) => Value::Int(value),
                    Err(_) => Value::Float(text.parse().unwrap()),
                }
            }
        }
    }

    fn string(&mut self) -> String {
        self.expect("\"");
        let mut rv = String::new();
        loop {
            let rest = std::str::from_utf8(&self.bytes[self.pos..]).unwrap();
            let c = rest.chars().next().unwrap();
            self.pos += c.len_utf8();
            match c {
                '"' => return rv,
                '\\' => {
                    let e = self.bytes[self.pos];
                    self.pos += 1;
                    rv.push(match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\x08',
                        b'f' => '\x0c',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut code = self.hex4();
                            if (0xd800..0xdc00).contains(&code) {
                                self.expect("\\u");
                                let low = self.hex4();
                                code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                            }
                            char::from_u32(code).unwrap()
                        }
                        _ => panic!("invalid escape"),
                    });
                }
                c => rv.push(c),
            }
        }
    }

    fn hex4(&mut self) -> u32 {
        let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).unwrap();
        self.pos += 4;
        u32::from_str_radix(hex, 16).unwrap()
    }
}
