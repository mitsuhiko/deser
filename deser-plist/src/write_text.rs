//! The writer of the text property lists (XML and OpenStep).
//!
//! Unlike binary property lists, which need all objects to write the
//! object table, text property lists are written while the events arrive.
//! Only an event is held back: the start of an array or dictionary until
//! it's known if it's empty (empty containers are written differently)
//! and the key of a dictionary entry until its value is known not to be
//! null (entries with null values are left out).
use alloc::string::String;
use alloc::vec::Vec;

use deser_core::ser::EventSink;
use deser_core::{Error, ErrorKind, Event, Serialize, State};

use crate::format::Format;
use crate::ser::{Node, convert_atom, key_to_string, unsupported_key};
use crate::{write_ascii, write_xml};

/// An open container.
enum Open {
    Array,
    /// A dictionary with the key of the value that comes next.
    Dict(Option<String>),
}

/// What an event starts in a container.
enum Item {
    Null,
    Node(Node),
    /// The start of an array (`false`) or dictionary (`true`).
    Start(bool),
}

/// Writes a value as XML or OpenStep property list.
pub(crate) struct TextWriter {
    xml: bool,
    pub(crate) out: String,
    stack: Vec<Open>,
    /// A container whose start event was seen but that was not written,
    /// `true` for dictionaries.
    pending: Option<bool>,
    /// `true` once the value was written.
    done: bool,
    /// The driver is paused once the output is this long (see
    /// `EventSink`).
    pub(crate) limit: usize,
}

impl EventSink for TextWriter {
    fn event(
        &mut self,
        event: Event<'_>,
        _value: &dyn Serialize,
        _state: &mut State,
    ) -> Result<(), Error> {
        TextWriter::event(self, event)
    }

    fn pause(&mut self) -> bool {
        // what is held back is not in the output, it's always final
        self.out.len() >= self.limit
    }
}

impl TextWriter {
    /// Creates a writer for a text format which appends to the output.
    pub(crate) fn new(format: Format, mut out: String) -> TextWriter {
        let xml = match format {
            Format::Xml => true,
            Format::Ascii => false,
            Format::Binary => unreachable!("binary property lists are not text"),
        };
        if xml {
            out.push_str(write_xml::HEADER);
        }
        TextWriter {
            xml,
            out,
            stack: Vec::new(),
            pending: None,
            done: false,
            limit: usize::MAX,
        }
    }

    /// Ends the property list once the value was written.
    pub(crate) fn finish(&mut self) -> Result<(), Error> {
        if !self.done || !self.stack.is_empty() || self.pending.is_some() {
            return Err(Error::new(ErrorKind::Unexpected, "incomplete value"));
        }
        if self.xml {
            self.out.push_str(write_xml::FOOTER);
        }
        Ok(())
    }

