//! The reader of XML property lists.
//!
//! This is not a general XML parser but understands the subset that
//! property lists use: elements with attributes (which are ignored), text
//! with entity and character references, CDATA sections, comments,
//! processing instructions and a document type declaration.  Namespaces
//! and custom entities are not supported.
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use deser_core::ext::{ExtValue, Timestamp};
use deser_core::{Atom, Bytes, ContainerShape, Error, Event, Text};

use crate::common::{Out, decode_base64, eof_error, syntax_error};
use crate::uid::Uid;

/// A tag of an element.
#[derive(Clone, Copy)]
enum Tag<'i> {
    Start {
        name: &'i str,
        /// `true` for empty elements (`<true/>`).
        empty: bool,
        start: usize,
    },
    End {
        name: &'i str,
        start: usize,
    },
}

/// An open array or dictionary.
#[derive(Clone, Copy)]
enum Frame {
    Array,
    /// `true` if the key of an entry was read.
    Dict(bool),
}

struct Reader<'i> {
    src: &'i str,
    pos: usize,
    stack: Vec<Frame>,
}

/// Parses an XML property list and emits its events.
pub(crate) fn parse<'i, O: Out<'i>>(input: &'i str, out: &mut O) -> Result<(), Error> {
    let mut reader = Reader {
        src: input,
        pos: 0,
        stack: Vec::new(),
    };
    reader.document(out)
}

