//! Writes serialization events as YAML.
//!
//! The emitter writes block collections.  Containers are held back until
//! their first item is known so that empty containers can be written as
//! `{}` and `[]` and the layout does not need to be patched afterwards.
use deser::adapters::bytes::BytesFormat;
use deser::ext::{BigInt, Datetime, Decimal, ExtValue, Number, Timestamp};
use deser::{Atom, Error, ErrorKind, Event, State};

use crate::quote::{
    Literal, MAX_SIMPLE_KEY_LEN, is_plain_safe, is_single_quote_safe, push_indent,
    write_double_quoted, write_float, write_single_quoted, write_tag,
};
use crate::resolve::{Version, is_plain_str};
use crate::ser::{MultilineStyle, NullStyle, QuoteStyle, SerializerConfig};
use crate::tag::NodeTag;

/// The length of the lines of long `!!binary` values.
const BINARY_LINE_LEN: usize = 76;

/// Wraps base64 into a literal block of lines.
fn wrap_binary(encoded: &str) -> Literal<'static> {
    let mut lines = String::with_capacity(encoded.len() + encoded.len() / BINARY_LINE_LEN + 1);
    for (idx, chunk) in encoded.as_bytes().chunks(BINARY_LINE_LEN).enumerate() {
        if idx > 0 {
            lines.push('\n');
        }
        // base64 is ASCII
        lines.push_str(std::str::from_utf8(chunk).unwrap());
    }
    Literal::from_lines(lines)
}

/// Where the next node is written.
#[derive(Debug, Clone, Copy)]
enum Pos {
    /// The root of the document.
    Root,
    /// After an indicator (`-`, `?` or the `:` of an explicit key), the
    /// content of the node starts at the given column.
    Inline(usize),
    /// The value of a simple key of a mapping at the given column.
    Value(usize),
}

/// An open block collection.
struct Frame {
    is_map: bool,
    /// The column of the keys or dashes.
    indent: usize,
    /// `true` if the next entry continues the current line (the first entry
    /// of a collection that follows an indicator or starts the document).
    inline: bool,
    /// For maps: `true` if the next node is a value.
    value: bool,
    /// For maps: `true` if the key of the current entry is explicit (`?`).
    explicit: bool,
}

/// A collection whose first event was seen but that was not written yet.
struct Pending {
    is_map: bool,
    tag: Option<String>,
    pos: Pos,
}

/// A scalar that was rendered for a position.
enum Scalar<'a> {
    /// Text that is written as is.
    Text(String),
    /// A literal block scalar.
    Literal(Literal<'a>),
    /// Nothing (a null with [`NullStyle::Empty`]).
    Empty,
}

pub(crate) struct Emitter<'c> {
    config: &'c SerializerConfig,
    out: String,
    stack: Vec<Frame>,
    pending: Option<Pending>,
    /// `true` if a space has to be written before the next inline content
    /// (after an indicator).
    space: bool,
    /// `true` once the root value was written.
    done: bool,
    /// `true` if the current line was already terminated (after block
    /// scalars).
    line_done: bool,
}