    fn event(&mut self, event: Event<'_>) -> Result<(), Error> {
        if let Some(is_map) = self.pending.take() {
            match event {
                Event::MapEnd if is_map => return self.write_empty(true),
                Event::SeqEnd if !is_map => return self.write_empty(false),
                _ => self.open(is_map),
            }
        }
        match self.stack.last_mut() {
            None => {
                if self.done {
                    return Err(Error::new(ErrorKind::Unexpected, "unexpected event"));
                }
                match item(event)? {
                    Item::Null => Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "property lists cannot hold null values",
                    )),
                    item => self.write_item(item),
                }
            }
            Some(Open::Dict(key @ None)) => match event {
                Event::Atom(atom) => {
                    *key = Some(key_to_string(atom)?);
                    Ok(())
                }
                Event::MapEnd => self.close(),
                _ => Err(unsupported_key()),
            },
            Some(Open::Dict(key @ Some(_))) => {
                let key = key.take().unwrap();
                match item(event)? {
                    // map entries with null values are skipped
                    Item::Null => Ok(()),
                    item => {
                        self.write_key(&key);
                        self.write_item(item)
                    }
                }
            }
            Some(Open::Array) => {
                if event == Event::SeqEnd {
                    return self.close();
                }
                match item(event)? {
                    Item::Null => Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "property lists cannot hold null values in arrays",
                    )),
                    item => self.write_item(item),
                }
            }
        }
    }

    /// Writes the key of a dictionary entry.
    fn write_key(&mut self, key: &str) {
        let depth = self.stack.len();
        indent(&mut self.out, depth);
        if self.xml {
            self.out.push_str("<key>");
            write_xml::escape(&mut self.out, key);
            self.out.push_str("</key>\n");
        } else {
            write_ascii::write_str(&mut self.out, key);
            self.out.push_str(" = ");
        }
    }

    /// Writes a value (or holds back the start of a container).
    fn write_item(&mut self, item: Item) -> Result<(), Error> {
        let depth = self.stack.len();
        // values in dictionaries follow their key in OpenStep
        if self.xml || matches!(self.stack.last(), Some(Open::Array)) {
            indent(&mut self.out, depth);
        }
        match item {
            Item::Node(node) => {
                if self.xml {
                    write_xml::scalar(&mut self.out, &node, depth)?;
                } else {
                    write_ascii::scalar(&mut self.out, &node);
                }
                self.complete();
            }
            Item::Start(is_map) => self.pending = Some(is_map),
            Item::Null => unreachable!("nulls are handled by the caller"),
        }
        Ok(())
    }

    /// Writes the start of a container that is not empty.
    fn open(&mut self, is_map: bool) {
        self.out.push_str(match (self.xml, is_map) {
            (true, true) => "<dict>\n",
            (true, false) => "<array>\n",
            (false, true) => "{\n",
            (false, false) => "(\n",
        });
        self.stack.push(if is_map {
            Open::Dict(None)
        } else {
            Open::Array
        });
    }

    /// Writes an empty container.
    fn write_empty(&mut self, is_map: bool) -> Result<(), Error> {
        self.out.push_str(match (self.xml, is_map) {
            (true, true) => "<dict/>\n",
            (true, false) => "<array/>\n",
            (false, true) => "{}",
            (false, false) => "()",
        });
        self.complete();
        Ok(())
    }

    /// Closes the innermost container.
    fn close(&mut self) -> Result<(), Error> {
        let is_map = match self.stack.pop() {
            Some(Open::Array) => false,
            Some(Open::Dict(None)) => true,
            Some(Open::Dict(Some(_))) => {
                return Err(Error::new(ErrorKind::Unexpected, "map without value"));
            }
            None => return Err(Error::new(ErrorKind::Unexpected, "unexpected end")),
        };
        indent(&mut self.out, self.stack.len());
        self.out.push_str(match (self.xml, is_map) {
            (true, true) => "</dict>\n",
            (true, false) => "</array>\n",
            (false, true) => "}",
            (false, false) => ")",
        });
        self.complete();
        Ok(())
    }

    /// Records that a value was written.
    fn complete(&mut self) {
        if !self.xml {
            self.out.push_str(match self.stack.last() {
                Some(Open::Dict(_)) => ";\n",
                Some(Open::Array) => ",\n",
                None => "\n",
            });
        }
        if self.stack.is_empty() {
            self.done = true;
        }
    }
}

/// Returns what an event starts.
fn item(event: Event<'_>) -> Result<Item, Error> {
    Ok(match event {
        Event::Atom(atom) => match convert_atom(atom)? {
            Some(node) => Item::Node(node),
            None => Item::Null,
        },
        Event::MapStart(_) => Item::Start(true),
        Event::SeqStart(_) => Item::Start(false),
        Event::MapEnd | Event::SeqEnd => {
            return Err(Error::new(ErrorKind::Unexpected, "unexpected end event"));
        }
    })
}

/// Indents a line, both formats indent with tabs.
fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}
