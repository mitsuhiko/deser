//! A TOML 1.1 parser.
//!
//! The parser reads the whole input into a [`Document`].  It does not
//! recurse, arbitrarily nested arrays and inline tables are parsed with an
//! explicit stack.
//!
//! # Table definitions
//!
//! Most of the complexity of TOML is in the rules for which tables can be
//! defined and extended where.  Every table remembers how it was created
//! (see [`TableKind`]):
//!
//! * `[a.b.c]` creates `a` and `a.b` as implicit tables (unless they
//!   exist) and defines `a.b.c`.  Implicit tables can be defined by a header
//!   later, all other tables cannot be defined again.  Headers can create
//!   sub-tables in all tables other than inline tables, and `[[a]]` headers
//!   extend arrays of tables (other arrays cannot be extended).  A path
//!   that goes through an array of tables refers to its last table.
//! * Dotted keys (`a.b.c = 1`) create and define tables for all but the
//!   last key.  These can only be extended by dotted keys under the same
//!   header (or in the same inline table), see
//!   <https://github.com/toml-lang/toml/issues/846> and
//!   <https://github.com/toml-lang/toml/pull/859>.  Every header and every
//!   inline table starts a new section to track this.  Tables that were
//!   only created by headers (as parents) are not defined yet, so dotted
//!   keys can define them.  The order of table sections does not matter
//!   (<https://github.com/toml-lang/toml/issues/771>,
//!   <https://github.com/toml-lang/toml/issues/1032>): `[a.b.c]` followed
//!   by `[a] b.d = 1` is as valid as `[a] b.d = 1` followed by `[a.b.c]`.
//! * Inline tables and arrays (`{...}`, `[...]`) are complete, nothing can
//!   be added to them afterwards.
use std::borrow::Cow;

use deser::{Error, ErrorKind};

use crate::datetime::{is_datetime_start, parse_datetime};
use crate::document::{Document, Entry, Item, Span, TableKind, Value};

/// The index of the root table.
pub(crate) const ROOT: usize = 0;

/// A part of a (dotted) key.
struct Key<'a> {
    name: Cow<'a, str>,
    span: Span,
}

/// The kind of value that was found for a key.
#[derive(Clone, Copy)]
enum Found {
    Table(usize),
    Array(usize),
    Other,
}

