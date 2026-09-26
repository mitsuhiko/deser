use std::borrow::Cow;
use std::fmt::Write;

use deser::bytes::BytesFormat;
use deser::ext::ExtValue;
use deser::ser::SerializeDriver;
use deser::{Atom, Descriptor, Error, ErrorKind, Event, Serialize};

use crate::document::{Document, Entry, Item, Span, TableKind, Value};
use deser::ext::{Datetime, Number, Timestamp};

/// Configures how values are serialized to TOML.
///
/// The value has to serialize to a map (for instance a struct or a map
/// type) as TOML documents are tables.  As TOML has no null value, map
/// entries with null values (such as `None`) are skipped, null values in
/// sequences are an error.
///
/// Values that are maps are written as `[table]` sections and sequences of
/// maps as `[[array]]` sections unless they are nested in other sequences.
/// The output is compatible with TOML 1.0.
///
/// [`to_string`](Self::to_string) works like the
/// [`to_string`](crate::to_string) function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerializerConfig {
    bytes: BytesFormat,
}

impl SerializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> SerializerConfig {
        SerializerConfig {
            bytes: BytesFormat::BASE64,
        }
    }

    /// Sets how bytes are represented.
    ///
    /// TOML has no bytes, by default they are written as base64 strings
    /// ([`BytesFormat::BASE64`]).  Values can request a different format
    /// (see [`deser::bytes`]) which takes precedence.  Keys cannot be
    /// arrays, bytes in keys are always strings.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser::bytes::{BytesFormat, Hex};
    /// use deser_toml::SerializerConfig;
    ///
    /// let mut value = BTreeMap::new();
    /// value.insert("a", vec![1u8, 255]);
    /// assert_eq!(deser_toml::to_string(&value).unwrap(), "a = \"Af8=\"\n");
    /// const HEX: SerializerConfig = SerializerConfig::new().bytes(BytesFormat::encoded::<Hex>());
    /// assert_eq!(HEX.to_string(&value).unwrap(), "a = \"01ff\"\n");
    /// const SEQ: SerializerConfig = SerializerConfig::new().bytes(BytesFormat::SEQ);
    /// assert_eq!(SEQ.to_string(&value).unwrap(), "a = [1, 255]\n");
    /// ```
    ///
    /// Bytes in other formats than base64 (or arrays) need to be
    /// deserialized with the same format (see
    /// [`DeserializerConfig::bytes`](crate::DeserializerConfig::bytes)).
    pub const fn bytes(mut self, format: BytesFormat) -> SerializerConfig {
        self.bytes = format;
        self
    }

    /// Serializes the given value.
    pub fn to_string(&self, value: &dyn Serialize) -> Result<String, Error> {
        self.to_string_with(value, |_| {})
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser::ser::Layer)s.
    pub fn to_string_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<String, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut builder = Builder {
            doc: Document::default(),
            stack: Vec::new(),
            done: false,
            bytes: self.bytes,
        };
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        driver.drive(|event, descriptor, _state| builder.event(event, descriptor))?;
        if !builder.done {
            return Err(Error::new(ErrorKind::Unexpected, "no value was serialized"));
        }
        let mut writer = Writer {
            doc: &builder.doc,
            out: String::new(),
        };
        writer.write_document()?;
        Ok(writer.out)
    }
}

/// Serializes a value to TOML.
///
/// This uses the default [`SerializerConfig`], see there for more
/// information.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let mut value = BTreeMap::new();
/// value.insert("name", vec!["a", "b"]);
/// assert_eq!(deser_toml::to_string(&value).unwrap(), "name = [\"a\", \"b\"]\n");
/// ```
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// A map or sequence that is being built.
enum Frame {
    /// A table with the pending key if the next event is a value.
    Table(usize, Option<String>),
    Array(usize),
}

/// Builds a document from serialization events.
struct Builder {
    doc: Document<'static>,
    stack: Vec<Frame>,
    done: bool,
    bytes: BytesFormat,
}

