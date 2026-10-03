//! The parser of PHP's serialization format.
//!
//! A value is parsed twice: [`scan`] validates it and finds out which
//! arrays are lists (their keys are `0`, `1`, ... in order), then
//! [`emit`] emits its events.  Lists are sequences in the data model, but
//! that is only known once all keys of the array were read.  Scanning first
//! also means that malformed input fails before any event is emitted.
//!
//! Both passes use an explicit stack, deeply nested input does not
//! overflow the stack.
use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::str;

use deser_core::de::DeserializeDriver;
use deser_core::ext::ExtValue;
use deser_core::{
    Atom, Bytes, ContainerShape, Error, ErrorKind, Event, Implicit, ImplicitValue, Text,
};

use crate::float::{is_float, parse_float};
use crate::object::{ClassName, PropertyVisibility, Visibility};
use crate::reference::{Reference, ReferenceKind};

#[cold]
pub(crate) fn syntax_error(offset: usize, msg: &str) -> Error {
    Error::with_offset(ErrorKind::Syntax, format!("syntax error: {}", msg), offset)
}

#[cold]
fn eof_error(offset: usize) -> Error {
    Error::with_offset(ErrorKind::EndOfFile, "unexpected end of input", offset)
}

/// The head of a value: a scalar or the start of an array or object.
enum Head<'i> {
    Null,
    Bool(bool),
    /// An integer and its text.
    Int(i64, &'i [u8]),
    Float(f64),
    Str(&'i [u8]),
    /// A string with escape sequences (`S:`), decoded.
    EscapedStr(Vec<u8>),
    /// The start of an array with its number of entries.
    Array(usize),
    /// The start of an object with its class and number of properties.
    Object(&'i str, usize),
    /// A custom serialized object (`C:`): its class and payload.
    Custom(&'i str, &'i [u8]),
    /// An enum case: its class and case.
    Enum(&'i str, &'i [u8]),
    Reference(ReferenceKind, u64),
}

/// Reads the heads of values.
struct Lexer<'i> {
    input: &'i [u8],
    pos: usize,
}

impl<'i> Lexer<'i> {
    fn next_byte(&mut self) -> Result<u8, Error> {
        match self.input.get(self.pos) {
            Some(&byte) => {
                self.pos += 1;
                Ok(byte)
            }
            None => Err(eof_error(self.pos)),
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), Error> {
        let pos = self.pos;
        if self.next_byte()? != expected {
            return Err(syntax_error(
                pos,
                &format!("expected '{}'", char::from(expected)),
            ));
        }
        Ok(())
    }

    /// Takes `len` bytes.
    fn take(&mut self, len: usize) -> Result<&'i [u8], Error> {
        match self.pos.checked_add(len) {
            Some(end) if end <= self.input.len() => {
                let bytes = &self.input[self.pos..end];
                self.pos = end;
                Ok(bytes)
            }
            _ => Err(eof_error(self.input.len())),
        }
    }

    /// Takes the digits (and with `signed` a sign in front of them).
    fn number_text(&mut self, signed: bool) -> &'i [u8] {
        let start = self.pos;
        if signed && matches!(self.input.get(self.pos), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        while self.input.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        &self.input[start..self.pos]
    }

    /// Reads an unsigned number such as a length.
    fn unsigned(&mut self) -> Result<u64, Error> {
        let pos = self.pos;
        let text = self.number_text(false);
        if text.is_empty() {
            return Err(match self.input.get(self.pos) {
                None => eof_error(self.pos),
                Some(_) => syntax_error(pos, "expected a number"),
            });
        }
        parse_ascii::<u64>(text).ok_or_else(|| syntax_error(pos, "number out of range"))
    }

    /// Reads a length.
    fn length(&mut self) -> Result<usize, Error> {
        let pos = self.pos;
        usize::try_from(self.unsigned()?).map_err(|_| syntax_error(pos, "length out of range"))
    }

    /// Reads a quoted string of a length, `"..."`.
    fn quoted(&mut self, len: usize) -> Result<&'i [u8], Error> {
        self.expect(b'"')?;
        let bytes = self.take(len)?;
        self.expect(b'"')?;
        Ok(bytes)
    }

    /// Expects the end of an array or object.
    fn close(&mut self) -> Result<usize, Error> {
        let pos = self.pos;
        self.expect(b'}')?;
        Ok(pos)
    }

    /// Reads the head of a value.
    fn head(&mut self) -> Result<Head<'i>, Error> {
        let start = self.pos;
        let tag = self.next_byte()?;
        if tag == b'N' {
            self.expect(b';')?;
            return Ok(Head::Null);
        }
        if !b"bidsSaOCErR".contains(&tag) {
            return Err(syntax_error(start, "unknown type"));
        }
        self.expect(b':')?;
        let head = match tag {
            b'b' => {
                let pos = self.pos;
                let value = match self.next_byte()? {
                    b'0' => false,
                    b'1' => true,
                    _ => return Err(syntax_error(pos, "invalid boolean")),
                };
                self.expect(b';')?;
                Head::Bool(value)
            }
            b'i' => {
                let pos = self.pos;
                let text = self.number_text(true);
                if !text.last().is_some_and(u8::is_ascii_digit) {
                    return Err(syntax_error(pos, "invalid integer"));
                }
                let value = parse_ascii::<i64>(text)
                    .ok_or_else(|| syntax_error(pos, "integer out of range"))?;
                self.expect(b';')?;
                Head::Int(value, text)
            }
            b'd' => {
                let pos = self.pos;
                let len = self.input[pos..]
                    .iter()
                    .position(|&c| c == b';')
                    .ok_or_else(|| eof_error(self.input.len()))?;
                let text = &self.input[pos..pos + len];
                if !is_float(text) {
                    return Err(syntax_error(pos, "invalid float"));
                }
                self.pos = pos + len + 1;
                Head::Float(parse_float(text))
            }
            b's' => {
                let len = self.length()?;
                self.expect(b':')?;
                let bytes = self.quoted(len)?;
                self.expect(b';')?;
                Head::Str(bytes)
            }
            b'S' => {
                let len = self.length()?;
                self.expect(b':')?;
                self.expect(b'"')?;
                let mut bytes = Vec::with_capacity(len.min(self.input.len() - self.pos));
                for _ in 0..len {
                    let pos = self.pos;
                    match self.next_byte()? {
                        b'\\' => {
                            let hex = self.take(2)?;
                            let value = str::from_utf8(hex)
                                .ok()
                                .filter(|hex| hex.bytes().all(|c| c.is_ascii_hexdigit()))
                                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                                .ok_or_else(|| syntax_error(pos, "invalid escape sequence"))?;
                            bytes.push(value);
                        }
                        byte => bytes.push(byte),
                    }
                }
                self.expect(b'"')?;
                self.expect(b';')?;
                Head::EscapedStr(bytes)
            }
            b'a' => {
                let len = self.length()?;
                self.expect(b':')?;
                self.expect(b'{')?;
                Head::Array(len)
            }
            b'O' => {
                let class = self.class()?;
                self.expect(b':')?;
                // unlike other counts this one can have a sign
                let pos = self.pos;
                let text = self.number_text(true);
                let (negative, digits) = match text {
                    [b'-', digits @ ..] => (true, digits),
                    [b'+', digits @ ..] => (false, digits),
                    digits => (false, digits),
                };
                // and it can be empty
                let len = match digits.is_empty() {
                    true => Some(0),
                    false => parse_ascii::<usize>(digits),
                };
                let len = len
                    .filter(|&len| !negative || len == 0)
                    .ok_or_else(|| syntax_error(pos, "invalid number of properties"))?;
                self.expect(b':')?;
                self.expect(b'{')?;
                Head::Object(class, len)
            }
            b'C' => {
                let class = self.class()?;
                self.expect(b':')?;
                let len = self.length()?;
                self.expect(b':')?;
                self.expect(b'{')?;
                let payload = self.take(len)?;
                self.expect(b'}')?;
                Head::Custom(class, payload)
            }
            b'E' => {
                let len = self.length()?;
                self.expect(b':')?;
                let pos = self.pos + 1;
                let text = self.quoted(len)?;
                self.expect(b';')?;
                let colon = text
                    .iter()
                    .position(|&c| c == b':')
                    .ok_or_else(|| syntax_error(pos, "enum case without class"))?;
                let (class, case) = (&text[..colon], &text[colon + 1..]);
                if !is_class_name(class) || !is_name(case) {
                    return Err(syntax_error(pos, "invalid enum case"));
                }
                Head::Enum(utf8_class(class, pos)?, case)
            }
            b'r' | b'R' => {
                let number = self.unsigned()?;
                self.expect(b';')?;
                let kind = match tag {
                    b'r' => ReferenceKind::Object,
                    _ => ReferenceKind::Value,
                };
                Head::Reference(kind, number)
            }
            _ => unreachable!(),
        };
        Ok(head)
    }

    /// Reads the class of an object, `len:"Class"`.
    fn class(&mut self) -> Result<&'i str, Error> {
        let len = self.length()?;
        self.expect(b':')?;
        let pos = self.pos + 1;
        let class = self.quoted(len)?;
        if !is_class_name(class) {
            return Err(syntax_error(pos, "invalid class name"));
        }
        utf8_class(class, pos)
    }
}