/// Where a parsed value is placed.
enum Target<'a> {
    /// The value for a key in a table.
    Entry { table: usize, key: Key<'a> },
    /// The next item in an array.
    Item(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameState {
    /// The container was just opened.
    Open,
    /// A comma was read.
    AfterComma,
    /// A value was read.
    AfterValue,
}

/// An inline array or table that is being parsed.
struct Frame {
    is_table: bool,
    id: usize,
    /// The section of an inline table.
    section: u32,
    state: FrameState,
}

/// Parses a TOML document.
pub(crate) fn parse(input: &str) -> Result<Document<'_>, Error> {
    let mut parser = Parser {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        doc: Document::default(),
        next_section: 1,
        keys: Vec::new(),
    };
    parser.parse_document()?;
    Ok(parser.doc)
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    doc: Document<'a>,
    next_section: u32,
    /// Scratch space for the parts of dotted keys.
    keys: Vec<Key<'a>>,
}

impl<'a> Parser<'a> {
    fn parse_document(&mut self) -> Result<(), Error> {
        // a BOM at the start is permitted (and ignored)
        if self.bytes.starts_with(b"\xef\xbb\xbf") {
            self.pos = 3;
        }
        let root = self
            .doc
            .new_table(TableKind::Header, Span::new(0, self.bytes.len()));
        debug_assert_eq!(root, ROOT);
        let mut table = root;
        let mut section = 0;

        loop {
            self.skip_ws();
            match self.peek() {
                None => return Ok(()),
                Some(b'\n' | b'\r' | b'#') => {}
                Some(b'[') => {
                    section = self.new_section();
                    table = self.parse_header()?;
                }
                Some(_) => self.parse_keyval(table, section)?,
            }
            self.parse_line_end()?;
        }
    }

    fn new_section(&mut self) -> u32 {
        let rv = self.next_section;
        self.next_section += 1;
        rv
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    #[inline]
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    #[cold]
    fn error(&self, pos: usize, msg: &str) -> Error {
        error_at(self.input, pos, ErrorKind::Unexpected, msg)
    }

    /// Creates an error for the byte at the current position.
    #[cold]
    fn unexpected(&self, expected: &str) -> Error {
        match self.input.get(self.pos..).and_then(|x| x.chars().next()) {
            None => error_at(
                self.input,
                self.pos,
                ErrorKind::EndOfFile,
                &format!("unexpected end of input, expected {}", expected),
            ),
            Some(c) => self.error(
                self.pos,
                &format!("unexpected {}, expected {}", describe_char(c), expected),
            ),
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t') = self.peek() {
            self.pos += 1;
        }
    }

    /// Skips a comment (the current byte is `#`) up to the end of the line.
    fn skip_comment(&mut self) -> Result<(), Error> {
        self.pos += 1;
        loop {
            match self.peek() {
                None | Some(b'\n') => return Ok(()),
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => return Ok(()),
                Some(b'\t' | 0x20..=0x7e | 0x80..) => self.pos += 1,
                Some(_) => return Err(self.control_char_error("comment")),
            }
        }
    }

    #[cold]
    fn control_char_error(&self, what: &str) -> Error {
        let c = self.bytes[self.pos];
        let msg = if c == b'\r' {
            format!("carriage return without line feed in {}", what)
        } else {
            format!("control character U+{:04X} in {}", c, what)
        };
        self.error(self.pos, &msg)
    }

    /// Consumes a newline (LF or CRLF) if there is one.
    fn eat_newline(&mut self) -> Result<bool, Error> {
        match self.peek() {
            Some(b'\n') => {
                self.pos += 1;
                Ok(true)
            }
            Some(b'\r') => {
                if self.peek_at(1) == Some(b'\n') {
                    self.pos += 2;
                    Ok(true)
                } else {
                    Err(self.control_char_error("document"))
                }
            }
            _ => Ok(false),
        }
    }

    /// Parses the end of a line: whitespace, an optional comment and a
    /// newline (or the end of the input).
    fn parse_line_end(&mut self) -> Result<(), Error> {
        self.skip_ws();
        if self.peek() == Some(b'#') {
            self.skip_comment()?;
        }
        if self.pos == self.bytes.len() || self.eat_newline()? {
            Ok(())
        } else {
            Err(self.unexpected("end of line"))
        }
    }

    /// Skips whitespace, comments and newlines within arrays and inline
    /// tables.
    fn skip_ws_comments_newlines(&mut self) -> Result<(), Error> {
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'#') => self.skip_comment()?,
                Some(b'\n' | b'\r') => {
                    self.eat_newline()?;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Parses a `[table]` or `[[table]]` header and returns the table.
    fn parse_header(&mut self) -> Result<usize, Error> {
        let start = self.pos;
        self.pos += 1;
        let is_array = self.peek() == Some(b'[');
        if is_array {
            self.pos += 1;
        }
        self.skip_ws();
        self.parse_key()?;
        self.skip_ws();
        if self.peek() != Some(b']') {
            return Err(self.unexpected("']'"));
        }
        self.pos += 1;
        if is_array {
            if self.peek() != Some(b']') {
                return Err(self.unexpected("']]'"));
            }
            self.pos += 1;
        }
        let span = Span::new(start, self.pos);

        let keys = std::mem::take(&mut self.keys);
        let rv = self.define_table(&keys, is_array, span);
        self.keys = keys;
        rv
    }

    /// Resolves the tables of a header.
    fn define_table(
        &mut self,
        keys: &[Key<'a>],
        is_array: bool,
        span: Span,
    ) -> Result<usize, Error> {
        let (last, parents) = keys.split_last().unwrap();
        let mut table = ROOT;
        for key in parents {
            table = match self.lookup(table, &key.name) {
                None => self.add_table(table, key, TableKind::Implicit, key.span),
                Some(Found::Table(id)) if self.doc.tables[id].kind != TableKind::Inline => id,
                Some(Found::Table(_)) => {
                    return Err(self.error(key.span.start, "cannot extend an inline table"));
                }
                Some(Found::Array(id)) if self.doc.arrays[id].of_tables => {
                    match self.doc.arrays[id].items.last().map(|x| &x.value) {
                        Some(&Value::Table(id)) => id,
                        _ => unreachable!("array of tables without table"),
                    }
                }
                Some(Found::Array(_)) => {
                    return Err(self.error(key.span.start, "cannot extend a static array"));
                }
                Some(Found::Other) => {
                    return Err(self.error(
                        key.span.start,
                        &format!("key '{}' is not a table", key.name),
                    ));
                }
            };
        }

        let existing = self.lookup(table, &last.name);
        if is_array {
            match existing {
                None => {
                    let array = self.doc.new_array(true, span);
                    self.doc.insert(
                        table,
                        Entry {
                            key: last.name.clone(),
                            key_span: last.span,
                            item: Item {
                                value: Value::Array(array),
                                span,
                            },
                        },
                    );
                    Ok(self.add_array_table(array, span))
                }
                Some(Found::Array(array)) if self.doc.arrays[array].of_tables => {
                    Ok(self.add_array_table(array, span))
                }
                Some(Found::Array(_)) => {
                    Err(self.error(last.span.start, "cannot extend a static array"))
                }
                Some(Found::Table(_)) => Err(self.error(
                    last.span.start,
                    &format!("table '{}' is not an array of tables", last.name),
                )),
                Some(Found::Other) => Err(self.error(
                    last.span.start,
                    &format!("key '{}' is already defined", last.name),
                )),
            }
        } else {
            match existing {
                None => Ok(self.add_table(table, last, TableKind::Header, span)),
                Some(Found::Table(id)) if self.doc.tables[id].kind == TableKind::Implicit => {
                    let table = &mut self.doc.tables[id];
                    table.kind = TableKind::Header;
                    table.span = span;
                    Ok(id)
                }
                Some(Found::Table(_)) => Err(self.error(
                    last.span.start,
                    &format!("table '{}' is already defined", last.name),
                )),
                Some(Found::Array(id)) if self.doc.arrays[id].of_tables => Err(self.error(
                    last.span.start,
                    &format!("'{}' is already defined as array of tables", last.name),
                )),
                Some(_) => Err(self.error(
                    last.span.start,
                    &format!("key '{}' is already defined", last.name),
                )),
            }
        }
    }

    /// Looks up a key in a table.
    fn lookup(&self, table: usize, key: &str) -> Option<Found> {
        self.doc
            .find(table, key)
            .map(|entry| match entry.item.value {
                Value::Table(id) => Found::Table(id),
                Value::Array(id) => Found::Array(id),
                _ => Found::Other,
            })
    }

    /// Creates a table and adds it to another table.
    fn add_table(&mut self, parent: usize, key: &Key<'a>, kind: TableKind, span: Span) -> usize {
        let id = self.doc.new_table(kind, span);
        self.doc.insert(
            parent,
            Entry {
                key: key.name.clone(),
                key_span: key.span,
                item: Item {
                    value: Value::Table(id),
                    span,
                },
            },
        );
        id
    }

    /// Adds a new table to an array of tables.
    fn add_array_table(&mut self, array: usize, span: Span) -> usize {
        let id = self.doc.new_table(TableKind::Header, span);
        self.doc.arrays[array].items.push(Item {
            value: Value::Table(id),
            span,
        });
        id
    }

    /// Parses a key/value pair and adds it to the table.
    fn parse_keyval(&mut self, table: usize, section: u32) -> Result<(), Error> {
        let target = self.parse_keyval_start(table, section)?;
        self.parse_value(target)
    }

    /// Parses the key and the equals sign of a key/value pair.
    ///
    /// Returns the target for the value.
    fn parse_keyval_start(&mut self, table: usize, section: u32) -> Result<Target<'a>, Error> {
        self.parse_key()?;
        self.skip_ws();
        if self.peek() != Some(b'=') {
            return Err(self.unexpected("'=' after key"));
        }
        self.pos += 1;
        self.skip_ws();

        let mut keys = std::mem::take(&mut self.keys);
        let rv = self.resolve_dotted_key(table, section, &keys);
        let key = keys.pop().unwrap();
        keys.clear();
        self.keys = keys;
        Ok(Target::Entry { table: rv?, key })
    }

    /// Resolves the tables of a dotted key (all but the last part).
    fn resolve_dotted_key(
        &mut self,
        mut table: usize,
        section: u32,
        keys: &[Key<'a>],
    ) -> Result<usize, Error> {
        for key in &keys[..keys.len() - 1] {
            table = match self.lookup(table, &key.name) {
                None => self.add_table(table, key, TableKind::Dotted(section), key.span),
                Some(Found::Table(id)) => match self.doc.tables[id].kind {
                    TableKind::Dotted(s) if s == section => id,
                    // tables that were created by headers without being
                    // defined can be defined by dotted keys.
                    TableKind::Implicit => {
                        self.doc.tables[id].kind = TableKind::Dotted(section);
                        id
                    }
                    _ => {
                        return Err(self.error(
                            key.span.start,
                            &format!(
                                "cannot add keys to table '{}' which is defined elsewhere",
                                key.name
                            ),
                        ));
                    }
                },
                Some(_) => {
                    return Err(self.error(
                        key.span.start,
                        &format!("key '{}' is already defined", key.name),
                    ));
                }
            };
        }
        Ok(table)
    }

    /// Parses a (dotted) key into `self.keys`.
    fn parse_key(&mut self) -> Result<(), Error> {
        self.keys.clear();
        loop {
            let start = self.pos;
            let name = match self.peek() {
                Some(b'"') => {
                    if self.bytes[self.pos..].starts_with(b"\"\"\"") {
                        return Err(self.error(start, "multi-line strings cannot be used as keys"));
                    }
                    self.parse_basic_string(false)?
                }
                Some(b'\'') => {
                    if self.bytes[self.pos..].starts_with(b"'''") {
                        return Err(self.error(start, "multi-line strings cannot be used as keys"));
                    }
                    self.parse_literal_string(false)?
                }
                Some(c) if is_bare_key_char(c) => {
                    while self.peek().is_some_and(is_bare_key_char) {
                        self.pos += 1;
                    }
                    Cow::Borrowed(&self.input[start..self.pos])
                }
                _ => return Err(self.unexpected("a key")),
            };
            self.keys.push(Key {
                name,
                span: Span::new(start, self.pos),
            });

            let end = self.pos;
            self.skip_ws();
            if self.peek() == Some(b'.') {
                self.pos += 1;
                self.skip_ws();
            } else {
                self.pos = end;
                return Ok(());
            }
        }
    }

    /// Parses a value and places it into the target.
    ///
    /// Arrays and inline tables are parsed with an explicit stack.
    fn parse_value(&mut self, mut target: Target<'a>) -> Result<(), Error> {
        let mut stack: Vec<Frame> = Vec::new();

        'value: loop {
            let start = self.pos;
            match self.peek() {
                Some(b'[') => {
                    self.pos += 1;
                    let id = self.doc.new_array(false, Span::new(start, start));
                    self.place(target, Value::Array(id), Span::new(start, start))?;
                    stack.push(Frame {
                        is_table: false,
                        id,
                        section: 0,
                        state: FrameState::Open,
                    });
                }
                Some(b'{') => {
                    self.pos += 1;
                    let section = self.new_section();
                    let id = self
                        .doc
                        .new_table(TableKind::Inline, Span::new(start, start));
                    self.place(target, Value::Table(id), Span::new(start, start))?;
                    stack.push(Frame {
                        is_table: true,
                        id,
                        section,
                        state: FrameState::Open,
                    });
                }
                _ => {
                    let value = self.parse_scalar()?;
                    self.place(target, value, Span::new(start, self.pos))?;
                }
            }

            // find the place of the next value
            loop {
                let Some(frame) = stack.last_mut() else {
                    return Ok(());
                };
                self.skip_ws_comments_newlines()?;
                let close = if frame.is_table { b'}' } else { b']' };
                match self.peek() {
                    // empty containers and trailing commas are permitted
                    Some(c) if c == close => {
                        self.pos += 1;
                        let end = self.pos;
                        if frame.is_table {
                            self.doc.tables[frame.id].span.end = end;
                        } else {
                            self.doc.arrays[frame.id].span.end = end;
                        }
                        stack.pop();
                    }
                    Some(b',') if frame.state == FrameState::AfterValue => {
                        self.pos += 1;
                        frame.state = FrameState::AfterComma;
                    }
                    _ if frame.state == FrameState::AfterValue => {
                        return Err(self.unexpected(if frame.is_table {
                            "',' or '}'"
                        } else {
                            "',' or ']'"
                        }));
                    }
                    Some(b',') => return Err(self.unexpected("a value")),
                    None => {
                        return Err(self.unexpected(if frame.is_table {
                            "a key or '}'"
                        } else {
                            "a value or ']'"
                        }));
                    }
                    Some(_) => {
                        frame.state = FrameState::AfterValue;
                        target = if frame.is_table {
                            let (table, section) = (frame.id, frame.section);
                            self.parse_keyval_start(table, section)?
                        } else {
                            Target::Item(frame.id)
                        };
                        continue 'value;
                    }
                }
            }
        }
    }

    /// Places a value into its target.
    fn place(&mut self, target: Target<'a>, value: Value<'a>, span: Span) -> Result<(), Error> {
        match target {
            Target::Item(array) => {
                self.doc.arrays[array].items.push(Item { value, span });
            }
            Target::Entry { table, key } => {
                if self.doc.find(table, &key.name).is_some() {
                    return Err(self.error(
                        key.span.start,
                        &format!("key '{}' is already defined", key.name),
                    ));
                }
                self.doc.insert(
                    table,
                    Entry {
                        key: key.name,
                        key_span: key.span,
                        item: Item { value, span },
                    },
                );
            }
        }
        Ok(())
    }

    /// Parses a value other than an array or inline table.
    fn parse_scalar(&mut self) -> Result<Value<'a>, Error> {
        match self.peek() {
            Some(b'"') => self.parse_basic_string(true).map(Value::Str),
            Some(b'\'') => self.parse_literal_string(true).map(Value::Str),
            Some(b't') => self.parse_keyword("true", Value::Bool(true)),
            Some(b'f') => self.parse_keyword("false", Value::Bool(false)),
            Some(b'0'..=b'9') if is_datetime_start(self.bytes, self.pos) => {
                match parse_datetime(self.bytes, self.pos) {
                    Ok((value, end)) => {
                        self.pos = end;
                        Ok(Value::Datetime(value))
                    }
                    Err((pos, msg)) => Err(self.error(pos, msg)),
                }
            }
            Some(b'0'..=b'9' | b'+' | b'-' | b'i' | b'n') => self.parse_number(),
            _ => Err(self.unexpected("a value")),
        }
    }

    fn parse_keyword(&mut self, keyword: &str, value: Value<'a>) -> Result<Value<'a>, Error> {
        if self.bytes[self.pos..].starts_with(keyword.as_bytes())
            && !self
                .peek_at(keyword.len())
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            self.pos += keyword.len();
            Ok(value)
        } else {
            Err(self.unexpected("a value"))
        }
    }

    /// Parses an integer or float.
    fn parse_number(&mut self) -> Result<Value<'a>, Error> {
        let start = self.pos;
        while let Some(b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.' | b'+' | b'-') =
            self.peek()
        {
            self.pos += 1;
        }
        let token = &self.input[start..self.pos];
        parse_number(token).map_err(|(kind, msg)| error_at(self.input, start, kind, msg))
    }

    /// Parses a basic string (the current byte is `"`).
    ///
    /// If `multiline` is set, multi-line strings are permitted.
    fn parse_basic_string(&mut self, multiline: bool) -> Result<Cow<'a, str>, Error> {
        let multiline = multiline && self.bytes[self.pos..].starts_with(b"\"\"\"");
        if multiline {
            self.pos += 3;
            // a newline right after the delimiter is trimmed
            self.eat_newline()?;
        } else {
            self.pos += 1;
        }

        let mut owned: Option<String> = None;
        let mut chunk_start = self.pos;
        loop {
            let Some(c) = self.peek() else {
                return Err(self.unexpected("'\"'"));
            };
            match c {
                b'"' => {
                    let end;
                    if multiline {
                        // one or two quotes are part of the string, up to
                        // two quotes can precede the closing delimiter.
                        let quotes = self.bytes[self.pos..]
                            .iter()
                            .take_while(|&&c| c == b'"')
                            .count();
                        if quotes < 3 {
                            self.pos += quotes;
                            continue;
                        } else if quotes > 5 {
                            return Err(self.error(
                                self.pos + 5,
                                "too many quotes at the end of a multi-line string",
                            ));
                        }
                        end = self.pos + quotes - 3;
                        self.pos += quotes;
                    } else {
                        end = self.pos;
                        self.pos += 1;
                    }
                    let rest = &self.input[chunk_start..end];
                    return Ok(match owned {
                        Some(mut owned) => {
                            owned.push_str(rest);
                            Cow::Owned(owned)
                        }
                        None => Cow::Borrowed(rest),
                    });
                }
                b'\\' => {
                    let buf = owned.get_or_insert_with(String::new);
                    buf.push_str(&self.input[chunk_start..self.pos]);
                    self.pos += 1;
                    self.parse_escape(multiline, owned.as_mut().unwrap())?;
                    chunk_start = self.pos;
                }
                b'\n' if multiline => self.pos += 1,
                b'\r' if multiline && self.peek_at(1) == Some(b'\n') => {
                    // newlines are normalized to LF
                    let buf = owned.get_or_insert_with(String::new);
                    buf.push_str(&self.input[chunk_start..self.pos]);
                    buf.push('\n');
                    self.pos += 2;
                    chunk_start = self.pos;
                }
                b'\t' | 0x20..=0x7e | 0x80.. => self.pos += 1,
                b'\n' | b'\r' if !multiline => {
                    return Err(self.error(self.pos, "newline in single-line string"));
                }
                _ => return Err(self.control_char_error("string")),
            }
        }
    }

    /// Parses an escape sequence (after the backslash).
    fn parse_escape(&mut self, multiline: bool, buf: &mut String) -> Result<(), Error> {
        let start = self.pos - 1;
        let Some(c) = self.peek() else {
            return Err(self.unexpected("an escape sequence"));
        };
        self.pos += 1;
        let c = match c {
            b'b' => '\x08',
            b't' => '\t',
            b'n' => '\n',
            b'f' => '\x0c',
            b'r' => '\r',
            b'e' => '\x1b',
            b'"' => '"',
            b'\\' => '\\',
            b'x' => self.parse_hex_escape(start, 2)?,
            b'u' => self.parse_hex_escape(start, 4)?,
            b'U' => self.parse_hex_escape(start, 8)?,
            b' ' | b'\t' | b'\n' | b'\r' if multiline => {
                // a line ending backslash trims all whitespace and newlines
                // up to the next non-whitespace character.  Only whitespace
                // can follow the backslash on its line.
                self.pos -= 1;
                self.skip_ws();
                if !self.eat_newline()? {
                    return Err(self.error(start, "invalid escape sequence"));
                }
                loop {
                    self.skip_ws();
                    if !self.eat_newline()? {
                        return Ok(());
                    }
                }
            }
            _ => return Err(self.error(start, "invalid escape sequence")),
        };
        buf.push(c);
        Ok(())
    }

    fn parse_hex_escape(&mut self, start: usize, digits: usize) -> Result<char, Error> {
        let hex = self
            .bytes
            .get(self.pos..self.pos + digits)
            .filter(|x| x.iter().all(u8::is_ascii_hexdigit))
            .ok_or_else(|| self.error(start, "invalid escape sequence"))?;
        // the digits are ASCII
        let value = u32::from_str_radix(std::str::from_utf8(hex).unwrap(), 16).unwrap();
        self.pos += digits;
        char::from_u32(value)
            .ok_or_else(|| self.error(start, "escape sequence is not a unicode scalar value"))
    }

    /// Parses a literal string (the current byte is `'`).
    ///
    /// If `multiline` is set, multi-line strings are permitted.
    fn parse_literal_string(&mut self, multiline: bool) -> Result<Cow<'a, str>, Error> {
        let multiline = multiline && self.bytes[self.pos..].starts_with(b"'''");
        if multiline {
            self.pos += 3;
            self.eat_newline()?;
        } else {
            self.pos += 1;
        }

        let mut owned: Option<String> = None;
        let mut chunk_start = self.pos;
        loop {
            let Some(c) = self.peek() else {
                return Err(self.unexpected("\"'\""));
            };
            match c {
                b'\'' => {
                    let end;
                    if multiline {
                        let quotes = self.bytes[self.pos..]
                            .iter()
                            .take_while(|&&c| c == b'\'')
                            .count();
                        if quotes < 3 {
                            self.pos += quotes;
                            continue;
                        } else if quotes > 5 {
                            return Err(self.error(
                                self.pos + 5,
                                "too many quotes at the end of a multi-line string",
                            ));
                        }
                        end = self.pos + quotes - 3;
                        self.pos += quotes;
                    } else {
                        end = self.pos;
                        self.pos += 1;
                    }
                    let rest = &self.input[chunk_start..end];
                    return Ok(match owned {
                        Some(mut owned) => {
                            owned.push_str(rest);
                            Cow::Owned(owned)
                        }
                        None => Cow::Borrowed(rest),
                    });
                }
                b'\n' if multiline => self.pos += 1,
                b'\r' if multiline && self.peek_at(1) == Some(b'\n') => {
                    let buf = owned.get_or_insert_with(String::new);
                    buf.push_str(&self.input[chunk_start..self.pos]);
                    buf.push('\n');
                    self.pos += 2;
                    chunk_start = self.pos;
                }
                b'\t' | 0x20..=0x7e | 0x80.. => self.pos += 1,
                b'\n' | b'\r' if !multiline => {
                    return Err(self.error(self.pos, "newline in single-line string"));
                }
                _ => return Err(self.control_char_error("string")),
            }
        }
    }
}

