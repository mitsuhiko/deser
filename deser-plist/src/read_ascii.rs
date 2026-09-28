//! The reader of OpenStep (ASCII) property lists.
//!
//! OpenStep property lists only know strings, data, arrays and
//! dictionaries.  All strings are emitted as lexical atoms, so they can be
//! deserialized into numbers and booleans (`YES` and `NO` are booleans).
//!
//! Like Core Foundation the reader also accepts the format of `.strings`
//! files: a dictionary without braces at the top level
//! (`"key" = "value";`).  In dictionaries an entry can consist of a key
//! only (`"key";`), the value is then the key.
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use deser_core::{Atom, Bytes, ContainerShape, Error, Event, Text};

use crate::common::{Out, eof_error, syntax_error};

/// An open array or dictionary.
#[derive(Clone, Copy)]
enum Frame {
    /// `true` if a value was read and a separator or the end is expected.
    Array(bool),
    Dict {
        /// `true` if a value was read and `;` is expected.
        after_value: bool,
        /// `true` for the dictionary without braces of `.strings` files.
        implicit: bool,
    },
}

struct Reader<'i> {
    src: &'i str,
    pos: usize,
    stack: Vec<Frame>,
}

/// Parses an OpenStep property list and emits its events.
pub(crate) fn parse<'i, O: Out<'i>>(input: &'i str, out: &mut O) -> Result<(), Error> {
    let mut reader = Reader {
        src: input,
        pos: 0,
        stack: Vec::new(),
    };
    reader.document(out)
}

/// Returns `true` for the characters of strings without quotes.
fn is_unquoted(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'/' | b':' | b'.' | b'-')
}