/// Returns a class name as text.
///
/// PHP allows bytes from 0x80 on in names, classes are only supported if
/// their names are valid UTF-8.
fn utf8_class(class: &[u8], pos: usize) -> Result<&str, Error> {
    str::from_utf8(class).map_err(|_| {
        Error::with_offset(
            ErrorKind::UnsupportedType,
            "class names that are not valid UTF-8 are not supported",
            pos,
        )
    })
}

/// Parses ASCII digits (with an optional sign).
fn parse_ascii<T: str::FromStr>(text: &[u8]) -> Option<T> {
    str::from_utf8(text).ok()?.parse().ok()
}

/// Returns `true` if the bytes are a name of PHP (of a class or a case).
///
/// Names consist of ASCII letters, digits, underscores and bytes from 0x80
/// on.
pub(crate) fn is_name(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80)
}

/// Returns `true` if the bytes are a class name: names separated by
/// backslashes.
pub(crate) fn is_class_name(bytes: &[u8]) -> bool {
    bytes.split(|&c| c == b'\\').all(is_name)
}

/// Returns the integer of a key that is a string.
///
/// PHP turns keys that are the canonical text of an integer into
/// integers: `"5"` is the key `5` while `"05"` and `"-0"` stay strings.
pub(crate) fn int_key(text: &[u8]) -> Option<i64> {
    let digits = text.strip_prefix(b"-").unwrap_or(text);
    match digits {
        [b'0'] if digits.len() == text.len() => Some(0),
        [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit) => parse_ascii(text),
        _ => None,
    }
}