fn is_bare_key_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// Describes a character for error messages.
fn describe_char(c: char) -> String {
    match c {
        '\n' | '\r' => "newline".into(),
        c if c.is_control() => format!("control character U+{:04X}", c as u32),
        c => format!("'{}'", c),
    }
}

/// Creates an error with the location of the byte offset.
#[cold]
pub(crate) fn error_at(input: &str, pos: usize, kind: ErrorKind, msg: &str) -> Error {
    Error::new(kind, msg.to_string())
        .with_offset(pos.min(input.len()))
        .resolve_position(input.as_bytes())
}

type NumberError = (ErrorKind, &'static str);

/// Checks digits with optional underscores between them.
fn is_valid_digits(s: &str, radix: u32) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && bytes[0] != b'_'
        && bytes[bytes.len() - 1] != b'_'
        && !s.contains("__")
        && s.chars().all(|c| c == '_' || c.is_digit(radix))
}

fn strip_underscores(s: &str) -> Cow<'_, str> {
    if s.contains('_') {
        Cow::Owned(s.replace('_', ""))
    } else {
        Cow::Borrowed(s)
    }
}

/// Parses an integer or float token.
fn parse_number(token: &str) -> Result<Value<'static>, NumberError> {
    const INVALID: NumberError = (ErrorKind::Unexpected, "invalid number");

    let (negative, unsigned) = match token.as_bytes().first() {
        Some(b'-') => (true, &token[1..]),
        Some(b'+') => (false, &token[1..]),
        _ => (false, token),
    };
    let has_sign = unsigned.len() != token.len();

    match unsigned {
        "inf" => {
            return Ok(Value::Float(if negative {
                -f64::INFINITY
            } else {
                f64::INFINITY
            }));
        }
        "nan" => return Ok(Value::Float(if negative { -f64::NAN } else { f64::NAN })),
        _ => {}
    }

    let radix = match unsigned.get(..2) {
        Some("0x") => 16,
        Some("0o") => 8,
        Some("0b") => 2,
        _ => 10,
    };
    if radix != 10 {
        if has_sign {
            return Err((
                ErrorKind::Unexpected,
                "signs are not permitted for hexadecimal, octal and binary integers",
            ));
        }
        let digits = &unsigned[2..];
        if !is_valid_digits(digits, radix) {
            return Err(INVALID);
        }
        return match u64::from_str_radix(&strip_underscores(digits), radix) {
            Ok(value) => Ok(int_value(false, value)),
            Err(_) => Err((ErrorKind::OutOfRange, "integer out of range")),
        };
    }

    // the integer part of floats follows the rules of decimal integers
    let int_end = unsigned.find(['.', 'e', 'E']).unwrap_or(unsigned.len());
    let int_part = &unsigned[..int_end];
    if !is_valid_digits(int_part, 10) {
        return Err(INVALID);
    }
    if int_part.len() > 1 && int_part.starts_with('0') {
        return Err((ErrorKind::Unexpected, "leading zeros are not permitted"));
    }

    if int_end == unsigned.len() {
        return match strip_underscores(int_part).parse::<u64>() {
            Ok(value) => {
                if !negative {
                    Ok(int_value(false, value))
                } else if value <= i64::MIN.unsigned_abs() {
                    Ok(int_value(true, value))
                } else {
                    Err((ErrorKind::OutOfRange, "integer out of range"))
                }
            }
            Err(_) => Err((ErrorKind::OutOfRange, "integer out of range")),
        };
    }

    let mut rest = &unsigned[int_end..];
    if let Some(frac) = rest.strip_prefix('.') {
        let frac_end = frac.find(['e', 'E']).unwrap_or(frac.len());
        if !is_valid_digits(&frac[..frac_end], 10) {
            return Err(INVALID);
        }
        rest = &frac[frac_end..];
    }
    if !rest.is_empty() {
        let exp = &rest[1..];
        let exp = exp
            .strip_prefix('+')
            .or_else(|| exp.strip_prefix('-'))
            .unwrap_or(exp);
        if !is_valid_digits(exp, 10) {
            return Err(INVALID);
        }
    }

    // the token is validated, which leaves only digits, a sign, a dot and
    // the exponent for the float parser.
    let value: f64 = strip_underscores(token).parse().map_err(|_| INVALID)?;
    if value.is_infinite() {
        return Err((ErrorKind::OutOfRange, "float out of range"));
    }
    Ok(Value::Float(value))
}

fn int_value(negative: bool, value: u64) -> Value<'static> {
    if negative {
        Value::Int((value as i64).wrapping_neg())
    } else if let Ok(value) = i64::try_from(value) {
        Value::Int(value)
    } else {
        Value::UInt(value)
    }
}
