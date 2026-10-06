//! Generates dynamic values from fuzz input.
use arbitrary::{Result, Unstructured};
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

/// Generates a scalar.
fn scalar(u: &mut Unstructured<'_>) -> Result<Value> {
    Ok(match u.int_in_range(0..=13)? {
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