impl<'i> Reader<'i> {
    fn document<O: Out<'i>>(&mut self, out: &mut O) -> Result<(), Error> {
        self.prolog()?;
        let tag = self.expect_tag()?;
        match tag {
            Tag::Start {
                name: "plist",
                empty,
                start,
            } => {
                if empty {
                    return Err(syntax_error(start, "empty plist"));
                }
                let tag = self.expect_tag()?;
                if let Tag::End { name: "plist", .. } = tag {
                    return Err(syntax_error(start, "empty plist"));
                }
                self.value(tag, out)?;
                match self.expect_tag()? {
                    Tag::End { name: "plist", .. } => {}
                    Tag::Start { start, .. } => {
                        return Err(syntax_error(start, "a plist can only hold a single value"));
                    }
                    Tag::End { start, .. } => {
                        return Err(syntax_error(start, "unexpected closing tag"));
                    }
                }
            }
            // Core Foundation accepts documents without `<plist>`
            tag => self.value(tag, out)?,
        }
        self.skip_misc()?;
        if self.pos < self.src.len() {
            return Err(syntax_error(self.pos, "unexpected content after the plist"));
        }
        Ok(())
    }

    /// Parses a value starting with the given tag.
    fn value<O: Out<'i>>(&mut self, tag: Tag<'i>, out: &mut O) -> Result<(), Error> {
        self.open(tag, out)?;
        while let Some(&frame) = self.stack.last() {
            let tag = self.expect_tag()?;
            match (frame, tag) {
                (
                    Frame::Array,
                    Tag::End {
                        name: "array",
                        start,
                    },
                ) => {
                    self.stack.pop();
                    out.emit_at(start, self.pos, Event::SeqEnd)?;
                }
                (Frame::Array, Tag::Start { .. }) => self.open(tag, out)?,
                (
                    Frame::Dict(false),
                    Tag::End {
                        name: "dict",
                        start,
                    },
                ) => {
                    self.stack.pop();
                    out.emit_at(start, self.pos, Event::MapEnd)?;
                }
                (
                    Frame::Dict(false),
                    Tag::Start {
                        name: "key",
                        empty,
                        start,
                    },
                ) => {
                    let key = if empty {
                        Cow::Borrowed("")
                    } else {
                        self.text("key")?
                    };
                    emit_text(out, start, self.pos, key, Atom::Lexical)?;
                    *self.stack.last_mut().unwrap() = Frame::Dict(true);
                }
                (Frame::Dict(false), Tag::Start { start, .. }) => {
                    return Err(syntax_error(start, "expected <key> in <dict>"));
                }
                (Frame::Dict(true), Tag::Start { .. }) => {
                    *self.stack.last_mut().unwrap() = Frame::Dict(false);
                    self.open(tag, out)?;
                }
                (Frame::Dict(true), Tag::End { start, .. }) => {
                    return Err(syntax_error(start, "missing value for key in <dict>"));
                }
                (_, Tag::End { start, .. }) => {
                    return Err(syntax_error(start, "closing tag does not match"));
                }
            }
        }
        Ok(())
    }

    /// Emits the value of an element.  Arrays and dictionaries are pushed
    /// to the stack.
    fn open<O: Out<'i>>(&mut self, tag: Tag<'i>, out: &mut O) -> Result<(), Error> {
        let (name, empty, start) = match tag {
            Tag::Start { name, empty, start } => (name, empty, start),
            Tag::End { start, .. } => {
                return Err(syntax_error(start, "unexpected closing tag"));
            }
        };
        match name {
            "dict" | "array" => {
                let is_dict = name == "dict";
                if is_dict
                    && !empty
                    && let Some(uid) = self.try_uid()
                {
                    return out.emit_at(start, self.pos, Atom::Ext(ExtValue::owned(uid)));
                }
                let shape = if empty {
                    ContainerShape::with_len(0)
                } else {
                    ContainerShape::new()
                };
                out.emit_at(
                    start,
                    self.pos,
                    if is_dict {
                        Event::MapStart(shape)
                    } else {
                        Event::SeqStart(shape)
                    },
                )?;
                if empty {
                    out.emit_at(
                        self.pos,
                        self.pos,
                        if is_dict {
                            Event::MapEnd
                        } else {
                            Event::SeqEnd
                        },
                    )?;
                } else {
                    self.stack.push(if is_dict {
                        Frame::Dict(false)
                    } else {
                        Frame::Array
                    });
                }
                Ok(())
            }
            "true" | "false" => {
                if !empty && !self.text(name)?.is_empty() {
                    return Err(syntax_error(start, "unexpected content in boolean"));
                }
                out.emit_at(start, self.pos, Atom::Bool(name == "true"))
            }
            "string" => {
                let text = if empty {
                    Cow::Borrowed("")
                } else {
                    self.text(name)?
                };
                emit_text(out, start, self.pos, text, Atom::Str)
            }
            "integer" => {
                let text = self.content(name, empty)?;
                let value =
                    parse_integer(&text).ok_or_else(|| syntax_error(start, "invalid integer"))?;
                let atom = if let Ok(value) = u64::try_from(value) {
                    Atom::U64(value)
                } else {
                    Atom::I64(value as i64)
                };
                out.emit_at(start, self.pos, atom)
            }
            "real" => {
                let text = self.content(name, empty)?;
                let value = text
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| syntax_error(start, "invalid real"))?;
                out.emit_at(start, self.pos, Atom::F64(value))
            }
            "date" => {
                let text = self.content(name, empty)?;
                let value = text
                    .trim()
                    .parse::<Timestamp>()
                    .map_err(|_| syntax_error(start, "invalid date"))?;
                out.emit_at(start, self.pos, Atom::Ext(ExtValue::borrowed(&value)))
            }
            "data" => {
                let text = self.content(name, empty)?;
                let data =
                    decode_base64(&text).map_err(|_| syntax_error(start, "invalid base64 data"))?;
                out.emit_at(start, self.pos, Atom::Bytes(Bytes::new(data)))
            }
            "key" => Err(syntax_error(start, "<key> outside of <dict>")),
            _ => Err(syntax_error(start, "unknown element")),
        }
    }

    /// Reads the content of an element (empty if the element is empty).
    fn content(&mut self, name: &str, empty: bool) -> Result<Cow<'i, str>, Error> {
        if empty {
            Ok(Cow::Borrowed(""))
        } else {
            self.text(name)
        }
    }

    /// Checks if the dictionary that was just opened is a UID
    /// (`<dict><key>CF$UID</key><integer>1</integer></dict>`).
    ///
    /// If it is not, the reader is reset to the start of its content.
    fn try_uid(&mut self) -> Option<Uid> {
        let pos = self.pos;
        let rv = self.try_uid_inner();
        if rv.is_none() {
            self.pos = pos;
        }
        rv
    }

    fn try_uid_inner(&mut self) -> Option<Uid> {
        let Ok(Some(Tag::Start {
            name: "key",
            empty: false,
            ..
        })) = self.next_tag()
        else {
            return None;
        };
        if self.text("key").ok()? != "CF$UID" {
            return None;
        }
        let Ok(Some(Tag::Start {
            name: "integer",
            empty: false,
            ..
        })) = self.next_tag()
        else {
            return None;
        };
        let value = u64::try_from(parse_integer(&self.text("integer").ok()?)?).ok()?;
        match self.next_tag() {
            Ok(Some(Tag::End { name: "dict", .. })) => Some(Uid::new(value)),
            _ => None,
        }
    }

    fn bytes(&self) -> &'i [u8] {
        &self.src.as_bytes()[self.pos..]
    }

    fn skip_ws(&mut self) {
        let len = self
            .bytes()
            .iter()
            .position(|c| !c.is_ascii_whitespace())
            .unwrap_or(self.bytes().len());
        self.pos += len;
    }

    /// Skips to after the given terminator.
    fn skip_past(&mut self, terminator: &str, what: &str) -> Result<(), Error> {
        let start = self.pos;
        match self.src[self.pos..].find(terminator) {
            Some(idx) => {
                self.pos += idx + terminator.len();
                Ok(())
            }
            None => Err(syntax_error(start, what)),
        }
    }

    /// Skips whitespace, comments and processing instructions.
    fn skip_misc(&mut self) -> Result<(), Error> {
        loop {
            self.skip_ws();
            let rest = self.bytes();
            if rest.starts_with(b"<!--") {
                self.skip_past("-->", "unterminated comment")?;
            } else if rest.starts_with(b"<?") {
                self.skip_past("?>", "unterminated processing instruction")?;
            } else {
                return Ok(());
            }
        }
    }

    /// Skips the XML declaration, the document type declaration and
    /// comments before the document.
    fn prolog(&mut self) -> Result<(), Error> {
        self.skip_misc()?;
        if !self.bytes().starts_with(b"<!DOCTYPE") {
            return Ok(());
        }
        let start = self.pos;
        let mut depth = 0;
        let mut quote = None;
        for (idx, &c) in self.bytes().iter().enumerate() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(c),
                (None, b'[') => depth += 1,
                (None, b']') => depth -= 1,
                (None, b'>') if depth == 0 => {
                    self.pos += idx + 1;
                    return self.skip_misc();
                }
                _ => {}
            }
        }
        Err(syntax_error(
            start,
            "unterminated document type declaration",
        ))
    }

    /// Reads the next tag, failing at the end of the input.
    fn expect_tag(&mut self) -> Result<Tag<'i>, Error> {
        self.next_tag()?.ok_or_else(|| eof_error(self.src.len()))
    }

    /// Reads the next tag.  Only whitespace and comments can come before.
    fn next_tag(&mut self) -> Result<Option<Tag<'i>>, Error> {
        self.skip_misc()?;
        let start = self.pos;
        let rest = self.bytes();
        if rest.is_empty() {
            return Ok(None);
        }
        if rest[0] != b'<' || rest.starts_with(b"<!") {
            return Err(syntax_error(start, "unexpected content, expected a tag"));
        }
        if rest.starts_with(b"</") {
            self.pos += 2;
            let name = self.name()?;
            self.skip_ws();
            if !self.bytes().starts_with(b">") {
                return Err(syntax_error(self.pos, "expected `>`"));
            }
            self.pos += 1;
            return Ok(Some(Tag::End { name, start }));
        }
        self.pos += 1;
        let name = self.name()?;
        // attributes are skipped
        let mut quote = None;
        for (idx, &c) in self.bytes().iter().enumerate() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(c),
                (None, b'>') => {
                    let empty = idx > 0 && self.bytes()[idx - 1] == b'/';
                    self.pos += idx + 1;
                    return Ok(Some(Tag::Start { name, empty, start }));
                }
                (None, b'<') => break,
                _ => {}
            }
        }
        Err(syntax_error(start, "unterminated tag"))
    }

    /// Reads the name of a tag.
    fn name(&mut self) -> Result<&'i str, Error> {
        let len = self
            .bytes()
            .iter()
            .position(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-')))
            .unwrap_or(self.bytes().len());
        if len == 0 {
            return Err(syntax_error(self.pos, "expected a tag name"));
        }
        let name = &self.src[self.pos..self.pos + len];
        self.pos += len;
        Ok(name)
    }

    /// Reads the text content of an element and its closing tag.
    ///
    /// The text is borrowed if it does not contain references or CDATA
    /// sections.
    fn text(&mut self, name: &str) -> Result<Cow<'i, str>, Error> {
        let mut owned: Option<String> = None;
        let mut chunk_start = self.pos;
        loop {
            let Some(idx) = self.bytes().iter().position(|&c| c == b'<' || c == b'&') else {
                return Err(eof_error(self.src.len()));
            };
            self.pos += idx;
            let chunk = &self.src[chunk_start..self.pos];
            let rest = self.bytes();
            if rest[0] == b'&' {
                let buf = owned.get_or_insert_with(String::new);
                buf.push_str(chunk);
                self.reference(buf)?;
            } else if rest.starts_with(b"<![CDATA[") {
                let buf = owned.get_or_insert_with(String::new);
                buf.push_str(chunk);
                let start = self.pos;
                self.pos += 9;
                let Some(end) = self.src[self.pos..].find("]]>") else {
                    return Err(syntax_error(start, "unterminated CDATA section"));
                };
                buf.push_str(&self.src[self.pos..self.pos + end]);
                self.pos += end + 3;
            } else if rest.starts_with(b"</") {
                let tag_start = self.pos;
                match self.next_tag()? {
                    Some(Tag::End { name: end, .. }) if end == name => {}
                    _ => return Err(syntax_error(tag_start, "closing tag does not match")),
                }
                return Ok(match owned {
                    Some(mut buf) => {
                        buf.push_str(chunk);
                        Cow::Owned(buf)
                    }
                    None => Cow::Borrowed(chunk),
                });
            } else {
                return Err(syntax_error(self.pos, "unexpected markup in text"));
            }
            chunk_start = self.pos;
        }
    }

    /// Decodes an entity or character reference.
    fn reference(&mut self, buf: &mut String) -> Result<(), Error> {
        let start = self.pos;
        let invalid = || syntax_error(start, "invalid reference");
        let len = self
            .bytes()
            .iter()
            .position(|&c| c == b';')
            .ok_or_else(invalid)?;
        let reference = &self.src[self.pos + 1..self.pos + len];
        let c = match reference {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = reference
                    .strip_prefix("#x")
                    .or_else(|| reference.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = reference.strip_prefix('#') {
                    dec.parse::<u32>().ok()
                } else {
                    None
                };
                code.filter(|&c| c != 0)
                    .and_then(char::from_u32)
                    .ok_or_else(invalid)?
            }
        };
        buf.push(c);
        self.pos += len + 1;
        Ok(())
    }
}

