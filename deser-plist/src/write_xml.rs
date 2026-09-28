//! The writer of XML property lists.
//!
//! The output matches what Core Foundation writes: elements are indented
//! with tabs and data is written as base64 in lines.
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use deser_core::__format::format_finite;
use deser_core::{Error, ErrorKind};

use crate::common::{encode_base64, format_xml_date};
use crate::ser::{Node, Tree};

const HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n";

/// Writes the tree as XML property list.
pub(crate) fn write(tree: &Tree) -> Result<String, Error> {
    let mut out = String::with_capacity(256);
    out.push_str(HEADER);
    // the open containers with the index of the next child
    let mut stack: Vec<(usize, usize)> = Vec::new();
    if open(&mut out, tree, 0, 0)? {
        stack.push((0, 0));
    }
    while let Some(&(id, idx)) = stack.last() {
        let depth = stack.len();
        let child = match tree.nodes[id] {
            Node::Array(ref items) if idx < items.len() => items[idx],
            Node::Dict(ref entries) if idx < entries.len() => {
                let (ref key, value) = entries[idx];
                indent(&mut out, depth);
                out.push_str("<key>");
                escape(&mut out, key);
                out.push_str("</key>\n");
                value
            }
            ref node => {
                indent(&mut out, depth - 1);
                out.push_str(if matches!(node, Node::Dict(_)) {
                    "</dict>\n"
                } else {
                    "</array>\n"
                });
                stack.pop();
                continue;
            }
        };
        stack.last_mut().unwrap().1 += 1;
        if open(&mut out, tree, child, depth)? {
            stack.push((child, 0));
        }
    }
    out.push_str("</plist>\n");
    Ok(out)
}

/// Writes a value.  For containers that are not empty only the start tag
/// is written and `true` is returned.
fn open(out: &mut String, tree: &Tree, id: usize, depth: usize) -> Result<bool, Error> {
    indent(out, depth);
    match tree.nodes[id] {
        Node::Array(ref items) if items.is_empty() => out.push_str("<array/>\n"),
        Node::Array(_) => {
            out.push_str("<array>\n");
            return Ok(true);
        }
        Node::Dict(ref entries) if entries.is_empty() => out.push_str("<dict/>\n"),
        Node::Dict(_) => {
            out.push_str("<dict>\n");
            return Ok(true);
        }
        Node::Bool(true) => out.push_str("<true/>\n"),
        Node::Bool(false) => out.push_str("<false/>\n"),
        Node::Int(value) => {
            if !(i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(&value) {
                return Err(Error::new(
                    ErrorKind::OutOfRange,
                    "integer out of range for XML property lists",
                ));
            }
            out.push_str("<integer>");
            out.push_str(&value.to_string());
            out.push_str("</integer>\n");
        }
        Node::Real(value) => {
            out.push_str("<real>");
            out.push_str(&format_real(value));
            out.push_str("</real>\n");
        }
        Node::Real32(value) => {
            out.push_str("<real>");
            if value.is_finite() {
                out.push_str(&format_finite(value));
            } else {
                out.push_str(&format_real(f64::from(value)));
            }
            out.push_str("</real>\n");
        }
        Node::Str(ref value) => {
            out.push_str("<string>");
            escape(out, value);
            out.push_str("</string>\n");
        }
        Node::Data(ref value) => {
            out.push_str("<data>\n");
            // like Core Foundation the lines get shorter with the
            // indentation, which is capped at eight tabs.
            let data_depth = depth.min(8);
            let line_len = 76 - data_depth * 8;
            let mut encoded = String::new();
            encode_base64(value, &mut encoded);
            for line in encoded.as_bytes().chunks(line_len) {
                indent(out, data_depth);
                // base64 is ASCII
                out.push_str(core::str::from_utf8(line).unwrap());
                out.push('\n');
            }
            indent(out, depth);
            out.push_str("</data>\n");
        }
        Node::Date(ref value) => {
            out.push_str("<date>");
            out.push_str(&format_xml_date(value));
            out.push_str("</date>\n");
        }
        Node::Uid(value) => {
            out.push_str("<dict>\n");
            indent(out, depth + 1);
            out.push_str("<key>CF$UID</key>\n");
            indent(out, depth + 1);
            out.push_str("<integer>");
            out.push_str(&value.to_string());
            out.push_str("</integer>\n");
            indent(out, depth);
            out.push_str("</dict>\n");
        }
    }
    Ok(false)
}

/// Formats a real like Core Foundation for the values that are not
/// finite.
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

/// Escapes text for XML.
fn escape(out: &mut String, text: &str) {
    let mut last = 0;
    for (idx, c) in text.bytes().enumerate() {
        let escaped = match c {
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'&' => "&amp;",
            _ => continue,
        };
        out.push_str(&text[last..idx]);
        out.push_str(escaped);
        last = idx + 1;
    }
    out.push_str(&text[last..]);
}
