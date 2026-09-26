//! Writes indented JSON and JSON with spaces after separators.
//!
//! The compact output (no indentation, no spaces) is written by the writer
//! in `ser` which is optimized for it.  This writer handles everything
//! else.  Maps and sequences are either written on multiple lines (every
//! entry on a line of its own) or on a single line.  Everything in a
//! container on a single line is on that line too.
//!
//! For the inline policy maps and sequences are written on a single line
//! tentatively.  If they turn out to contain a map or sequence or not to
//! fit, the text written so far is laid out on multiple lines.  The text of
//! the entries is the same either way, only the separators between them
//! change.  So instead of recording events, only the offsets of the
//! entries are recorded.
use deser::hints::Layout;
use deser::{Atom, Error, ErrorKind, Event, State};

use crate::ser::{Indent, Output};

/// An open map or sequence.
struct Frame {
    is_map: bool,
    /// `true` if every entry is on a line of its own.
    multiline: bool,
    /// `true` until the first entry was written.
    first: bool,
}

/// A map or sequence that is tentatively written on a single line.
///
/// It's always the innermost container.
struct Attempt {
    /// The offset of the opening bracket.
    start: usize,
    /// The column at `checked`.
    column: usize,
    checked: usize,
}

pub(crate) struct PrettyWriter {
    ser: Output,
    indent: Indent,
    compact: bool,
    inline_width: Option<usize>,
    stack: Vec<Frame>,
    /// `true` if the next atom is a map key.
    is_key: bool,
    /// The offset of the start of the current line.
    line_start: usize,
    attempt: Option<Attempt>,
    /// The offsets of the entries of the attempt (after their separators).
    entries: Vec<usize>,
    /// A buffer for the text of aborted attempts.
    scratch: String,
}

impl PrettyWriter {
    pub fn new(
        ser: Output,
        indent: Indent,
        compact: bool,
        inline_width: Option<usize>,
    ) -> PrettyWriter {
        PrettyWriter {
            ser,
            indent,
            compact,
            inline_width,
            stack: Vec::new(),
            is_key: false,
            line_start: 0,
            attempt: None,
            entries: Vec::new(),
            scratch: String::new(),
        }
    }

    pub fn finish(self) -> String {
        self.ser.out.into_string()
    }