/// A value converted from an atom.
enum Converted {
    Value(Value<'static>),
    Null,
}

impl Builder {
    fn event(&mut self, event: Event, descriptor: &dyn Descriptor) -> Result<(), Error> {
        let Some(frame) = self.stack.last_mut() else {
            if self.done {
                return Err(Error::new(ErrorKind::Unexpected, "unexpected event"));
            }
            return match event {
                Event::MapStart => {
                    let id = self.doc.new_table(TableKind::Header, Span::default());
                    self.stack.push(Frame::Table(id, None));
                    Ok(())
                }
                _ => Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "TOML documents must be tables",
                )),
            };
        };

        match *frame {
            Frame::Table(_, ref mut key @ None) => match event {
                Event::Atom(atom) => {
                    *key = Some(key_to_string(atom, descriptor, self.bytes)?);
                    Ok(())
                }
                Event::MapEnd => {
                    self.stack.pop();
                    self.done = self.stack.is_empty();
                    Ok(())
                }
                _ => Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "TOML only supports strings and integers as keys",
                )),
            },
            Frame::Table(id, ref mut key @ Some(_)) => {
                let key = key.take().unwrap();
                let value = match self.value(event, descriptor)? {
                    Converted::Value(value) => value,
                    // map entries with null values are skipped
                    Converted::Null => return Ok(()),
                };
                if self.doc.find(id, &key).is_some() {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        format!("duplicate key '{}'", key),
                    ));
                }
                self.doc.insert(
                    id,
                    Entry {
                        key: Cow::Owned(key),
                        key_span: Span::default(),
                        item: Item {
                            value,
                            span: Span::default(),
                        },
                    },
                );
                Ok(())
            }
            Frame::Array(id) => {
                if event == Event::SeqEnd {
                    self.stack.pop();
                    return Ok(());
                }
                match self.value(event, descriptor)? {
                    Converted::Value(value) => {
                        self.doc.arrays[id].items.push(Item {
                            value,
                            span: Span::default(),
                        });
                        Ok(())
                    }
                    Converted::Null => Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "TOML does not support null values in arrays",
                    )),
                }
            }
        }
    }

    /// Converts the first event of a value.  Maps and sequences are pushed
    /// to the stack.
    fn value(&mut self, event: Event, descriptor: &dyn Descriptor) -> Result<Converted, Error> {
        match event {
            Event::Atom(Atom::Bytes(ref bytes)) => Ok(Converted::Value(
                self.bytes_value(bytes, descriptor.bytes_format().unwrap_or(self.bytes)),
            )),
            Event::Atom(atom) => convert_atom(atom, descriptor, self.bytes),
            Event::MapStart => {
                let id = self.doc.new_table(TableKind::Header, Span::default());
                self.stack.push(Frame::Table(id, None));
                Ok(Converted::Value(Value::Table(id)))
            }
            Event::SeqStart => {
                let id = self.doc.new_array(false, Span::default());
                self.stack.push(Frame::Array(id));
                Ok(Converted::Value(Value::Array(id)))
            }
            Event::MapEnd | Event::SeqEnd => {
                Err(Error::new(ErrorKind::Unexpected, "unexpected end event"))
            }
        }
    }

    /// Converts bytes into a string or an array of integers.
    fn bytes_value(&mut self, bytes: &[u8], format: BytesFormat) -> Value<'static> {
        match format.encode(bytes) {
            Some(encoded) => Value::Str(Cow::Owned(encoded)),
            None => {
                let id = self.doc.new_array(false, Span::default());
                self.doc.arrays[id]
                    .items
                    .extend(bytes.iter().map(|&byte| Item {
                        value: Value::Int(byte.into()),
                        span: Span::default(),
                    }));
                Value::Array(id)
            }
        }
    }
}