impl<'i> Reader<'i> {
    fn document<O: Out<'i>>(&mut self, out: &mut O) -> Result<(), Error> {
        self.skip()?;
        if self.pos == self.src.len() {
            // an empty `.strings` file
            out.emit_at(0, 0, Event::MapStart(ContainerShape::new().with_len(0)))?;
            return out.emit_at(0, 0, Event::MapEnd);
        }
        if self.is_strings_file()? {
            out.emit_at(self.pos, self.pos, Event::MapStart(ContainerShape::new()))?;
            self.stack.push(Frame::Dict {
                after_value: false,
                implicit: true,
            });
        } else {
            self.value(out)?;
        }
        self.run(out)?;
        self.skip()?;
        if self.pos < self.src.len() {
            return Err(syntax_error(self.pos, "unexpected content after the plist"));
        }
        Ok(())
    }

    /// Checks if the document starts with a key, which makes it a
    /// dictionary without braces.
    fn is_strings_file(&mut self) -> Result<bool, Error> {
        let pos = self.pos;
        if self
            .peek()
            .is_none_or(|c| !is_unquoted(c) && c != b'"' && c != b'\'')
        {
            return Ok(false);
        }
        self.string()?;
        self.skip()?;
        let rv = matches!(self.peek(), Some(b'=' | b';'));
        self.pos = pos;
        Ok(rv)
    }

    fn run<O: Out<'i>>(&mut self, out: &mut O) -> Result<(), Error> {
        while let Some(&frame) = self.stack.last() {
            self.skip()?;
            let start = self.pos;
            match frame {
                Frame::Array(after_value) => {
                    if self.eat(b')') {
                        self.stack.pop();
                        out.emit_at(start, self.pos, Event::SeqEnd)?;
                    } else if after_value {
                        if !self.eat(b',') {
                            return Err(self.unexpected("expected `,` or `)`"));
                        }
                        *self.stack.last_mut().unwrap() = Frame::Array(false);
                    } else {
                        *self.stack.last_mut().unwrap() = Frame::Array(true);
                        self.value(out)?;
                    }
                }
                Frame::Dict {
                    after_value: true,
                    implicit,
                } => {
                    if !self.eat(b';') {
                        return Err(self.unexpected("expected `;`"));
                    }
                    *self.stack.last_mut().unwrap() = Frame::Dict {
                        after_value: false,
                        implicit,
                    };
                }
                Frame::Dict {
                    after_value: false,
                    implicit,
                } => {
                    let end = if implicit {
                        self.pos == self.src.len()
                    } else {
                        self.eat(b'}')
                    };
                    if end {
                        self.stack.pop();
                        out.emit_at(start, self.pos, Event::MapEnd)?;
                        continue;
                    }
                    if !self
                        .peek()
                        .is_some_and(|c| is_unquoted(c) || c == b'"' || c == b'\'')
                    {
                        return Err(self.unexpected(if implicit {
                            "expected a key"
                        } else {
                            "expected a key or `}`"
                        }));
                    }
                    let key = self.string()?;
                    let key_end = self.pos;
                    self.skip()?;
                    *self.stack.last_mut().unwrap() = Frame::Dict {
                        after_value: true,
                        implicit,
                    };
                    // `"key";` is short for `"key" = "key";`
                    if self.peek() == Some(b';') {
                        emit_text(out, start, key_end, key.clone())?;
                        emit_text(out, start, key_end, key)?;
                    } else {
                        emit_text(out, start, key_end, key)?;
                        if !self.eat(b'=') {
                            return Err(self.unexpected("expected `=`"));
                        }
                        self.skip()?;
                        self.value(out)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    #[cold]
    fn unexpected(&self, msg: &str) -> Error {
        if self.pos == self.src.len() {
            eof_error(self.pos)
        } else {
            syntax_error(self.pos, msg)
        }
    }

    /// Skips whitespace and comments.
    fn skip(&mut self) -> Result<(), Error> {
        let bytes = self.src.as_bytes();
        loop {
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_whitespace() || c == b'\x0b')
            {
                self.pos += 1;
            }
            let rest = &bytes[self.pos..];
            if rest.starts_with(b"//") {
                self.pos += rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
            } else if rest.starts_with(b"/*") {
                match self.src[self.pos + 2..].find("*/") {
                    Some(idx) => self.pos += idx + 4,
                    None => return Err(syntax_error(self.pos, "unterminated comment")),
                }
            } else {
                return Ok(());
            }
        }
    }

    /// Parses a value.  Arrays and dictionaries are pushed to the stack.
    fn value<O: Out<'i>>(&mut self, out: &mut O) -> Result<(), Error> {
        let start = self.pos;
        match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                out.emit_at(start, self.pos, Event::MapStart(ContainerShape::new()))?;
                self.stack.push(Frame::Dict {
                    after_value: false,
                    implicit: false,
                });
                Ok(())
            }
            Some(b'(') => {
                self.pos += 1;
                out.emit_at(start, self.pos, Event::SeqStart(ContainerShape::new()))?;
                self.stack.push(Frame::Array(false));
                Ok(())
            }
            Some(b'<') => {
                let data = self.data()?;
                out.emit_at(start, self.pos, Atom::Bytes(Bytes::new(data)))
            }
            Some(c) if is_unquoted(c) || c == b'"' || c == b'\'' => {
                let text = self.string()?;
                emit_text(out, start, self.pos, text)
            }
            _ => Err(self.unexpected("expected a value")),
        }
    }

    /// Parses data in angle brackets (`<0fbd 7a>`).
    fn data(&mut self) -> Result<Vec<u8>, Error> {
        let start = self.pos;
        self.pos += 1;
        let mut data = Vec::new();
        let mut high = None;
        loop {
            let Some(c) = self.peek() else {
                return Err(eof_error(self.pos));
            };
            self.pos += 1;
            let nibble = match c {
                b'>' => break,
                c if c.is_ascii_whitespace() => continue,
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(syntax_error(self.pos - 1, "invalid character in data")),
            };
            match high.take() {
                Some(high) => data.push(high << 4 | nibble),
                None => high = Some(nibble),
            }
        }
        if high.is_some() {
            return Err(syntax_error(start, "odd number of hex digits in data"));
        }
        Ok(data)
    }

    /// Parses a string with or without quotes.
    fn string(&mut self) -> Result<Cow<'i, str>, Error> {
        let start = self.pos;
        let quote = match self.peek() {
            Some(c @ (b'"' | b'\'')) => c,
            _ => {
                let len = self.src.as_bytes()[start..]
                    .iter()
                    .position(|&c| !is_unquoted(c))
                    .unwrap_or(self.src.len() - start);
                self.pos += len;
                return Ok(Cow::Borrowed(&self.src[start..self.pos]));
            }
        };
        self.pos += 1;
        let mut owned: Option<String> = None;
        let mut chunk_start = self.pos;
        loop {
            let rest = &self.src.as_bytes()[self.pos..];
            let Some(idx) = rest.iter().position(|&c| c == quote || c == b'\\') else {
                return Err(syntax_error(start, "unterminated string"));
            };
            self.pos += idx;
            let chunk = &self.src[chunk_start..self.pos];
            if rest[idx] == quote {
                self.pos += 1;
                return Ok(match owned {
                    Some(mut buf) => {
                        buf.push_str(chunk);
                        Cow::Owned(buf)
                    }
                    None => Cow::Borrowed(chunk),
                });
            }
            let buf = owned.get_or_insert_with(String::new);
            buf.push_str(chunk);
            self.escape(buf)?;
            chunk_start = self.pos;
        }
    }

    /// Decodes an escape sequence.
    fn escape(&mut self, buf: &mut String) -> Result<(), Error> {
        let start = self.pos;
        self.pos += 1;
        let Some(c) = self.src[self.pos..].chars().next() else {
            return Err(syntax_error(start, "unterminated string"));
        };
        self.pos += c.len_utf8();
        let c = match c {
            'a' => '\x07',
            'b' => '\x08',
            'f' => '\x0c',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\x0b',
            '0'..='7' => {
                let mut value = c as u32 - '0' as u32;
                for _ in 0..2 {
                    match self.peek() {
                        Some(c @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(c - b'0');
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                // higher values are in the NeXTSTEP encoding
                if value >= 0x80 {
                    return Err(syntax_error(
                        start,
                        "octal escapes of non-ASCII characters are not supported",
                    ));
                }
                char::from(value as u8)
            }
            'U' => {
                let unit = self.unicode_escape(start)?;
                let c = if (0xd800..0xdc00).contains(&unit) {
                    // a high surrogate has to be followed by a low one
                    let low = if self.src.as_bytes()[self.pos..].starts_with(b"\\U") {
                        self.pos += 2;
                        self.unicode_escape(start)?
                    } else {
                        0
                    };
                    if !(0xdc00..0xe000).contains(&low) {
                        return Err(syntax_error(start, "unpaired surrogate in escape"));
                    }
                    char::from_u32(0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00))
                } else {
                    char::from_u32(unit)
                };
                c.ok_or_else(|| syntax_error(start, "unpaired surrogate in escape"))?
            }
            // all other characters stand for themselves
            c => c,
        };
        buf.push(c);
        Ok(())
    }

    /// Reads the up to four hex digits of a `\U` escape.
    fn unicode_escape(&mut self, start: usize) -> Result<u32, Error> {
        let digits = self.src.as_bytes()[self.pos..]
            .iter()
            .take(4)
            .take_while(|c| c.is_ascii_hexdigit())
            .count();
        if digits == 0 {
            return Err(syntax_error(start, "invalid unicode escape"));
        }
        let value = u32::from_str_radix(&self.src[self.pos..self.pos + digits], 16).unwrap();
        self.pos += digits;
        Ok(value)
    }
}

/// Emits a string as lexical atom, borrowed if possible.
fn emit_text<'i, O: Out<'i>>(
    out: &mut O,
    start: usize,
    end: usize,
    text: Cow<'i, str>,
) -> Result<(), Error> {
    match text {
        Cow::Borrowed(text) => {
            out.emit_input_at(start, end, Event::Atom(Atom::Lexical(Text::borrowed(text))))
        }
        Cow::Owned(text) => out.emit_at(start, end, Atom::Lexical(Text::owned(text))),
    }
}
