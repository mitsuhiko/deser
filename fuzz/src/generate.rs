//! Generates dynamic values from fuzz input.
use std::str::FromStr;

use arbitrary::{Result, Unstructured};
use deser::ext::{
    BigInt, Datetime, Decimal, Duration, ExtValue, Extension, Number, Timestamp, Uuid,
};
use deser::{Atom, Bytes, Implicit, ImplicitValue};
use deser_value::{Kind, Map, Seq, Value};

/// Strings that are special in some format.
const STRINGS: &[&str] = &[
    "",
    " ",
    "true",
    "false",
    "yes",
    "no",
    "on",
    "null",
    "Null",
    "~",
    "nil",
    "None",
    "NULL",
    "0",
    "-0",
    "1",
    "-1",
    "007",
    "0x1F",
    "0o17",
    "0b101",
    "1_000",
    "1.0",
    "1e5",
    ".5",
    "+1",
    "inf",
    "-inf",
    ".inf",
    "NaN",
    ".nan",
    "2024-01-02",
    "2024-01-02T03:04:05Z",
    "12:30:00",
    "<<",
    "-",
    "- a",
    "a: b",
    "a:b",
    "#",
    "# x",
    ";",
    "=",
    "a=b",
    "&",
    "&amp;",
    "%",
    "%zz",
    "+",
    "?",
    "[",
    "]",
    "{",
    "}",
    "[]",
    "{}",
    ",",
    "'",
    "\"",
    "\\",
    "\\n",
    "\n",
    "\r\n",
    "\r",
    "\t",
    " a ",
    "a\nb",
    "a\n",
    "\u{0}",
    "\u{7f}",
    "\u{85}",
    "\u{a0}",
    "\u{feff}",
    "\u{2028}",
    "\u{fffd}",
    "\u{10ffff}",
    "😀",
    "<a>",
    "</a>",
    "]]>",
    "<!--",
    "@",
    "@a",
    "#text",
    "$value",
    "xmlns",
    "xmlns:a",
    "a.b",
    "a[b]",
    "a[]",
    "a[0]",
    "a__b",
    "__",
    "*a",
    "!a",
    "|",
    ">",
    "...",
    "---",
    "--- a",
    "=cmd",
    "NaN",
];

fn string(u: &mut Unstructured<'_>) -> Result<String> {
    Ok(match u.int_in_range(0..=3)? {
        0 | 1 => u.choose(STRINGS)?.to_string(),
        2 => {
            // a few special strings glued together
            let mut rv = String::new();
            for _ in 0..u.int_in_range(1..=4)? {
                rv.push_str(u.choose(STRINGS)?);
            }
            rv
        }
        _ => u.arbitrary()?,
    })
}

/// Texts of the well-known extension types.
const EXT_TEXTS: &[&str] = &[
    "0",
    "-0",
    "1",
    "-1",
    "1.5",
    "-0.0",
    "1e5",
    "1E-7",
    "0.1",
    "123456789012345678901234567890",
    "-340282366920938463463374607431768211456",
    "340282366920938463463374607431768211456",
    "18446744073709551616",
    "1.000000000000000000001",
    "2024-01-02",
    "2024-01-02T03:04:05Z",
    "2024-01-02T03:04:05.123456789+01:30",
    "2024-01-02T03:04:05",
    "2024-01-02 03:04:05-00:00",
    "03:04:05",
    "03:04:05.5",
    "0000-01-01T00:00:00Z",
    "9999-12-31T23:59:59.999999999Z",
    "1970-01-01T00:00:00Z",
    "PT0S",
    "PT1.5S",
    "-PT1S",
    "P1DT2H3M4S",
    "1s",
    "67e55044-10b1-426f-9247-bb680e5fe0c8",
    "00000000-0000-0000-0000-000000000000",
];

/// The fallback of a [`Custom`] extension value.
#[derive(Debug, Clone, PartialEq)]
enum Fallback {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    Char(char),
    Str(String),
    Bytes(Vec<u8>),
}

/// An extension type that no format knows.
#[derive(Debug, Clone, PartialEq)]
struct Custom(Fallback);

impl Extension for Custom {
    fn name(&self) -> &str {
        "custom"
    }

    fn fallback(&self) -> Atom<'_> {
        match self.0 {
            Fallback::Null => Atom::Null,
            Fallback::Bool(value) => Atom::Bool(value),
            Fallback::U64(value) => Atom::U64(value),
            Fallback::I64(value) => Atom::I64(value),
            Fallback::F64(value) => Atom::F64(value),
            Fallback::Char(value) => Atom::Char(value),
            Fallback::Str(ref value) => Atom::Str(value.as_str().into()),
            Fallback::Bytes(ref value) => Atom::Bytes(Bytes::from(value.as_slice())),
        }
    }
}

