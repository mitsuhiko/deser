//! Writes indented JSON and JSON with spaces after separators.
//!
//! The compact output (no indentation, no spaces) is written by the writer
//! in `ser` which is optimized for it.  This writer handles everything
//! else.  Maps and sequences are either written on multiple lines (every
//! entry on a line of its own) or on a single line.  Everything in a
//! container on a single line is on that line too.
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

pub(crate) struct PrettyWriter {
    ser: Output,
    indent: Indent,
    compact: bool,
    stack: Vec<Frame>,
    /// `true` if the next atom is a map key.
    is_key: bool,
}

impl PrettyWriter {
    pub fn new(ser: Output, indent: Indent, compact: bool) -> PrettyWriter {
        PrettyWriter {
            ser,
            indent,
            compact,
            stack: Vec::new(),
            is_key: false,
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
        Ok(())
    }

    fn start(&mut self, is_map: bool, layout: Layout) -> Result<(), Error> {
        if self.is_key {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "JSON does not support this value for map keys",
            ));
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
        self.ser.write_char(if is_map { '{' } else { '[' });
        self.stack.push(Frame {
            is_map,
            multiline: parent_multiline && self.indent != Indent::None && layout != Layout::Compact,
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
        let frame = self.stack.pop().unwrap();
        if frame.multiline && !frame.first {
            self.newline();
        }
        self.ser.write_char(if is_map { '}' } else { ']' });
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