    pub fn event(&mut self, event: Event, state: &State) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => self.atom(atom),
            Event::MapStart(_) => self.start(true, Layout::of(state)),
            Event::SeqStart(_) => self.start(false, Layout::of(state)),
            Event::MapEnd => self.end(true),
            Event::SeqEnd => self.end(false),
        }
    }

    fn atom(&mut self, atom: Atom) -> Result<(), Error> {
        if self.is_key {
            self.begin_entry();
            self.ser.write_key_text(atom)?;
            self.ser.write_str(if self.compact { ":" } else { ": " });
            self.is_key = false;
        } else {
            // map values follow their key, everything else is an entry
            if self.stack.last().is_some_and(|frame| !frame.is_map) {
                self.begin_entry();
            }
            self.ser.write_atom(atom)?;
            self.complete();
        }
        if let Some(width) = self.inline_width
            && self.attempt.is_some()
            && self.column() > width
        {
            self.abort_attempt();
        }
        Ok(())
    }

    fn start(&mut self, is_map: bool, layout: Layout) -> Result<(), Error> {
        if self.is_key {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "JSON does not support this value for map keys",
            ));
        }
        // only maps and sequences of scalars are written on a single line
        if self.attempt.is_some() {
            self.abort_attempt();
        }
        let parent_multiline = match self.stack.last() {
            Some(frame) => {
                let multiline = frame.multiline;
                if !frame.is_map {
                    self.begin_entry();
                }
                multiline
            }
            None => true,
        };
        let start = self.ser.out.len();
        self.ser.write_char(if is_map { '{' } else { '[' });
        let mut multiline =
            parent_multiline && self.indent != Indent::None && layout != Layout::Compact;
        if multiline && layout == Layout::Auto && self.inline_width.is_some() {
            multiline = false;
            self.entries.clear();
            self.attempt = Some(Attempt {
                start,
                column: self.ser.out.as_str()[self.line_start..start]
                    .chars()
                    .count(),
                checked: start,
            });
        }
        self.stack.push(Frame {
            is_map,
            multiline,
            first: true,
        });
        self.is_key = is_map;
        Ok(())
    }

    fn end(&mut self, is_map: bool) -> Result<(), Error> {
        match self.stack.last() {
            Some(frame) if frame.is_map == is_map && (!is_map || self.is_key) => {}
            _ if is_map => return Err(Error::new(ErrorKind::Unexpected, "unexpected map end")),
            _ => return Err(Error::new(ErrorKind::Unexpected, "unexpected array end")),
        }
        // the closing bracket has to fit too
        if let Some(width) = self.inline_width
            && self.attempt.is_some()
            && self.column() + 1 > width
        {
            self.abort_attempt();
        }
        let frame = self.stack.pop().unwrap();
        if frame.multiline && !frame.first {
            self.newline();
        }
        self.ser.write_char(if is_map { '}' } else { ']' });
        // the attempt is always the innermost container, it fits
        self.attempt = None;
        self.complete();
        Ok(())
    }

    /// Writes the separator before an entry (a sequence item or a map key).
    fn begin_entry(&mut self) {
        let frame = self.stack.last_mut().unwrap();
        let first = std::mem::replace(&mut frame.first, false);
        if !first {
            self.ser.write_char(',');
        }
        if frame.multiline {
            self.newline();
        } else if !first && !self.compact {
            self.ser.write_char(' ');
        }
        if self.attempt.is_some() {
            self.entries.push(self.ser.out.len());
        }
    }

    /// Returns the current column of the attempt.
    fn column(&mut self) -> usize {
        let attempt = self.attempt.as_mut().unwrap();
        // strings never contain line breaks, the attempt is on one line
        attempt.column += self.ser.out.as_str()[attempt.checked..].chars().count();
        attempt.checked = self.ser.out.len();
        attempt.column
    }

    /// Lays out the container of the attempt on multiple lines.
    #[cold]
    fn abort_attempt(&mut self) {
        let attempt = self.attempt.take().unwrap();
        let mut text = std::mem::take(&mut self.scratch);
        text.clear();
        text.push_str(&self.ser.out.as_str()[attempt.start..]);
        self.ser.out.truncate(attempt.start);
        self.stack.last_mut().unwrap().multiline = true;
        // the bracket
        self.ser.write_str(&text[..1]);
        let separator = if self.compact { 1 } else { 2 };
        let entries = std::mem::take(&mut self.entries);
        for (idx, &entry) in entries.iter().enumerate() {
            if idx > 0 {
                self.ser.write_char(',');
            }
            self.newline();
            let end = entries
                .get(idx + 1)
                .map_or(text.len(), |next| next - attempt.start - separator);
            self.ser.write_str(&text[entry - attempt.start..end]);
        }
        self.entries = entries;
        self.scratch = text;
    }

    /// Records that a value was completed.
    fn complete(&mut self) {
        // after a value in a map, a key follows
        self.is_key = self.stack.last().is_some_and(|frame| frame.is_map);
    }

    /// Starts a new line indented for the depth of the current container.
    fn newline(&mut self) {
        const SPACES: &str = "                                ";
        const TABS: &str = "\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t";
        self.ser.write_char('\n');
        self.line_start = self.ser.out.len();
        let (chunk, mut len) = match self.indent {
            Indent::Spaces(width) => (SPACES, width * self.stack.len()),
            Indent::Tab => (TABS, self.stack.len()),
            Indent::None => return,
        };
        while len > 0 {
            let n = len.min(chunk.len());
            self.ser.write_str(&chunk[..n]);
            len -= n;
        }
    }
}