/// Emits text, borrowed if possible.
fn emit_text<'i, O: Out<'i>>(
    out: &mut O,
    start: usize,
    end: usize,
    text: Cow<'i, str>,
    make: fn(Text<'i>) -> Atom<'i>,
) -> Result<(), Error> {
    match text {
        Cow::Borrowed(text) => {
            out.emit_input_at(start, end, Event::Atom(make(Text::borrowed(text))))
        }
        Cow::Owned(text) => out.emit_at(start, end, make(Text::owned(text))),
    }
}

/// Parses an integer: decimal or hexadecimal (`0x`) with an optional
/// sign.  The value has to be in the range of `i64` or `u64`.
fn parse_integer(text: &str) -> Option<i128> {
    let text = text.trim();
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let magnitude = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) if hex.bytes().all(|c| c.is_ascii_hexdigit()) => {
            u128::from_str_radix(hex, 16).ok()?
        }
        None if digits.bytes().all(|c| c.is_ascii_digit()) => digits.parse::<u128>().ok()?,
        _ => return None,
    };
    let value = if negative {
        -i128::try_from(magnitude).ok()?
    } else {
        i128::try_from(magnitude).ok()?
    };
    (i128::from(i64::MIN)..=i128::from(u64::MAX))
        .contains(&value)
        .then_some(value)
}

#[test]
fn test_parse_integer() {
    assert_eq!(parse_integer("42"), Some(42));
    assert_eq!(parse_integer(" -42\n"), Some(-42));
    assert_eq!(parse_integer("+42"), Some(42));
    assert_eq!(parse_integer("0xDEADBEEF"), Some(0xdeadbeef));
    assert_eq!(parse_integer("-0x10"), Some(-16));
    assert_eq!(parse_integer("18446744073709551615"), Some(u64::MAX.into()));
    assert_eq!(parse_integer("18446744073709551616"), None);
    assert_eq!(parse_integer("-9223372036854775808"), Some(i64::MIN.into()));
    assert_eq!(parse_integer("-9223372036854775809"), None);
    assert_eq!(parse_integer(""), None);
    assert_eq!(parse_integer("-"), None);
    assert_eq!(parse_integer("0x"), None);
    assert_eq!(parse_integer("+-1"), None);
    assert_eq!(parse_integer("1.0"), None);
}