/// An open array or object of the scan.
struct ScanFrame {
    /// The number of entries that are still to come.
    remaining: usize,
    /// For arrays that are lists so far: the index in the list flags.
    list: Option<usize>,
    /// The key the next entry needs for the array to be a list.
    next: i64,
}

/// The result of scanning a value.
pub(crate) struct Scan {
    /// The offset after the value.
    pub(crate) end: usize,
    /// For every array (in the order they start): if it's a list.
    pub(crate) lists: Vec<bool>,
}

/// Validates a value and returns which of its arrays are lists.
pub(crate) fn scan(input: &[u8], start: usize) -> Result<Scan, Error> {
    let mut lexer = Lexer { input, pos: start };
    let mut lists = Vec::new();
    // for every value with a number: if it's an object (the target of `r:`)
    let mut objects = Vec::new();
    let mut stack = Vec::new();
    scan_value(&mut lexer, &mut lists, &mut objects, &mut stack)?;
    while let Some(frame) = stack.last_mut() {
        if frame.remaining == 0 {
            lexer.close()?;
            stack.pop();
            continue;
        }
        frame.remaining -= 1;
        let pos = lexer.pos;
        let key = match lexer.head()? {
            Head::Int(value, _) => Some(value),
            Head::Str(text) => int_key(text),
            Head::EscapedStr(ref text) => int_key(text),
            _ => return Err(syntax_error(pos, "keys must be integers or strings")),
        };
        if let Some(list) = frame.list {
            if key == Some(frame.next) {
                frame.next += 1;
            } else {
                lists[list] = false;
                frame.list = None;
            }
        }
        scan_value(&mut lexer, &mut lists, &mut objects, &mut stack)?;
    }
    Ok(Scan {
        end: lexer.pos,
        lists,
    })
}

