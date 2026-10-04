//! The writer of OpenStep (ASCII) property lists.
//!
//! OpenStep property lists only have strings, data, arrays and
//! dictionaries: numbers and dates are written as strings, booleans as
//! `YES` and `NO`.  The output is ASCII, other characters are escaped.
use alloc::string::{String, ToString};
use core::fmt::Write;

use crate::ser::Node;

/// Writes a value that is not an array or dictionary.
pub(crate) fn scalar(out: &mut String, node: &Node) {
    match *node {
        Node::Bool(value) => out.push_str(if value { "YES" } else { "NO" }),
        Node::Int(value) => out.push_str(&value.to_string()),
        Node::Real(value) => write_str(out, &format_real(value)),
        Node::Real32(value) if value.is_finite() => {
            write_str(out, zmij::Buffer::new().format_finite(value))
        }
        Node::Real32(value) => write_str(out, &format_real(f64::from(value))),
        Node::Str(ref value) => write_str(out, value),
        Node::Data(ref value) => {
            out.push('<');
            for (idx, byte) in value.iter().enumerate() {
                if idx > 0 && idx % 4 == 0 {
                    out.push(' ');
                }
                write!(out, "{:02x}", byte).unwrap();
            }
            out.push('>');
        }
        Node::Date(ref value) => write_str(out, &value.to_string()),
        Node::Uid(value) => {
            write!(out, "{{CF$UID = {};}}", value).unwrap();
        }
        Node::Array(_) | Node::Dict(_) => unreachable!("containers are written by the writer"),
    }
}

fn format_real(value: f64) -> String {
    if value.is_finite() {
        zmij::Buffer::new().format_finite(value).into()
    } else if value.is_nan() {
        "nan".into()
    } else if value > 0.0 {
        "+infinity".into()
    } else {
        "-infinity".into()
    }
}

/// Writes a string, quoted if necessary.
pub(crate) fn write_str(out: &mut String, value: &str) {
    let unquoted = !value.is_empty()
        && value.bytes().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'/' | b':' | b'.' | b'-')
        });
    if unquoted {
        out.push_str(value);
        return;
    }
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ' '..='~' => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    write!(out, "\\U{:04x}", unit).unwrap();
                }
            }
        }
    }
    out.push('"');
}
