//! Writes serialization events as YAML.
//!
//! Collections are held back until their first item is known so that empty
//! collections can be written as `{}` and `[]` and the layout never needs to
//! be patched afterwards.  Collections are written in block style unless
//! they are compact (see [`Layout`]), the flow policy allows them to be
//! written in flow style or there is no indentation ([`Indent::None`]).
//! For the flow policy the events of a collection are recorded while it's
//! written in flow style.  If it turns out not to fit, the output is rolled
//! back and the recorded events are written in block style.
use std::borrow::Cow;
use std::fmt::{self, Write};

use deser::adapters::bytes::BytesFormat;
use deser::ext::{BigInt, Datetime, Decimal, ExtValue, Number, Timestamp};
use deser::hints::Layout;
use deser::{Atom, Error, ErrorKind, Event, State};

use crate::quote::{
    BlockScalar, MAX_SIMPLE_KEY_LEN, is_plain_safe, is_single_quote_safe, push_indent,
    write_double_quoted, write_float, write_single_quoted, write_tag,
};
use crate::resolve::{Version, is_plain_str};
use crate::ser::{FlowPolicy, Indent, MultilineStyle, NullStyle, QuoteStyle, SerializerConfig};
use crate::style::{ScalarStyle, StyleHint};
use crate::tag::NodeTag;

/// The length of the lines of long `!!binary` values.
const BINARY_LINE_LEN: usize = 76;

/// The width folded scalars use if no width is configured.
const DEFAULT_FOLD_WIDTH: usize = 80;

/// Wraps base64 into a literal block of lines.
fn wrap_binary(encoded: &str) -> BlockScalar<'static> {
    let mut lines = String::with_capacity(encoded.len() + encoded.len() / BINARY_LINE_LEN + 1);
    for (idx, chunk) in encoded.as_bytes().chunks(BINARY_LINE_LEN).enumerate() {
        if idx > 0 {
            lines.push('\n');
        }
        // base64 is ASCII
        lines.push_str(std::str::from_utf8(chunk).unwrap());
    }
    BlockScalar::from_lines(lines)
}

/// The hints of an event.
#[derive(Debug, Clone, Default)]
struct Hints {
    tag: Option<String>,
    layout: Layout,
    style: Option<ScalarStyle>,
}

/// Where the next node is written.
#[derive(Debug, Clone, Copy)]
enum Pos {
    /// The root of the document.
    Root,
    /// After an indicator (`-`, `?` or the `:` of an explicit key), the
    /// content of the node starts at the given column.
    Inline(usize),
    /// The value of a simple key of a block mapping at the given column.
    Value(usize),
    /// Inside a flow collection.
    Flow,
}

/// Where a scalar is written, which restricts its styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// Block scalars are allowed.
    Block,
    /// A key: on a single line.
    Key,
    /// In a flow collection: on a single line, no flow indicators.
    Flow,
}

/// An open collection.
struct Frame {
    is_map: bool,
    flow: bool,
    /// The column of the keys or dashes (block collections).
    indent: usize,
    /// For block collections `true` if the next entry continues the current
    /// line (the first entry of a collection that follows an indicator or
    /// starts the document).  For flow collections `true` for the first
    /// entry.
    first: bool,
    /// For maps: `true` if the next node is a value.
    value: bool,
    /// For maps: `true` if the key of the current entry is explicit (`?`).
    explicit: bool,
}

/// A collection whose first event was seen but that was not written yet.
#[derive(Clone)]
struct Pending {
    is_map: bool,
    hints: Hints,
    pos: Pos,
}

