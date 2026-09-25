//! Hooks for the conformance tests.  Not part of the public API.
use std::fmt::Write;

use crate::event::{EventKind, Props, ScalarStyle};
use crate::parser::Parser;

/// Guards the test suite against parser bugs that loop forever.
const MAX_EVENTS: usize = 10_000_000;

/// Parses the input and renders the events in the format of the
/// `test.event` files of the YAML test suite.
///
/// Returns the rendered events (up to the error) and the error.
pub fn parse_to_test_events(input: &str) -> (String, Option<String>) {
    let mut parser = Parser::new(input);
    let mut out = String::new();
    for _ in 0..MAX_EVENTS {
        let event = match parser.next_event() {
            Ok(event) => event,
            Err(err) => return (out, Some(err.to_string())),
        };
        write_event(&mut out, &event.kind);
        if event.kind == EventKind::StreamEnd {
            return (out, None);
        }
    }
    (out, Some("too many events".into()))
}

fn write_event(out: &mut String, kind: &EventKind) {
    match kind {
        EventKind::StreamStart => out.push_str("+STR"),
        EventKind::StreamEnd => out.push_str("-STR"),
        EventKind::DocumentStart { explicit } => {
            out.push_str(if *explicit { "+DOC ---" } else { "+DOC" })
        }
        EventKind::DocumentEnd { explicit } => {
            out.push_str(if *explicit { "-DOC ..." } else { "-DOC" })
        }
        EventKind::SequenceStart { props, flow } => {
            out.push_str(if *flow { "+SEQ []" } else { "+SEQ" });
            write_props(out, props);
        }
        EventKind::SequenceEnd => out.push_str("-SEQ"),
        EventKind::MappingStart { props, flow } => {
            out.push_str(if *flow { "+MAP {}" } else { "+MAP" });
            write_props(out, props);
        }
        EventKind::MappingEnd => out.push_str("-MAP"),
        EventKind::Scalar {
            props,
            style,
            value,
        } => {
            out.push_str("=VAL");
            write_props(out, props);
            out.push(' ');
            out.push(match style {
                ScalarStyle::Plain => ':',
                ScalarStyle::SingleQuoted => '\'',
                ScalarStyle::DoubleQuoted => '"',
                ScalarStyle::Literal => '|',
                ScalarStyle::Folded => '>',
            });
            for c in value.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\0' => out.push_str("\\0"),
                    '\x08' => out.push_str("\\b"),
                    '\t' => out.push_str("\\t"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    c => out.push(c),
                }
            }
        }
        EventKind::Alias { anchor } => {
            write!(out, "=ALI *{}", anchor).unwrap();
        }
    }
    out.push('\n');
}

fn write_props(out: &mut String, props: &Props) {
    if let Some(ref anchor) = props.anchor {
        write!(out, " &{}", anchor).unwrap();
    }
    if let Some(ref tag) = props.tag {
        write!(out, " <{}>", tag).unwrap();
    }
}