fn convert_atom(
    atom: Atom,
    descriptor: &dyn Descriptor,
    bytes: BytesFormat,
) -> Result<Converted, Error> {
    Ok(Converted::Value(match atom {
        Atom::Null => return Ok(Converted::Null),
        Atom::Bool(value) => Value::Bool(value),
        Atom::Str(value) => Value::Str(Cow::Owned(value.into_owned())),
        Atom::Char(value) => Value::Str(Cow::Owned(value.to_string())),
        Atom::U64(value) => match i64::try_from(value) {
            Ok(value) => Value::Int(value),
            Err(_) => Value::UInt(value),
        },
        Atom::I64(value) => Value::Int(value),
        Atom::F64(value) => {
            if descriptor.precision() == Some(32) {
                // keep the shortest representation of the f32
                Value::Float(format!("{:?}", value as f32).parse().unwrap_or(value))
            } else {
                Value::Float(value)
            }
        }
        // bytes are converted by the builder, this is reached for the
        // fallbacks of extension values which cannot be arrays.
        Atom::Bytes(value) => Value::Str(Cow::Owned(encode_str(&value, descriptor, bytes))),
        Atom::Ext(ref ext) => return convert_ext(ext, descriptor, bytes),
        _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
    }))
}

#[cold]
fn convert_ext(
    ext: &ExtValue,
    descriptor: &dyn Descriptor,
    bytes: BytesFormat,
) -> Result<Converted, Error> {
    if let Some(value) = ext.downcast_ref::<Datetime>() {
        if !value.is_valid() {
            return Err(Error::new(ErrorKind::Unexpected, "invalid datetime"));
        }
        return Ok(Converted::Value(Value::Datetime(*value)));
    }
    // numbers from text formats keep their text if it's a float, the syntax
    // of JSON floats is valid in TOML
    if let Some(value) = ext.downcast_value_ref::<Number>() {
        return Ok(Converted::Value(if value.is_integer() {
            Value::Float(value.value())
        } else {
            Value::FloatText(Cow::Owned(value.as_str().to_string()))
        }));
    }
    // instants are written as offset date-times in UTC if possible
    if let Some(value) = ext
        .downcast_ref::<Timestamp>()
        .and_then(|x| x.to_datetime())
    {
        return Ok(Converted::Value(Value::Datetime(value)));
    }
    let out_of_range = || Error::new(ErrorKind::OutOfRange, "integer out of range for TOML");
    if let Some(&value) = ext.downcast_ref::<u128>() {
        let value = u64::try_from(value).map_err(|_| out_of_range())?;
        return convert_atom(Atom::U64(value), descriptor, bytes);
    }
    if let Some(&value) = ext.downcast_ref::<i128>() {
        return if let Ok(value) = i64::try_from(value) {
            convert_atom(Atom::I64(value), descriptor, bytes)
        } else {
            let value = u64::try_from(value).map_err(|_| out_of_range())?;
            convert_atom(Atom::U64(value), descriptor, bytes)
        };
    }
    match ext.fallback() {
        Atom::Ext(_) => Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("TOML does not support {}", ext.name()),
        )),
        fallback => convert_atom(fallback, descriptor, bytes),
    }
}

/// Encodes bytes as string.
///
/// Strings are required (for keys), so bytes that would be arrays are base64.
fn encode_str(value: &[u8], descriptor: &dyn Descriptor, bytes: BytesFormat) -> String {
    descriptor
        .bytes_format()
        .unwrap_or(bytes)
        .encode(value)
        .or_else(|| BytesFormat::BASE64.encode(value))
        .unwrap_or_default()
}

fn key_to_string(
    atom: Atom,
    descriptor: &dyn Descriptor,
    bytes: BytesFormat,
) -> Result<String, Error> {
    Ok(match atom {
        Atom::Str(value) => value.into_owned(),
        Atom::Char(value) => value.to_string(),
        Atom::U64(value) => value.to_string(),
        Atom::I64(value) => value.to_string(),
        Atom::Bytes(value) => encode_str(&value, descriptor, bytes),
        Atom::Ext(ref ext) => {
            if let Some(value) = ext.downcast_ref::<u128>() {
                value.to_string()
            } else if let Some(value) = ext.downcast_ref::<i128>() {
                value.to_string()
            } else {
                match ext.fallback() {
                    Atom::Ext(_) => return Err(unsupported_key()),
                    fallback => return key_to_string(fallback, descriptor, bytes),
                }
            }
        }
        _ => return Err(unsupported_key()),
    })
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "TOML only supports strings and integers as keys",
    )
}