/// Generates an extension value (of the well-known types or a custom
/// one).
fn ext(u: &mut Unstructured<'_>) -> Result<Value> {
    let text = match u.ratio(3, 4)? {
        true => u.choose(EXT_TEXTS)?.to_string(),
        false => string(u)?,
    };
    let ext = match u.int_in_range(0..=9)? {
        0 => Datetime::from_str(&text).ok().map(ExtValue::owned),
        1 => Timestamp::from_str(&text).ok().map(ExtValue::owned),
        2 => Some(ExtValue::owned(Timestamp {
            seconds: u.arbitrary()?,
            nanosecond: u.int_in_range(0..=999_999_999)?,
        })),
        3 => Decimal::from_str(&text).ok().map(ExtValue::owned),
        4 => BigInt::from_str(&text).ok().map(ExtValue::owned),
        5 => Duration::from_str(&text).ok().map(ExtValue::owned),
        6 => Some(match u.arbitrary()? {
            true => ExtValue::owned(Uuid(u.arbitrary()?)),
            false => ExtValue::owned(Uuid::from_str(&text).unwrap_or(Uuid([0; 16]))),
        }),
        7 => Number::parse(text)
            .ok()
            .map(ExtValue::owned_value::<Number<'static>>),
        _ => Some(ExtValue::owned(Custom(match u.int_in_range(0..=7)? {
            0 => Fallback::Null,
            1 => Fallback::Bool(u.arbitrary()?),
            2 => Fallback::U64(u.arbitrary()?),
            3 => Fallback::I64(u.arbitrary()?),
            4 => Fallback::F64(u.arbitrary()?),
            5 => Fallback::Char(u.arbitrary()?),
            6 => Fallback::Str(string(u)?),
            _ => Fallback::Bytes(u.arbitrary()?),
        }))),
    };
    Ok(match ext {
        Some(ext) => Value::from(Kind::Ext(ext)),
        None => Value::from(string(u)?),
    })
}

/// Generates a value whose type was inferred from its text (which can
/// be another value than the text says).
fn implicit(u: &mut Unstructured<'_>) -> Result<Value> {
    let text = string(u)?;
    let value = match u.int_in_range(0..=4)? {
        0 => ImplicitValue::Null,
        1 => ImplicitValue::Bool(u.arbitrary()?),
        2 => ImplicitValue::U64(u.arbitrary()?),
        3 => ImplicitValue::I64(u.arbitrary()?),
        _ => ImplicitValue::F64(u.arbitrary()?),
    };
    Ok(Value::from(Kind::Implicit(Implicit::new(text, value))))
}

/// Generates a scalar.
fn scalar(u: &mut Unstructured<'_>) -> Result<Value> {
    Ok(match u.int_in_range(0..=15)? {
        0 => Value::from(()),
        1 => Value::from(u.arbitrary::<bool>()?),
        2 => Value::from(u.arbitrary::<u64>()?),
        3 => Value::from(u.int_in_range(i64::MIN..=-1)?),
        4 => Value::from(u.arbitrary::<f32>()?),
        5 => Value::from(u.arbitrary::<f64>()?),
        6 => Value::from(u.arbitrary::<char>()?),
        7 => Value::from(u.arbitrary::<u128>()?),
        8 => Value::from(u.arbitrary::<i128>()?),
        9 => Value::from(deser::Bytes::new(u.arbitrary::<Vec<u8>>()?)),
        10 => Value::from(Kind::Lexical(string(u)?)),
        // small numbers are more common
        11 => Value::from(u.int_in_range(-2i64..=300)?),
        12 => ext(u)?,
        13 => implicit(u)?,
        _ => Value::from(string(u)?),
    })
}

/// Generates a value of up to the given depth.
pub fn value(u: &mut Unstructured<'_>, depth: usize) -> Result<Value> {
    if depth == 0 {
        return scalar(u);
    }
    Ok(match u.int_in_range(0..=3)? {
        0 => {
            let mut seq = Seq::new();
            for _ in 0..u.int_in_range(0..=4)? {
                seq.push(value(u, depth - 1)?);
            }
            Value::from(seq)
        }
        1 => {
            let mut map = Map::new();
            for _ in 0..u.int_in_range(0..=4)? {
                let key = if u.ratio(1, 4)? {
                    scalar(u)?
                } else {
                    Value::from(string(u)?)
                };
                map.insert(key, value(u, depth - 1)?);
            }
            Value::from(map)
        }
        _ => scalar(u)?,
    })
}
