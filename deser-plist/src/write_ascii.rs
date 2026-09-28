//! The writer of OpenStep (ASCII) property lists.
//!
//! OpenStep property lists only have strings, data, arrays and
//! dictionaries: numbers and dates are written as strings, booleans as
//! `YES` and `NO`.  The output is ASCII, other characters are escaped.
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use deser_core::__format::format_finite;

use crate::ser::{Node, Tree};

/// Writes the tree as OpenStep property list.
pub(crate) fn write(tree: &Tree) -> String {
    let mut out = String::with_capacity(256);
    // the open containers with the index of the next child
    let mut stack: Vec<(usize, usize)> = Vec::new();
    if open(&mut out, tree, 0) {
        stack.push((0, 0));
    } else {
        out.push('\n');
    }
    while let Some(&(id, idx)) = stack.last() {
        let depth = stack.len();
        let child = match tree.nodes[id] {
            Node::Array(ref items) if idx < items.len() => {
                indent(&mut out, depth);
                items[idx]
            }
            Node::Dict(ref entries) if idx < entries.len() => {
                let (ref key, value) = entries[idx];
                indent(&mut out, depth);
                write_str(&mut out, key);
                out.push_str(" = ");
                value
            }
            ref node => {
                stack.pop();
                indent(&mut out, depth - 1);
                out.push(if matches!(node, Node::Dict(_)) {
                    '}'
                } else {
                    ')'
                });
                out.push_str(separator(tree, stack.last()));
                continue;
            }
        };
        stack.last_mut().unwrap().1 += 1;
        if open(&mut out, tree, child) {
            stack.push((child, 0));
        } else {
            out.push_str(separator(tree, stack.last()));
        }
    }
    out
}

/// Returns what follows a value in its container.
fn separator(tree: &Tree, parent: Option<&(usize, usize)>) -> &'static str {
    match parent.map(|&(id, _)| &tree.nodes[id]) {
        Some(Node::Dict(_)) => ";\n",
        Some(_) => ",\n",
        None => "\n",
    }
}

/// Writes a value.  For containers that are not empty only the opening
/// bracket is written and `true` is returned.
fn open(out: &mut String, tree: &Tree, id: usize) -> bool {
    match tree.nodes[id] {
        Node::Array(ref items) if items.is_empty() => out.push_str("()"),
        Node::Array(_) => {
            out.push_str("(\n");
            return true;
        }
        Node::Dict(ref entries) if entries.is_empty() => out.push_str("{}"),
        Node::Dict(_) => {
            out.push_str("{\n");
            return true;
        }
        Node::Bool(value) => out.push_str(if value { "YES" } else { "NO" }),
        Node::Int(value) => out.push_str(&value.to_string()),
        Node::Real(value) => write_str(out, &format_real(value)),
        Node::Real32(value) if value.is_finite() => write_str(out, &format_finite(value)),
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
    }
    false
}

fn format_real(value: f64) -> String {
    if value.is_finite() {
        format_finite(value)
    } else if value.is_nan() {
        "nan".into()
    } else if value > 0.0 {
        "+infinity".into()
    } else {
        "-infinity".into()
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}

/// Writes a string, quoted if necessary.
fn write_str(out: &mut String, value: &str) {
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