/// How a table is introduced.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Header {
    /// The root table.
    None,
    /// `[table]`
    Table,
    /// `[[table]]`
    ArrayTable,
}

/// A table section to write.
struct Section<'d> {
    id: usize,
    path: Vec<&'d str>,
    header: Header,
}

/// An inline array or table that is being written.
enum InlineFrame {
    Table(usize, usize),
    Array(usize, usize),
}

struct Writer<'d> {
    doc: &'d Document<'static>,
    out: String,
}

impl<'d> Writer<'d> {
    /// Returns `true` if the value is written as section rather than inline.
    fn is_section(&self, value: &Value) -> bool {
        match *value {
            Value::Table(_) => true,
            Value::Array(id) => self.is_array_of_tables(id),
            _ => false,
        }
    }

    fn is_array_of_tables(&self, id: usize) -> bool {
        let items = &self.doc.arrays[id].items;
        !items.is_empty() && items.iter().all(|x| matches!(x.value, Value::Table(_)))
    }

    fn write_document(&mut self) -> Result<(), Error> {
        let doc = self.doc;
        let mut sections = vec![Section {
            id: 0,
            path: Vec::new(),
            header: Header::None,
        }];

        while let Some(section) = sections.pop() {
            let table = &doc.tables[section.id];
            let has_values = table
                .entries
                .iter()
                .any(|x| !self.is_section(&x.item.value));

            // tables which only contain other tables do not need a header,
            // they are created implicitly by the headers of their children.
            let header = match section.header {
                Header::Table if !has_values && !table.entries.is_empty() => None,
                Header::Table => Some(("[", "]")),
                Header::ArrayTable => Some(("[[", "]]")),
                Header::None => None,
            };
            if let Some((open, close)) = header {
                if !self.out.is_empty() {
                    self.out.push('\n');
                }
                self.out.push_str(open);
                for (idx, key) in section.path.iter().enumerate() {
                    if idx > 0 {
                        self.out.push('.');
                    }
                    write_key(&mut self.out, key);
                }
                self.out.push_str(close);
                self.out.push('\n');
            }

            for entry in &table.entries {
                if !self.is_section(&entry.item.value) {
                    write_key(&mut self.out, &entry.key);
                    self.out.push_str(" = ");
                    self.write_value(&entry.item.value)?;
                    self.out.push('\n');
                }
            }

            // the sections are processed from the end of the stack
            let first_child = sections.len();
            for entry in &table.entries {
                let child_path = || {
                    let mut path = section.path.clone();
                    path.push(&*entry.key);
                    path
                };
                match entry.item.value {
                    Value::Table(id) => sections.push(Section {
                        id,
                        path: child_path(),
                        header: Header::Table,
                    }),
                    Value::Array(id) if self.is_array_of_tables(id) => {
                        for item in &doc.arrays[id].items {
                            if let Value::Table(id) = item.value {
                                sections.push(Section {
                                    id,
                                    path: child_path(),
                                    header: Header::ArrayTable,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            sections[first_child..].reverse();
        }

        Ok(())
    }

    /// Writes a value inline.
    fn write_value(&mut self, value: &Value) -> Result<(), Error> {
        let doc = self.doc;
        let mut stack = Vec::new();
        self.write_value_start(value, &mut stack);

        while let Some(frame) = stack.last_mut() {
            let value = match *frame {
                InlineFrame::Table(id, ref mut index) => {
                    let table = &doc.tables[id];
                    match table.entries.get(*index) {
                        Some(entry) => {
                            self.out.push_str(if *index == 0 { " " } else { ", " });
                            *index += 1;
                            write_key(&mut self.out, &entry.key);
                            self.out.push_str(" = ");
                            &entry.item.value
                        }
                        None => {
                            self.out
                                .push_str(if table.entries.is_empty() { "}" } else { " }" });
                            stack.pop();
                            continue;
                        }
                    }
                }
                InlineFrame::Array(id, ref mut index) => match doc.arrays[id].items.get(*index) {
                    Some(item) => {
                        if *index > 0 {
                            self.out.push_str(", ");
                        }
                        *index += 1;
                        &item.value
                    }
                    None => {
                        self.out.push(']');
                        stack.pop();
                        continue;
                    }
                },
            };
            self.write_value_start(value, &mut stack);
        }

        Ok(())
    }

    /// Writes a scalar or the start of an inline table or array.
    fn write_value_start(&mut self, value: &Value, stack: &mut Vec<InlineFrame>) {
        match *value {
            Value::Str(ref value) => write_string(&mut self.out, value),
            Value::Int(value) => write!(self.out, "{}", value).unwrap(),
            Value::UInt(value) => write!(self.out, "{}", value).unwrap(),
            Value::Float(value) => write_float(&mut self.out, value),
            Value::FloatText(ref value) => self.out.push_str(value),
            Value::Bool(value) => self.out.push_str(if value { "true" } else { "false" }),
            Value::Datetime(ref value) => write!(self.out, "{}", value).unwrap(),
            Value::Table(id) => {
                self.out.push('{');
                stack.push(InlineFrame::Table(id, 0));
            }
            Value::Array(id) => {
                self.out.push('[');
                stack.push(InlineFrame::Array(id, 0));
            }
        }
    }
}

fn write_float(out: &mut String, value: f64) {
    if value.is_nan() {
        out.push_str("nan");
    } else if value.is_infinite() {
        out.push_str(if value > 0.0 { "inf" } else { "-inf" });
    } else {
        let start = out.len();
        write!(out, "{:?}", value).unwrap();
        // floats need a fractional part or an exponent
        if !out[start..].contains(['.', 'e', 'E']) {
            out.push_str(".0");
        }
    }
}

fn is_bare_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn write_key(out: &mut String, key: &str) {
    if is_bare_key(key) {
        out.push_str(key);
    } else {
        write_basic_string(out, key);
    }
}

/// Writes a string as basic, literal or multi-line basic string.
fn write_string(out: &mut String, value: &str) {
    if value.contains('\n') {
        write_multiline_string(out, value);
    } else if value.contains(['"', '\\'])
        && !value
            .chars()
            .any(|c| c == '\'' || (c.is_control() && c != '\t'))
    {
        // literal strings do not need escaping for quotes and backslashes
        out.push('\'');
        out.push_str(value);
        out.push('\'');
    } else {
        write_basic_string(out, value);
    }
}

/// Writes an escape for a control character.
///
/// Only escapes that exist in TOML 1.0 are used.
fn write_control_escape(out: &mut String, c: char) {
    match c {
        '\x08' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\x0c' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        c => write!(out, "\\u{:04X}", c as u32).unwrap(),
    }
}

fn is_escaped_control(c: char) -> bool {
    matches!(c, '\0'..='\x1f' | '\x7f')
}

fn write_basic_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if is_escaped_control(c) => write_control_escape(out, c),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_multiline_string(out: &mut String, value: &str) {
    // the newline after the opening delimiter is trimmed by parsers
    out.push_str("\"\"\"\n");
    let mut quotes = 0;
    for c in value.chars() {
        match c {
            // three quotes in a row would end the string
            '"' if quotes == 2 => {
                out.push_str("\\\"");
                quotes = 0;
                continue;
            }
            '"' => out.push('"'),
            '\\' => out.push_str("\\\\"),
            '\n' | '\t' => out.push(c),
            c if is_escaped_control(c) => write_control_escape(out, c),
            c => out.push(c),
        }
        quotes = if c == '"' { quotes + 1 } else { 0 };
    }
    out.push_str("\"\"\"");
}
