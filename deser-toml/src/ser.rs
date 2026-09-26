use std::borrow::Cow;
use std::fmt::{self, Write};

use deser::adapters::bytes::BytesFormat;
use deser::ext::ExtValue;
use deser::hints::Layout;
use deser::ser::{self, SerializeDriver};
use deser::{Atom, Error, ErrorKind, Event, Serialize, State};

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
/// maps as `[[array]]` sections unless they are nested in other sequences or
/// have the [`Layout::Compact`](deser::hints::Layout) hint (see
/// [`hints`](deser::hints)) which makes them inline.  Inline tables and
/// inline arrays of tables have that hint when deserialized, so they stay
/// inline when a value is deserialized and serialized again.
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
    /// (see [`deser::adapters::bytes`]) which takes precedence.  Keys cannot be
    /// arrays, bytes in keys are always strings.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser::adapters::bytes::{BytesFormat, Hex};
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
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        self.serialize_driver(&mut driver)
    }

    /// Serializes the value of a driver.
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
    ) -> Result<String, Error> {
        let mut builder = Builder {
            doc: Document::default(),
            stack: Vec::new(),
            done: false,
            bytes: self.bytes,
        };
        driver.drive(|event, state| builder.event(event, state))?;
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

/// Serializes values into TOML.
///
/// A TOML document holds a single value, writing a second one fails.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_toml::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&BTreeMap::from([("a", 1)])).unwrap();
/// assert!(serializer.serialize(&BTreeMap::from([("b", 2)])).is_err());
/// assert_eq!(serializer.finish(), "a = 1\n");
/// ```
///
/// To write to a [`Write`](std::io::Write) use a
/// [`deser::io::Writer`] with the configuration.
#[derive(Debug, Clone)]
pub struct Serializer {
    config: SerializerConfig,
    out: String,
    written: usize,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(&SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: &SerializerConfig) -> Serializer {
        Serializer {
            config: config.clone(),
            out: String::new(),
            written: 0,
        }
    }

    /// Serializes a value.
    ///
    /// If the value fails to serialize, nothing is written.
    pub fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser::ser::Layer)s.
    pub fn serialize_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the output written so far.
    pub fn output(&self) -> &str {
        &self.out
    }

    /// Returns the output.
    pub fn finish(self) -> String {
        self.out
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        if self.written > 0 {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "a TOML document holds a single value",
            ));
        }
        let toml = self.config.serialize_driver(driver)?;
        self.out.push_str(&toml);
        self.written += 1;
        Ok(())
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
    fn event(&mut self, event: Event, state: &State) -> Result<(), Error> {
        let Some(frame) = self.stack.last_mut() else {
            if self.done {
                return Err(Error::new(ErrorKind::Unexpected, "unexpected event"));
            }
            return match event {
                Event::MapStart(_) => {
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
                    *key = Some(key_to_string(atom, self.bytes)?);
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
                let value = match self.value(event, state)? {
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
                match self.value(event, state)? {
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
    fn value(&mut self, event: Event, state: &State) -> Result<Converted, Error> {
        match event {
            Event::Atom(Atom::Bytes(ref bytes)) => Ok(Converted::Value(
                self.bytes_value(bytes, bytes.fallback.copied().unwrap_or(self.bytes)),
            )),
            Event::Atom(atom) => convert_atom(atom, self.bytes),
            Event::MapStart(_) => {
                // compact tables are inline tables, others sections
                let kind = match Layout::of(state) {
                    Layout::Compact => TableKind::Inline,
                    _ => TableKind::Header,
                };
                let id = self.doc.new_table(kind, Span::default());
                self.stack.push(Frame::Table(id, None));
                Ok(Converted::Value(Value::Table(id)))
            }
            Event::SeqStart(_) => {
                // arrays of tables are `[[array]]` sections unless compact
                let of_tables = Layout::of(state) != Layout::Compact;
                let id = self.doc.new_array(of_tables, Span::default());
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

fn convert_atom(atom: Atom, bytes: BytesFormat) -> Result<Converted, Error> {
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
        Atom::F64(value) => Value::Float(value),
        Atom::F32(value) => Value::Float32(value),
        // bytes are converted by the builder, this is reached for the
        // fallbacks of extension values which cannot be arrays.
        Atom::Bytes(value) => Value::Str(Cow::Owned(encode_str(&value, value.fallback, bytes))),
        Atom::Ext(ref ext) => return convert_ext(ext, bytes),
        _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
    }))
}

#[cold]
fn convert_ext(ext: &ExtValue, bytes: BytesFormat) -> Result<Converted, Error> {
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
        return convert_atom(Atom::U64(value), bytes);
    }
    if let Some(&value) = ext.downcast_ref::<i128>() {
        return if let Ok(value) = i64::try_from(value) {
            convert_atom(Atom::I64(value), bytes)
        } else {
            let value = u64::try_from(value).map_err(|_| out_of_range())?;
            convert_atom(Atom::U64(value), bytes)
        };
    }
    match ext.fallback() {
        Atom::Ext(_) => Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("TOML does not support {}", ext.name()),
        )),
        fallback => convert_atom(fallback, bytes),
    }
}

/// Encodes bytes as string.
///
/// Strings are required (for keys), so bytes that would be arrays are base64.
fn encode_str(value: &[u8], fallback: Option<&BytesFormat>, bytes: BytesFormat) -> String {
    fallback
        .copied()
        .unwrap_or(bytes)
        .encode(value)
        .or_else(|| BytesFormat::BASE64.encode(value))
        .unwrap_or_default()
}

fn key_to_string(atom: Atom, bytes: BytesFormat) -> Result<String, Error> {
    Ok(match atom {
        Atom::Str(value) => value.into_owned(),
        Atom::Char(value) => value.to_string(),
        Atom::U64(value) => value.to_string(),
        Atom::I64(value) => value.to_string(),
        Atom::Bytes(value) => encode_str(&value, value.fallback, bytes),
        Atom::Ext(ref ext) => {
            if let Some(value) = ext.downcast_ref::<u128>() {
                value.to_string()
            } else if let Some(value) = ext.downcast_ref::<i128>() {
                value.to_string()
            } else {
                match ext.fallback() {
                    Atom::Ext(_) => return Err(unsupported_key()),
                    fallback => return key_to_string(fallback, bytes),
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
            Value::Table(id) => self.doc.tables[id].kind != TableKind::Inline,
            Value::Array(id) => self.is_array_of_tables(id),
            _ => false,
        }
    }

    fn is_array_of_tables(&self, id: usize) -> bool {
        let array = &self.doc.arrays[id];
        array.of_tables
            && !array.items.is_empty()
            && array
                .items
                .iter()
                .all(|x| matches!(x.value, Value::Table(_)))
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
                    Value::Table(id) if self.is_section(&entry.item.value) => {
                        sections.push(Section {
                            id,
                            path: child_path(),
                            header: Header::Table,
                        })
                    }
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
            Value::Float32(value) => write_float(&mut self.out, value),
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

/// Writes a float with the shortest text that reads back as the same value
/// of its type (`f32` or `f64`).
fn write_float<F: Into<f64> + fmt::Debug + Copy>(out: &mut String, value: F) {
    let wide: f64 = value.into();
    if wide.is_nan() {
        out.push_str("nan");
    } else if wide.is_infinite() {
        out.push_str(if wide > 0.0 { "inf" } else { "-inf" });
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