/// A collection that is tentatively written in flow style.
struct Attempt {
    /// The collection, to write it in block style instead.
    pending: Pending,
    /// The state before the collection was written.
    out_len: usize,
    depth: usize,
    space: bool,
    line_done: bool,
    /// The events of the collection after its start.
    events: Vec<(Event<'static>, Hints)>,
    /// The column the collection has to end before.
    width: usize,
}

/// A scalar that was rendered for a position.
enum Scalar<'a> {
    /// Text that is written as is.
    Text(Cow<'a, str>),
    /// Short text that is written as is (numbers), without allocating.
    Short(ShortText),
    /// A block scalar.
    Block(BlockScalar<'a>),
    /// Nothing (a null with [`NullStyle::Empty`]).
    Empty,
}

/// A short text held inline.
struct ShortText {
    buf: [u8; 48],
    len: usize,
}

impl ShortText {
    fn new() -> ShortText {
        ShortText {
            buf: [0; 48],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        // only complete strings are written into the buffer
        std::str::from_utf8(&self.buf[..self.len]).unwrap()
    }
}

impl fmt::Write for ShortText {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        self.buf
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

impl<'a> Scalar<'a> {
    /// Returns the text of a scalar that is written inline.
    fn text(&self) -> Option<&str> {
        match self {
            Scalar::Text(text) => Some(text),
            Scalar::Short(text) => Some(text.as_str()),
            _ => None,
        }
    }

    /// Renders a number (or other short text) with its `Display`.
    fn short(value: impl fmt::Display) -> Scalar<'a> {
        let mut text = ShortText::new();
        write!(text, "{}", value).expect("short text");
        Scalar::Short(text)
    }
}

pub(crate) struct Emitter<'c> {
    config: &'c SerializerConfig,
    out: String,
    stack: Vec<Frame>,
    pending: Option<Pending>,
    attempt: Option<Attempt>,
    /// `true` if a space has to be written before the next inline content
    /// (after an indicator).
    space: bool,
    /// `true` once the root value was written.
    done: bool,
    /// `true` if the current line was already terminated (after block
    /// scalars).
    line_done: bool,
    /// The column at `column_offset` in the output.  The column is only
    /// needed for the flow policy, it's updated from the output written
    /// since it was last computed.
    column: usize,
    column_offset: usize,
    /// The buffer for the events of the next attempt (reused to not
    /// allocate for every attempt).
    spare_events: Vec<(Event<'static>, Hints)>,
}

impl<'c> Emitter<'c> {
    pub fn new(config: &'c SerializerConfig, out: String) -> Emitter<'c> {
        Emitter {
            config,
            out,
            stack: Vec::new(),
            pending: None,
            attempt: None,
            space: false,
            done: false,
            line_done: false,
            column: 0,
            column_offset: 0,
            spare_events: Vec::new(),
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
        let hints = if state.has_event_data() {
            Hints {
                tag: state.event::<NodeTag>().and_then(|x| x.0.clone()),
                layout: Layout::of(state),
                style: state.event::<StyleHint>().and_then(|x| x.0),
            }
        } else {
            Hints::default()
        };
        self.emit(event, hints)
    }

    fn emit(&mut self, event: Event, hints: Hints) -> Result<(), Error> {
        if let Some(ref mut attempt) = self.attempt {
            attempt.events.push((event.to_static(), hints.clone()));
            // only collections of scalars are written in flow style
            if matches!(event, Event::MapStart(_) | Event::SeqStart(_)) {
                return self.abort_attempt();
            }
        }
        if let Some(pending) = self.pending.take() {
            match event {
                Event::MapEnd if pending.is_map => return self.write_empty(pending),
                Event::SeqEnd if !pending.is_map => return self.write_empty(pending),
                ref event => {
                    let is_start = matches!(event, Event::MapStart(_) | Event::SeqStart(_));
                    if !is_start && self.may_attempt(&pending) {
                        self.start_attempt(pending, event, &hints);
                    } else {
                        self.open(pending);
                    }
                }
            }
        }
        match event {
            Event::Atom(atom) => self.atom(atom, hints)?,
            Event::MapStart(_) => self.start(true, hints)?,
            Event::SeqStart(_) => self.start(false, hints)?,
            Event::MapEnd => self.end(true)?,
            Event::SeqEnd => self.end(false)?,
        }
        if let Some(ref attempt) = self.attempt {
            let width = attempt.width;
            if self.stack.len() <= attempt.depth {
                // the collection ended and fits
                let mut events = self.attempt.take().unwrap().events;
                events.clear();
                self.spare_events = events;
            } else if self.column() > width {
                return self.abort_attempt();
            }
        }
        Ok(())
    }

    /// Returns `true` if the collection may be written in flow style if it
    /// fits.
    fn may_attempt(&self, pending: &Pending) -> bool {
        matches!(self.config.flow, FlowPolicy::LeafIfFits(_))
            && self.config.indent != Indent::None
            && pending.hints.layout == Layout::Auto
            && !matches!(pending.pos, Pos::Flow)
    }

    /// Starts writing a collection in flow style tentatively.
    fn start_attempt(&mut self, pending: Pending, event: &Event, hints: &Hints) {
        let FlowPolicy::LeafIfFits(width) = self.config.flow else {
            unreachable!();
        };
        self.attempt = Some(Attempt {
            pending: pending.clone(),
            out_len: self.out.len(),
            depth: self.stack.len(),
            space: self.space,
            line_done: self.line_done,
            events: {
                let mut events = std::mem::take(&mut self.spare_events);
                events.push((event.to_static(), hints.clone()));
                events
            },
            width,
        });
        self.open_flow(pending);
    }

    /// Rolls back a collection written in flow style and writes it in block
    /// style.
    #[cold]
    fn abort_attempt(&mut self) -> Result<(), Error> {
        let attempt = self.attempt.take().unwrap();
        self.out.truncate(attempt.out_len);
        if self.column_offset > attempt.out_len {
            // the column was computed for output that is gone, count the
            // line again
            self.column = 0;
            self.column_offset = self.out.rfind('\n').map_or(0, |x| x + 1);
        }
        self.stack.truncate(attempt.depth);
        self.space = attempt.space;
        self.line_done = attempt.line_done;
        self.pending = None;
        self.open_block(attempt.pending);
        let mut events = attempt.events;
        for (event, hints) in events.drain(..) {
            self.emit(event, hints)?;
        }
        if events.capacity() > self.spare_events.capacity() {
            self.spare_events = events;
        }
        Ok(())
    }

    /// Returns the current column.
    fn column(&mut self) -> usize {
        let new = &self.out[self.column_offset..];
        match new.rfind('\n') {
            Some(idx) => self.column = new[idx + 1..].chars().count(),
            None => self.column += new.chars().count(),
        }
        self.column_offset = self.out.len();
        self.column
    }

    /// Returns `true` if the current node is in a flow collection.
    fn in_flow(&self) -> bool {
        self.stack.last().is_some_and(|x| x.flow)
    }

    /// Starts a map or sequence, it's written once the next event is known.
    fn start(&mut self, is_map: bool, hints: Hints) -> Result<(), Error> {
        let pos = match self.begin_node()? {
            Some(pos) => pos,
            // collections as keys are explicit
            None => self.begin_explicit_key(),
        };
        self.pending = Some(Pending { is_map, hints, pos });
        Ok(())
    }

    fn end(&mut self, is_map: bool) -> Result<(), Error> {
        match self.stack.pop() {
            Some(frame) if frame.is_map == is_map && (!is_map || !frame.value) => {
                if frame.flow {
                    self.out.push(if is_map { '}' } else { ']' });
                }
            }
            _ => return Err(Error::new(ErrorKind::Unexpected, "unexpected end event")),
        }
        self.complete();
        Ok(())
    }

    /// Writes an empty collection.
    fn write_empty(&mut self, pending: Pending) -> Result<(), Error> {
        self.write_inline_start(pending.hints.tag.as_deref());
        self.out.push_str(if pending.is_map { "{}" } else { "[]" });
        self.complete();
        Ok(())
    }

    /// Writes the start of a non-empty collection.
    fn open(&mut self, pending: Pending) {
        if matches!(pending.pos, Pos::Flow)
            || pending.hints.layout == Layout::Compact
            || self.config.indent == Indent::None
        {
            self.open_flow(pending);
        } else {
            self.open_block(pending);
        }
    }

    fn open_flow(&mut self, pending: Pending) {
        self.write_inline_start(pending.hints.tag.as_deref());
        self.out.push(if pending.is_map { '{' } else { '[' });
        self.stack.push(Frame {
            is_map: pending.is_map,
            flow: true,
            indent: 0,
            first: true,
            value: false,
            explicit: false,
        });
    }

    fn open_block(&mut self, pending: Pending) {
        let (indent, first) = match pending.pos {
            Pos::Root => {
                if let Some(ref tag) = pending.hints.tag {
                    write_tag(&mut self.out, tag);
                    self.out.push('\n');
                }
                (0, true)
            }
            Pos::Inline(column) => match pending.hints.tag {
                Some(ref tag) => {
                    self.flush_space();
                    write_tag(&mut self.out, tag);
                    (column, false)
                }
                None => (column, true),
            },
            Pos::Value(key_column) => {
                if let Some(ref tag) = pending.hints.tag {
                    self.flush_space();
                    write_tag(&mut self.out, tag);
                }
                self.space = false;
                let indent = if pending.is_map || self.config.indent_sequences {
                    key_column + self.config.indent_width()
                } else {
                    key_column
                };
                (indent, false)
            }
            Pos::Flow => unreachable!("collections in flow collections are flow"),
        };
        self.stack.push(Frame {
            is_map: pending.is_map,
            flow: false,
            indent,
            first,
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
        if frame.flow {
            if frame.is_map && frame.value {
                if frame.explicit {
                    self.out.push(':');
                    self.space = true;
                }
                return Ok(Some(Pos::Flow));
            }
            if !std::mem::replace(&mut frame.first, false) {
                self.out.push(',');
                self.space = true;
            }
            return Ok(if frame.is_map { None } else { Some(Pos::Flow) });
        }
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

    /// Moves to the start of the next entry of the current block collection.
    fn begin_entry(&mut self) {
        let frame = self.stack.last_mut().unwrap();
        if frame.flow {
            // the separator was written by `begin_node`
            self.flush_space();
            return;
        }
        let indent = frame.indent;
        if std::mem::replace(&mut frame.first, false) {
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
        let pos = if frame.flow {
            Pos::Flow
        } else {
            Pos::Inline(frame.indent + 2)
        };
        self.out.push('?');
        self.space = true;
        pos
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

    fn atom(&mut self, atom: Atom, hints: Hints) -> Result<(), Error> {
        // bytes that are sequences are written as sequences of integers
        if let Atom::Bytes(ref bytes) = atom
            && !self.config.binary
            && bytes.fallback.copied().unwrap_or(self.config.bytes) == BytesFormat::SEQ
        {
            self.emit(
                Event::seq_start(),
                Hints {
                    layout: hints.layout,
                    tag: hints.tag,
                    style: None,
                },
            )?;
            for &byte in bytes.iter() {
                self.emit(Event::Atom(Atom::U64(byte.into())), Hints::default())?;
            }
            return self.emit(Event::SeqEnd, Hints::default());
        }

        let pos = match self.begin_node()? {
            Some(pos) => pos,
            None => return self.key(atom, hints),
        };
        let context = match pos {
            Pos::Flow => Context::Flow,
            // without indentation a scalar document is on a single line
            _ if self.config.indent == Indent::None => Context::Key,
            _ => Context::Block,
        };
        let (scalar, implicit_tag) = self.render(&atom, context, hints.style)?;
        let tag = hints.tag.as_deref().or(implicit_tag);
        match scalar {
            // nothing to write, the indicator or key stands on its own
            Scalar::Empty if !matches!(pos, Pos::Root) && tag.is_none() => {
                self.space = false;
            }
            Scalar::Empty => {
                self.write_inline_start(tag);
                self.out.push_str("null");
            }
            Scalar::Text(_) | Scalar::Short(_) => {
                self.write_inline_start(tag);
                self.out.push_str(scalar.text().unwrap());
            }
            Scalar::Block(block) => {
                self.write_inline_start(tag);
                // the indentation indicator is relative to the parent node
                let (column, parent) = match pos {
                    // the document is at indentation 0 for indicators
                    Pos::Root => (self.config.indent_width(), 0),
                    Pos::Inline(column) => (column, column - 2),
                    Pos::Value(key_column) => (key_column + self.config.indent_width(), key_column),
                    Pos::Flow => unreachable!("no block scalars in flow collections"),
                };
                block.write_header(&mut self.out, column - parent);
                self.out.push('\n');
                block.write_body(&mut self.out, column);
                self.line_done = true;
            }
        }
        self.complete();
        Ok(())
    }

    /// Writes a scalar key.
    fn key(&mut self, atom: Atom, hints: Hints) -> Result<(), Error> {
        let context = if self.in_flow() {
            Context::Flow
        } else {
            Context::Key
        };
        let (scalar, implicit_tag) = self.render(&atom, context, hints.style)?;
        let tag = hints.tag.as_deref().or(implicit_tag);
        let text = match scalar {
            Scalar::Empty => "null",
            Scalar::Block(_) => unreachable!("keys are never block scalars"),
            ref scalar => scalar.text().unwrap(),
        };
        if text.len() > MAX_SIMPLE_KEY_LEN {
            self.begin_explicit_key();
            self.write_inline_start(tag);
            self.out.push_str(text);
        } else {
            self.begin_entry();
            self.write_inline_start(tag);
            self.out.push_str(text);
            self.out.push(':');
            self.space = true;
        }
        self.complete();
        Ok(())
    }

    /// Renders an atom as scalar.
    ///
    /// Returns the scalar and a tag that is implied by the value (such as
    /// `!!binary`).
    fn render<'a>(
        &self,
        atom: &'a Atom,
        context: Context,
        style: Option<ScalarStyle>,
    ) -> Result<(Scalar<'a>, Option<&'static str>), Error> {
        Ok(match *atom {
            Atom::Null => match (self.config.null_style, context) {
                (NullStyle::Tilde, _) => (Scalar::Text("~".into()), None),
                (NullStyle::Empty, Context::Block) => (Scalar::Empty, None),
                _ => (Scalar::Text("null".into()), None),
            },
            Atom::Bool(value) => (
                Scalar::Text(if value { "true" } else { "false" }.into()),
                None,
            ),
            Atom::U64(value) => (Scalar::short(value), None),
            Atom::I64(value) => (Scalar::short(value), None),
            Atom::F64(value) => {
                let mut text = ShortText::new();
                write_float(&mut text, value);
                (Scalar::Short(text), None)
            }
            Atom::F32(value) => {
                let mut text = ShortText::new();
                write_float(&mut text, value);
                (Scalar::Short(text), None)
            }
            Atom::Char(value) => (
                self.render_owned_str(value.to_string(), context, style),
                None,
            ),
            Atom::Str(ref value) => (self.render_str(value, context, style), None),
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
                    } else if encoded.len() > BINARY_LINE_LEN && context == Context::Block {
                        (Scalar::Block(wrap_binary(&encoded)), tag)
                    } else {
                        (Scalar::Text(encoded.into()), tag)
                    }
                } else {
                    (self.render_owned_str(encoded, context, style), None)
                }
            }
            Atom::Ext(ref ext) => return self.render_ext(ext, context, style),
            _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        })
    }

    #[cold]
    fn render_ext<'a>(
        &self,
        ext: &'a ExtValue,
        context: Context,
        style: Option<ScalarStyle>,
    ) -> Result<(Scalar<'a>, Option<&'static str>), Error> {
        if let Some(value) = ext.downcast_ref::<u128>() {
            return Ok((Scalar::short(value), None));
        }
        if let Some(value) = ext.downcast_ref::<i128>() {
            return Ok((Scalar::short(value), None));
        }
        if let Some(value) = ext.downcast_ref::<BigInt>() {
            return Ok((Scalar::Text(value.to_string().into()), None));
        }
        // number literals keep their text if YAML reads it as number
        if let Some(value) = ext.downcast_value_ref::<Number>()
            && self.is_number(value.as_str())
        {
            return Ok((Scalar::Text(value.as_str().into()), None));
        }
        if let Some(value) = ext.downcast_ref::<Decimal>()
            && self.is_number(value.as_str())
        {
            return Ok((Scalar::Text(value.as_str().into()), None));
        }
        let datetime = ext.downcast_ref::<Datetime>().copied().or_else(|| {
            ext.downcast_ref::<Timestamp>()
                .and_then(|x| x.to_datetime())
        });
        if let Some(value) = datetime {
            return Ok(self.render_datetime(value, context, style));
        }
        match ext.fallback() {
            Atom::Ext(_) => Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("YAML does not support {}", ext.name()),
            )),
            // strings of fallbacks are always on a single line as they do
            // not outlive this call
            Atom::Str(value) => Ok((
                self.render_owned_str(value.into_owned(), context, style),
                None,
            )),
            fallback => {
                let (scalar, tag) = self.render(&fallback, context, style)?;
                Ok((
                    match scalar {
                        Scalar::Text(text) => Scalar::Text(Cow::Owned(text.into_owned())),
                        Scalar::Short(text) => Scalar::Short(text),
                        Scalar::Empty => Scalar::Empty,
                        Scalar::Block(_) => unreachable!("only strings are block scalars"),
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
    fn render_datetime<'a>(
        &self,
        value: Datetime,
        context: Context,
        style: Option<ScalarStyle>,
    ) -> (Scalar<'a>, Option<&'static str>) {
        let text = value.to_string();
        let is_timestamp = value.date.is_some() && (value.time.is_none() || value.offset.is_some());
        if !is_timestamp {
            return (self.render_owned_str(text, context, style), None);
        }
        let tag = self
            .config
            .timestamp_tag
            .then_some("tag:yaml.org,2002:timestamp");
        (Scalar::Text(text.into()), tag)
    }

    /// Returns `true` if text is read as number in all versions.
    fn is_number(&self, s: &str) -> bool {
        !is_plain_str(s, Version::V1_2)
            && (self.config.compat == Version::V1_2 || !is_plain_str(s, Version::V1_1))
            && s.parse::<f64>().is_ok()
    }

    /// Renders an owned string, never as block scalar.
    fn render_owned_str<'a>(
        &self,
        value: String,
        context: Context,
        style: Option<ScalarStyle>,
    ) -> Scalar<'a> {
        let context = match context {
            Context::Block => Context::Key,
            other => other,
        };
        match self.render_str(&value, context, style) {
            Scalar::Text(text) => Scalar::Text(Cow::Owned(text.into_owned())),
            _ => unreachable!("strings outside of blocks are text"),
        }
    }

    /// Renders a string.
    ///
    /// The requested style is used if the string can be written in it,
    /// otherwise the style that the configuration asks for (if possible).
    fn render_str<'a>(
        &self,
        value: &'a str,
        context: Context,
        style: Option<ScalarStyle>,
    ) -> Scalar<'a> {
        let block = context == Context::Block;
        let flow = context == Context::Flow;
        let fold_width = self.config.fold_width.unwrap_or(DEFAULT_FOLD_WIDTH);
        match style {
            Some(ScalarStyle::Plain) if is_plain_safe(value, self.config.compat, flow) => {
                return Scalar::Text(value.into());
            }
            Some(ScalarStyle::SingleQuoted) if is_single_quote_safe(value) => {
                let mut out = String::with_capacity(value.len() + 2);
                write_single_quoted(&mut out, value);
                return Scalar::Text(out.into());
            }
            Some(ScalarStyle::DoubleQuoted) => {
                let mut out = String::with_capacity(value.len() + 2);
                write_double_quoted(&mut out, value);
                return Scalar::Text(out.into());
            }
            Some(ScalarStyle::Literal) if block => {
                if let Some(block) = BlockScalar::literal(value) {
                    return Scalar::Block(block);
                }
            }
            Some(ScalarStyle::Folded) if block => {
                if let Some(block) =
                    BlockScalar::folded(value, fold_width).or_else(|| BlockScalar::literal(value))
                {
                    return Scalar::Block(block);
                }
            }
            _ => {}
        }

        if block && !self.config.quote_all {
            if value.contains('\n') {
                if self.config.multiline == MultilineStyle::Literal
                    && let Some(block) = BlockScalar::literal(value)
                {
                    return Scalar::Block(block);
                }
            } else if let Some(width) = self.config.fold_width
                && BlockScalar::should_fold(value, width)
                && let Some(block) = BlockScalar::folded(value, width)
            {
                return Scalar::Block(block);
            }
        }
        if !self.config.quote_all && is_plain_safe(value, self.config.compat, flow) {
            return Scalar::Text(Cow::Borrowed(value));
        }
        let mut out = String::with_capacity(value.len() + 2);
        if self.config.quote_style == QuoteStyle::Single && is_single_quote_safe(value) {
            write_single_quoted(&mut out, value);
        } else {
            write_double_quoted(&mut out, value);
        }
        Scalar::Text(out.into())
    }
}