fn scan_value(
    lexer: &mut Lexer<'_>,
    lists: &mut Vec<bool>,
    objects: &mut Vec<bool>,
    stack: &mut Vec<ScanFrame>,
) -> Result<(), Error> {
    let pos = lexer.pos;
    match lexer.head()? {
        Head::Reference(kind, number) => {
            // references refer to an earlier value, `r:` to an object.
            // `r:` has a number itself, `R:` does not.
            let target = usize::try_from(number)
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|index| objects.get(index));
            match (kind, target) {
                (ReferenceKind::Object, Some(true)) => objects.push(true),
                (ReferenceKind::Value, Some(_)) => {}
                _ => return Err(syntax_error(pos, "invalid reference")),
            }
        }
        Head::Array(len) => {
            objects.push(false);
            stack.push(ScanFrame {
                remaining: len,
                list: Some(lists.len()),
                next: 0,
            });
            lists.push(true);
        }
        Head::Object(_, len) => {
            objects.push(true);
            stack.push(ScanFrame {
                remaining: len,
                list: None,
                next: 0,
            });
        }
        Head::Custom(..) | Head::Enum(..) => objects.push(true),
        _ => objects.push(false),
    }
    Ok(())
}

/// The kind of an open container of the emitter.
#[derive(Clone, Copy, PartialEq)]
enum Container {
    List,
    Array,
    Object,
}

/// An open array or object of the emitter.
struct EmitFrame {
    remaining: usize,
    container: Container,
}

/// Emits the events of a value that was scanned.
pub(crate) fn emit<'i>(
    input: &'i [u8],
    start: usize,
    lists: &[bool],
    driver: &mut DeserializeDriver<'_, 'i>,
) -> Result<(), Error> {
    let mut emitter = Emitter {
        lexer: Lexer { input, pos: start },
        lists: lists.iter(),
        stack: Vec::new(),
        driver,
    };
    emitter.value()?;
    while let Some(frame) = emitter.stack.last_mut() {
        if frame.remaining == 0 {
            let container = frame.container;
            emitter.stack.pop();
            let pos = emitter.lexer.close()?;
            emitter.driver.state_mut().set_input_range(pos, pos + 1);
            emitter.driver.emit(match container {
                Container::List => Event::SeqEnd,
                _ => Event::MapEnd,
            })?;
            continue;
        }
        frame.remaining -= 1;
        let container = frame.container;
        let pos = emitter.lexer.pos;
        let key = emitter.lexer.head()?;
        if container != Container::List {
            let end = emitter.lexer.pos;
            emitter.driver.state_mut().set_input_range(pos, end);
            emitter.key(key, container == Container::Object)?;
        }
        emitter.value()?;
    }
    Ok(())
}

struct Emitter<'a, 'd, 'i, 'l> {
    lexer: Lexer<'i>,
    lists: core::slice::Iter<'l, bool>,
    stack: Vec<EmitFrame>,
    driver: &'a mut DeserializeDriver<'d, 'i>,
}