impl<'c> Emitter<'c> {
    pub fn new(config: &'c SerializerConfig, out: String) -> Emitter<'c> {
        Emitter {
            config,
            out,
            stack: Vec::new(),
            pending: None,
            space: false,
            done: false,
            line_done: false,
        }
    }

    /// Finishes the document and returns the output.
    pub fn finish(mut self) -> Result<String, Error> {
        if !self.done || !self.stack.is_empty() || self.pending.is_some() {
            return Err(Error::new(ErrorKind::Unexpected, "incomplete document"));
        }
        if !self.line_done {
            self.out.push('\n');
        }
        Ok(self.out)
    }

    pub fn event(&mut self, event: Event, state: &State) -> Result<(), Error> {
        let tag = if state.has_event_data() {
            state.event::<NodeTag>().and_then(|x| x.0.clone())
        } else {
            None
        };
        self.emit(event, tag)
    }

    fn emit(&mut self, event: Event, tag: Option<String>) -> Result<(), Error> {
        if let Some(pending) = self.pending.take() {
            match event {
                Event::MapEnd if pending.is_map => return self.write_empty(pending),
                Event::SeqEnd if !pending.is_map => return self.write_empty(pending),
                _ => self.open(pending),
            }
        }
        match event {
            Event::Atom(atom) => self.atom(atom, tag),
            Event::MapStart(_) => self.start(true, tag),
            Event::SeqStart(_) => self.start(false, tag),
            Event::MapEnd => self.end(true),
            Event::SeqEnd => self.end(false),
        }
    }

    /// Starts a map or sequence, it's written once the next event is known.
    fn start(&mut self, is_map: bool, tag: Option<String>) -> Result<(), Error> {
        let pos = match self.begin_node()? {
            Some(pos) => pos,
            // collections as keys are explicit
            None => self.begin_explicit_key(),
        };
        self.pending = Some(Pending { is_map, tag, pos });
        Ok(())
    }

    fn end(&mut self, is_map: bool) -> Result<(), Error> {
        match self.stack.pop() {
            Some(frame) if frame.is_map == is_map && (!is_map || !frame.value) => {}
            _ => return Err(Error::new(ErrorKind::Unexpected, "unexpected end event")),
        }
        self.complete();
        Ok(())
    }

    /// Writes an empty collection.
    fn write_empty(&mut self, pending: Pending) -> Result<(), Error> {
        self.write_inline_start(pending.tag.as_deref());
        self.out.push_str(if pending.is_map { "{}" } else { "[]" });
        self.complete();
        Ok(())
    }

    /// Writes the start of a non-empty collection.
    fn open(&mut self, pending: Pending) {
        let (indent, inline) = match pending.pos {
            Pos::Root => {
                if let Some(ref tag) = pending.tag {
                    write_tag(&mut self.out, tag);
                    self.out.push('\n');
                }
                (0, true)
            }
            Pos::Inline(column) => match pending.tag {
                Some(ref tag) => {
                    self.flush_space();
                    write_tag(&mut self.out, tag);
                    (column, false)
                }
                None => (column, true),
            },
            Pos::Value(key_column) => {
                if let Some(ref tag) = pending.tag {
                    self.flush_space();
                    write_tag(&mut self.out, tag);
                }
                self.space = false;
                let indent = if pending.is_map || self.config.indent_sequences {
                    key_column + self.config.indent
                } else {
                    key_column
                };
                (indent, false)
            }
        };
        self.stack.push(Frame {
            is_map: pending.is_map,
            indent,
            inline,
            value: false,
            explicit: false,
        });
    }

    /// Writes what precedes a node in its parent and returns the position
    /// of the node.  Returns `None` for keys, they are written by the caller.
    fn begin_node(&mut self) -> Result<Option<Pos>, Error> {
        let Some(frame) = self.stack.last_mut() else {
            if self.done {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "a document can only contain one value",
                ));
            }
            return Ok(Some(Pos::Root));
        };
        let indent = frame.indent;
        if !frame.is_map {
            self.begin_entry();
            self.out.push('-');
            self.space = true;
            return Ok(Some(Pos::Inline(indent + 2)));
        }
        if !frame.value {
            return Ok(None);
        }
        if frame.explicit {
            self.newline(indent);
            self.out.push(':');
            self.space = true;
            Ok(Some(Pos::Inline(indent + 2)))
        } else {
            Ok(Some(Pos::Value(indent)))
        }
    }

    /// Moves to the start of the next entry of the current collection.
    fn begin_entry(&mut self) {
        let frame = self.stack.last_mut().unwrap();
        let indent = frame.indent;
        if frame.inline {
            frame.inline = false;
            self.flush_space();
        } else {
            self.newline(indent);
        }
    }

    /// Writes the `?` of an explicit key and returns the position of the key.
    fn begin_explicit_key(&mut self) -> Pos {
        self.begin_entry();
        let frame = self.stack.last_mut().unwrap();
        frame.explicit = true;
        let indent = frame.indent;
        self.out.push('?');
        self.space = true;
        Pos::Inline(indent + 2)
    }

    /// Records that a node was completed.
    fn complete(&mut self) {
        match self.stack.last_mut() {
            Some(frame) if frame.is_map => {
                if frame.value {
                    frame.explicit = false;
                }
                frame.value = !frame.value;
            }
            Some(_) => {}
            None => self.done = true,
        }
    }

    fn newline(&mut self, indent: usize) {
        if !std::mem::take(&mut self.line_done) {
            self.out.push('\n');
        }
        push_indent(&mut self.out, indent);
        self.space = false;
    }

    fn flush_space(&mut self) {
        if self.space {
            self.out.push(' ');
            self.space = false;
        }
    }

    /// Writes the separator and the tag before inline content.
    fn write_inline_start(&mut self, tag: Option<&str>) {
        self.flush_space();
        if let Some(tag) = tag {
            write_tag(&mut self.out, tag);
            self.out.push(' ');
        }
    }

    fn atom(&mut self, atom: Atom, tag: Option<String>) -> Result<(), Error> {
        // bytes that are sequences are written as sequences of integers
        if let Atom::Bytes(ref bytes) = atom
            && !self.config.binary
            && bytes.fallback.copied().unwrap_or(self.config.bytes) == BytesFormat::SEQ
        {
            self.emit(Event::seq_start(), tag)?;
            for &byte in bytes.iter() {
                self.emit(Event::Atom(Atom::U64(byte.into())), None)?;
            }
            return self.emit(Event::SeqEnd, None);
        }

        let pos = match self.begin_node()? {
            Some(pos) => pos,
            None => return self.key(atom, tag),
        };
        let mut rendered = String::new();
        let (scalar, implicit_tag) = self.render(&atom, false, &mut rendered)?;
        let tag = tag.as_deref().or(implicit_tag);
        match scalar {
            // nothing to write, the indicator or key stands on its own
            Scalar::Empty if !matches!(pos, Pos::Root) && tag.is_none() => {
                self.space = false;
            }
            Scalar::Empty => {
                self.write_inline_start(tag);
                self.out.push_str("null");
            }
            Scalar::Text(text) => {
                self.write_inline_start(tag);
                self.out.push_str(&text);
            }
            Scalar::Literal(literal) => {
                self.write_inline_start(tag);
                // the indentation indicator is relative to the parent node
                let (column, parent) = match pos {
                    // the document is at indentation 0 for indicators
                    Pos::Root => (self.config.indent, 0),
                    Pos::Inline(column) => (column, column as isize - 2),
                    Pos::Value(key_column) => {
                        (key_column + self.config.indent, key_column as isize)
                    }
                };
                literal.write_header(&mut self.out, (column as isize - parent) as usize);
                self.out.push('\n');
                literal.write_body(&mut self.out, column);
                self.line_done = true;
            }
        }
        self.complete();
        Ok(())
    }

    /// Writes a scalar key.
    fn key(&mut self, atom: Atom, tag: Option<String>) -> Result<(), Error> {
        let mut rendered = String::new();
        let (scalar, implicit_tag) = self.render(&atom, true, &mut rendered)?;
        let tag = tag.as_deref().or(implicit_tag);
        let text = match scalar {
            Scalar::Text(text) => text,
            Scalar::Empty => "null".into(),
            Scalar::Literal(_) => unreachable!("keys are never block scalars"),
        };
        if text.len() > MAX_SIMPLE_KEY_LEN {
            self.begin_explicit_key();
            self.write_inline_start(tag);
            self.out.push_str(&text);
        } else {
            self.begin_entry();
            self.write_inline_start(tag);
            self.out.push_str(&text);
            self.out.push(':');
            self.space = true;
        }
        self.complete();
        Ok(())
    }

    /// Renders an atom as scalar.
    ///
    /// Returns the scalar and a tag that is implied by the value (such as
    /// `!!binary`).  Keys are always on a single line.
    fn render<'a>(
        &self,
        atom: &'a Atom,
        is_key: bool,
        buf: &mut String,
    ) -> Result<(Scalar<'a>, Option<&'static str>), Error> {
        buf.clear();
        Ok(match *atom {
            Atom::Null => match (self.config.null_style, is_key) {
                (NullStyle::Tilde, _) => (Scalar::Text("~".into()), None),
                (NullStyle::Empty, false) => (Scalar::Empty, None),
                _ => (Scalar::Text("null".into()), None),
            },
            Atom::Bool(value) => (
                Scalar::Text(if value { "true" } else { "false" }.into()),
                None,
            ),
            Atom::U64(value) => (Scalar::Text(value.to_string()), None),
            Atom::I64(value) => (Scalar::Text(value.to_string()), None),
            Atom::F64(value) => {
                write_float(buf, value);
                (Scalar::Text(std::mem::take(buf)), None)
            }
            Atom::Char(value) => (self.render_owned_str(value.to_string()), None),
            Atom::Str(ref value) => (self.render_str(value, is_key), None),
            Atom::Bytes(ref bytes) => {
                let format = if self.config.binary {
                    BytesFormat::BASE64
                } else {
                    bytes.fallback.copied().unwrap_or(self.config.bytes)
                };
                // sequences are handled before, keys are always strings
                let encoded = format
                    .encode(bytes)
                    .or_else(|| BytesFormat::BASE64.encode(bytes))
                    .unwrap_or_default();
                if self.config.binary {
                    let tag = Some("tag:yaml.org,2002:binary");
                    if encoded.is_empty() {
                        (Scalar::Text("\"\"".into()), tag)
                    } else if encoded.len() > BINARY_LINE_LEN && !is_key {
                        (Scalar::Literal(wrap_binary(&encoded)), tag)
                    } else {
                        (Scalar::Text(encoded), tag)
                    }
                } else {
                    (self.render_owned_str(encoded), None)
                }
            }
            Atom::Ext(ref ext) => return self.render_ext(ext, is_key, buf),
            _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        })
    }

    #[cold]
    fn render_ext<'a>(
        &self,
        ext: &'a ExtValue,
        is_key: bool,
        buf: &mut String,
    ) -> Result<(Scalar<'a>, Option<&'static str>), Error> {
        if let Some(value) = ext.downcast_ref::<u128>() {
            return Ok((Scalar::Text(value.to_string()), None));
        }
        if let Some(value) = ext.downcast_ref::<i128>() {
            return Ok((Scalar::Text(value.to_string()), None));
        }
        if let Some(value) = ext.downcast_ref::<BigInt>() {
            return Ok((Scalar::Text(value.to_string()), None));
        }
        // number literals keep their text if YAML reads it as number
        if let Some(value) = ext.downcast_value_ref::<Number>()
            && self.is_number(value.as_str())
        {
            return Ok((Scalar::Text(value.as_str().to_string()), None));
        }
        if let Some(value) = ext.downcast_ref::<Decimal>()
            && self.is_number(value.as_str())
        {
            return Ok((Scalar::Text(value.as_str().to_string()), None));
        }
        let datetime = ext.downcast_ref::<Datetime>().copied().or_else(|| {
            ext.downcast_ref::<Timestamp>()
                .and_then(|x| x.to_datetime())
        });
        if let Some(value) = datetime {
            return Ok(self.render_datetime(value));
        }
        match ext.fallback() {
            Atom::Ext(_) => Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("YAML does not support {}", ext.name()),
            )),
            // strings of fallbacks are always on a single line as they do
            // not outlive this call
            Atom::Str(value) => Ok((self.render_owned_str(value.into_owned()), None)),
            fallback => {
                let (scalar, tag) = self.render(&fallback, is_key, buf)?;
                Ok((
                    match scalar {
                        Scalar::Text(text) => Scalar::Text(text),
                        Scalar::Empty => Scalar::Empty,
                        Scalar::Literal(_) => unreachable!("only strings are block scalars"),
                    },
                    tag,
                ))
            }
        }
    }

    /// Renders a date / time.
    ///
    /// Timestamps (dates and date-times with offset) are written plain (or
    /// tagged), other values (local date-times and times) as strings as YAML
    /// has no representation for them.
    fn render_datetime<'a>(&self, value: Datetime) -> (Scalar<'a>, Option<&'static str>) {
        let text = value.to_string();
        let is_timestamp = value.date.is_some() && (value.time.is_none() || value.offset.is_some());
        if !is_timestamp {
            return (self.render_owned_str(text), None);
        }
        let tag = self
            .config
            .timestamp_tag
            .then_some("tag:yaml.org,2002:timestamp");
        (Scalar::Text(text), tag)
    }

    /// Returns `true` if text is read as number in all versions.
    fn is_number(&self, s: &str) -> bool {
        !is_plain_str(s, Version::V1_2)
            && (self.config.compat == Version::V1_2 || !is_plain_str(s, Version::V1_1))
            && s.parse::<f64>().is_ok()
    }

    /// Renders an owned string, always on a single line.
    fn render_owned_str<'a>(&self, value: String) -> Scalar<'a> {
        match self.render_str(&value, true) {
            Scalar::Text(text) => Scalar::Text(text),
            _ => unreachable!("single line strings are text"),
        }
    }

    /// Renders a string.
    fn render_str<'a>(&self, value: &'a str, single_line: bool) -> Scalar<'a> {
        if !single_line
            && self.config.multiline == MultilineStyle::Literal
            && !self.config.quote_all
            && let Some(literal) = Literal::new(value)
        {
            return Scalar::Literal(literal);
        }
        let mut out = String::with_capacity(value.len() + 2);
        if !self.config.quote_all && is_plain_safe(value, self.config.compat) {
            out.push_str(value);
        } else if self.config.quote_style == QuoteStyle::Single && is_single_quote_safe(value) {
            write_single_quoted(&mut out, value);
        } else {
            write_double_quoted(&mut out, value);
        }
        Scalar::Text(out)
    }
}