impl<'i> Emitter<'_, '_, 'i, '_> {
    fn value(&mut self) -> Result<(), Error> {
        let start = self.lexer.pos;
        let head = self.lexer.head()?;
        let end = self.lexer.pos;
        let driver = &mut *self.driver;
        driver.state_mut().set_input_range(start, end);
        match head {
            Head::Null => driver.emit(Atom::Null),
            Head::Bool(value) => driver.emit(Atom::Bool(value)),
            Head::Int(value, _) => driver.emit(int_atom(value)),
            Head::Float(value) => driver.emit(Atom::F64(value)),
            Head::Str(bytes) => driver.emit_borrowed(Event::Atom(string_atom(bytes))),
            Head::EscapedStr(bytes) => driver.emit(owned_string_atom(bytes)),
            Head::Array(len) => {
                // the scan saw the same arrays
                let is_list = *self.lists.next().unwrap();
                let mut shape = ContainerShape::with_len(len);
                // the empty array is an empty list and an empty map
                shape.set_ambiguous_empty(len == 0);
                self.stack.push(EmitFrame {
                    remaining: len,
                    container: if is_list {
                        Container::List
                    } else {
                        Container::Array
                    },
                });
                driver.emit(if is_list {
                    Event::SeqStart(shape)
                } else {
                    Event::MapStart(shape)
                })
            }
            Head::Object(class, len) => {
                set_class(driver, class);
                self.stack.push(EmitFrame {
                    remaining: len,
                    container: Container::Object,
                });
                driver.emit(Event::MapStart(ContainerShape::with_len(len)))
            }
            Head::Custom(class, payload) => {
                set_class(driver, class);
                driver.emit_borrowed(Event::Atom(Atom::Bytes(Bytes::borrowed(payload))))
            }
            Head::Enum(class, case) => {
                set_class(driver, class);
                driver.emit_borrowed(Event::Atom(string_atom(case)))
            }
            Head::Reference(kind, number) => {
                driver.emit(Atom::Ext(ExtValue::owned(Reference::new(kind, number))))
            }
        }
    }

    /// Emits a key.
    ///
    /// Integers (and strings that PHP turns into integers) are implicit
    /// atoms: they are integers for types that take them and their text
    /// for all others.  The names of protected and private properties of
    /// objects lose their prefix, which is passed on as visibility.
    fn key(&mut self, key: Head<'i>, is_object: bool) -> Result<(), Error> {
        let driver = &mut *self.driver;
        match key {
            Head::Int(value, text) => {
                // the text is borrowed if it's how PHP writes the integer
                let text = match int_key(text) {
                    Some(_) => Text::borrowed(str::from_utf8(text).unwrap()),
                    None => Text::owned(value.to_string()),
                };
                driver.emit_borrowed(Event::Atom(int_key_atom(text, value)))
            }
            Head::Str(bytes) => match int_key(bytes) {
                Some(value) => {
                    let text = Text::borrowed(str::from_utf8(bytes).unwrap());
                    driver.emit_borrowed(Event::Atom(int_key_atom(text, value)))
                }
                None => {
                    let name = match is_object {
                        true => demangle(bytes, driver),
                        false => bytes,
                    };
                    driver.emit_borrowed(Event::Atom(string_atom(name)))
                }
            },
            Head::EscapedStr(bytes) => match int_key(&bytes) {
                Some(value) => {
                    let text = Text::owned(String::from_utf8(bytes).unwrap());
                    driver.emit(int_key_atom(text, value))
                }
                None => {
                    let name = match is_object {
                        true => demangle(&bytes, driver),
                        false => &bytes,
                    };
                    driver.emit(owned_string_atom(name.to_vec()))
                }
            },
            // the scan only lets integers and strings through
            _ => unreachable!(),
        }
    }
}

/// Publishes the class of the next event.
fn set_class(driver: &mut DeserializeDriver<'_, '_>, class: &str) {
    driver.state_mut().event_mut::<ClassName>().0 = Some(class.to_string());
}

/// Removes the prefix of the name of a protected or private property and
/// publishes its visibility for the next event.
fn demangle<'b>(name: &'b [u8], driver: &mut DeserializeDriver<'_, '_>) -> &'b [u8] {
    let Some(rest) = name.strip_prefix(b"\0") else {
        return name;
    };
    let Some(end) = rest.iter().position(|&c| c == 0) else {
        return name;
    };
    let visibility = match &rest[..end] {
        b"*" => Visibility::Protected,
        class => match str::from_utf8(class) {
            Ok(class) if !class.is_empty() => Visibility::Private(class.to_string()),
            _ => return name,
        },
    };
    driver.state_mut().event_mut::<PropertyVisibility>().0 = Some(visibility);
    &rest[end + 1..]
}

fn int_atom(value: i64) -> Atom<'static> {
    match u64::try_from(value) {
        Ok(value) => Atom::U64(value),
        Err(_) => Atom::I64(value),
    }
}

fn int_key_atom(text: Text<'_>, value: i64) -> Atom<'_> {
    let value = match u64::try_from(value) {
        Ok(value) => ImplicitValue::U64(value),
        Err(_) => ImplicitValue::I64(value),
    };
    Atom::Implicit(Implicit::new(text, value))
}

/// Returns the atom of a string: text if it's UTF-8, bytes otherwise.
fn string_atom(bytes: &[u8]) -> Atom<'_> {
    match str::from_utf8(bytes) {
        Ok(text) => Atom::Str(Text::borrowed(text)),
        Err(_) => Atom::Bytes(Bytes::borrowed(bytes)),
    }
}

fn owned_string_atom(bytes: Vec<u8>) -> Atom<'static> {
    match String::from_utf8(bytes) {
        Ok(text) => Atom::Str(Text::owned(text)),
        Err(err) => Atom::Bytes(Bytes::new(Cow::Owned(err.into_bytes()))),
    }
}
