use std::borrow::Cow;
use std::mem::ManuallyDrop;

use deser::adapters::bytes::BytesFormat;
use deser::ext::{BigInt, Decimal, ExtValue, Number};
use deser::ser::{self, SerializeDriver};
use deser::{Atom, Error, ErrorKind, Event, Serialize};

use crate::buf::Buffer;
use crate::de::Trailing;
use crate::pretty::PrettyWriter;
use crate::scan::{find_escape, skip_to_escape};

/// How the output is indented.
///
/// See [`SerializerConfig::indent`] and [`SerializerConfig::pretty`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Indent {
    /// No indentation, the value is written on a single line.
    #[default]
    None,
    /// Every entry on a line of its own, indented by the given number of
    /// spaces per level.
    Spaces(usize),
    /// Every entry on a line of its own, indented by a tab per level.
    Tab,
}

/// When maps and sequences are written on a single line in indented
/// output.
///
/// Maps and sequences with the [`Layout::Compact`](deser::hints::Layout)
/// hint are always written on a single line, the ones with
/// [`Layout::Expanded`](deser::hints::Layout) never (unless they are in a
/// map or sequence on a single line).  See [`SerializerConfig::inline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum InlinePolicy {
    /// Only compact maps and sequences are written on a single line.
    #[default]
    Never,
    /// Maps and sequences which only contain scalars (no maps or
    /// sequences, not even empty ones) are written on a single line if
    /// that line is not longer than the given number of characters
    /// (including the indentation, a tab counts as one character).
    LeafIfFits(usize),
}

/// Configures how values are serialized to JSON.
///
/// By default the output is as short as possible: no line breaks and no
/// spaces.  [`pretty`](Self::pretty) writes every entry on a line of its
/// own:
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_json::{Indent, SerializerConfig};
///
/// let value = BTreeMap::from([("name", vec!["a", "b"])]);
/// assert_eq!(deser_json::to_string(&value).unwrap(), r#"{"name":["a","b"]}"#);
///
/// const PRETTY: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(2));
/// assert_eq!(
///     PRETTY.to_string(&value).unwrap(),
///     "{\n  \"name\": [\n    \"a\",\n    \"b\"\n  ]\n}"
/// );
/// ```
///
/// In indented output maps and sequences with the
/// [`Layout::Compact`](deser::hints::Layout) hint (see
/// [`hints`](deser::hints)) are written on a single line.  The output never
/// ends with a line break.
///
/// [`to_string`](Self::to_string) works like the
/// [`to_string`](crate::to_string) function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    bytes: BytesFormat,
    indent: Indent,
    compact: bool,
    inline: InlinePolicy,
    trailing: Trailing,
}

impl Default for SerializerConfig {
    fn default() -> SerializerConfig {
        SerializerConfig::new()
    }
}

impl SerializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> SerializerConfig {
        SerializerConfig {
            bytes: BytesFormat::BASE64,
            indent: Indent::None,
            compact: true,
            inline: InlinePolicy::Never,
            trailing: Trailing::Strict,
        }
    }

    /// Sets what follows the values of a stream.
    ///
    /// This is the counterpart of
    /// [`DeserializerConfig::trailing`](crate::DeserializerConfig::trailing)
    /// for writing more than one value (with a [`Serializer`] or a stream
    /// writer), it does not affect [`to_string`](Self::to_string):
    ///
    /// * [`Trailing::Strict`]: the stream holds a single value, writing a
    ///   second one fails.  This is the default.
    /// * [`Trailing::Newline`]: every value is followed by a line break
    ///   ([JSON Lines](https://jsonlines.org/)).  The values must not be
    ///   indented.
    /// * [`Trailing::Stop`]: values are separated by line breaks.
    ///
    /// ```
    /// use deser_json::{Serializer, SerializerConfig, Trailing};
    ///
    /// const LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);
    /// let mut serializer = Serializer::with_config(&LINES);
    /// serializer.serialize(&vec![1, 2]).unwrap();
    /// serializer.serialize(&vec![3]).unwrap();
    /// assert_eq!(serializer.finish(), "[1,2]\n[3]\n");
    /// ```
    pub const fn trailing(mut self, trailing: Trailing) -> SerializerConfig {
        self.trailing = trailing;
        self
    }

    /// Returns what follows the values of a stream.
    pub(crate) fn trailing_mode(&self) -> Trailing {
        self.trailing
    }

    /// Sets how the output is indented.
    ///
    /// By default ([`Indent::None`]) the value is written on a single line.
    /// Otherwise every entry of a map or sequence is written on a line of
    /// its own, indented by its depth.  Empty maps and sequences are always
    /// written as `{}` and `[]`.  This does not change the spaces after
    /// separators, see [`compact`](Self::compact).  To indent with spaces
    /// after separators use [`pretty`](Self::pretty).
    ///
    /// ```
    /// use deser_json::{Indent, SerializerConfig};
    ///
    /// const TAB: SerializerConfig = SerializerConfig::new().indent(Indent::Tab);
    /// assert_eq!(TAB.to_string(&vec![1, 2]).unwrap(), "[\n\t1,\n\t2\n]");
    /// ```
    pub const fn indent(mut self, indent: Indent) -> SerializerConfig {
        self.indent = indent;
        self
    }

    /// Controls the spaces after separators.
    ///
    /// When enabled (which is the default) there are no spaces after `:`
    /// and `,`.  When disabled a space follows every `:` and every `,`
    /// that is not followed by a line break:
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_json::SerializerConfig;
    ///
    /// let value = BTreeMap::from([("a", vec![1, 2])]);
    /// const SPACED: SerializerConfig = SerializerConfig::new().compact(false);
    /// assert_eq!(SPACED.to_string(&value).unwrap(), r#"{"a": [1, 2]}"#);
    /// ```
    pub const fn compact(mut self, yes: bool) -> SerializerConfig {
        self.compact = yes;
        self
    }

    /// Sets when maps and sequences are written on a single line in
    /// indented output.
    ///
    /// ```
    /// use deser::Serialize;
    /// use deser_json::{Indent, InlinePolicy, SerializerConfig};
    ///
    /// #[derive(Serialize)]
    /// struct Shape {
    ///     name: &'static str,
    ///     points: Vec<Vec<i32>>,
    /// }
    ///
    /// let shape = Shape {
    ///     name: "line",
    ///     points: vec![vec![0, 0], vec![3, 4]],
    /// };
    /// const CONFIG: SerializerConfig = SerializerConfig::new()
    ///     .pretty(Indent::Spaces(2))
    ///     .inline(InlinePolicy::LeafIfFits(80));
    /// assert_eq!(CONFIG.to_string(&shape).unwrap(), r#"{
    ///   "name": "line",
    ///   "points": [
    ///     [0, 0],
    ///     [3, 4]
    ///   ]
    /// }"#);
    /// ```
    ///
    /// This has no effect without [indentation](Self::indent).
    pub const fn inline(mut self, policy: InlinePolicy) -> SerializerConfig {
        self.inline = policy;
        self
    }

    /// Enables or disables pretty printing.
    ///
    /// This sets the [indentation](Self::indent) and writes spaces after
    /// separators (see [`compact`](Self::compact)) unless the indentation
    /// is [`Indent::None`], in which case the output is compact again.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_json::{Indent, SerializerConfig};
    ///
    /// let value = BTreeMap::from([("a", 1)]);
    /// const PRETTY: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(4));
    /// assert_eq!(PRETTY.to_string(&value).unwrap(), "{\n    \"a\": 1\n}");
    /// const NOT_PRETTY: SerializerConfig = PRETTY.pretty(Indent::None);
    /// assert_eq!(NOT_PRETTY.to_string(&value).unwrap(), r#"{"a":1}"#);
    /// ```
    pub const fn pretty(mut self, indent: Indent) -> SerializerConfig {
        self.indent = indent;
        self.compact = matches!(indent, Indent::None);
        self
    }

    /// Sets how bytes are represented.
    ///
    /// JSON has no bytes, by default they are written as base64 strings
    /// ([`BytesFormat::BASE64`]).  Values can request a different format
    /// (see [`deser::adapters::bytes`]) which takes precedence.  Map keys cannot be
    /// sequences, bytes in keys are always strings.
    ///
    /// ```
    /// use deser::adapters::bytes::{BytesFormat, Hex};
    /// use deser_json::SerializerConfig;
    ///
    /// assert_eq!(deser_json::to_string(&b"\x01\xff").unwrap(), r#""Af8=""#);
    /// const HEX: SerializerConfig = SerializerConfig::new().bytes(BytesFormat::encoded::<Hex>());
    /// assert_eq!(HEX.to_string(&b"\x01\xff").unwrap(), r#""01ff""#);
    /// ```
    ///
    /// Bytes in other formats than base64 (or sequences) need to be
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
    ///
    /// ```
    /// use deser::ser::{Layer, Next};
    /// use deser::{Atom, Error, Event};
    /// use deser_json::SerializerConfig;
    ///
    /// /// Writes all numbers as strings.
    /// struct NumbersAsStrings;
    ///
    /// impl Layer for NumbersAsStrings {
    ///     fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
    ///         match event {
    ///             Event::Atom(Atom::U64(value)) => next.emit(value.to_string().into()),
    ///             event => next.emit(event),
    ///         }
    ///     }
    /// }
    ///
    /// let json = SerializerConfig::new()
    ///     .to_string_with(&vec![1u64, 2], |driver| driver.push_layer(NumbersAsStrings))
    ///     .unwrap();
    /// assert_eq!(json, r#"["1","2"]"#);
    /// ```
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
        let ser = Output {
            out: Buffer::with_capacity(128),
            bytes: self.bytes,
        };
        if self.indent == Indent::None && self.compact {
            let mut writer = Writer {
                ser,
                stack: Vec::new(),
                container: Container::Top,
                first: true,
                is_key: false,
            };
            driver.drive(|event, _| writer.event(event))?;
            Ok(writer.ser.out.into_string())
        } else {
            let inline_width = match self.inline {
                InlinePolicy::Never => None,
                InlinePolicy::LeafIfFits(width) => Some(width),
            };
            let mut writer = PrettyWriter::new(ser, self.indent, self.compact, inline_width);
            driver.drive(|event, state| writer.event(event, state))?;
            Ok(writer.finish())
        }
    }
}

/// Serializes values into JSON.
///
/// Every call to [`serialize`](Self::serialize) writes a value.  What
/// follows the values depends on [`SerializerConfig::trailing`]: by default
/// only a single value can be written, with [`Trailing::Newline`] every
/// value is followed by a line break ([JSON Lines](https://jsonlines.org/)).
///
/// ```
/// use deser_json::{Serializer, SerializerConfig, Trailing};
///
/// const LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);
/// let mut serializer = Serializer::with_config(&LINES);
/// serializer.serialize(&vec![1, 2]).unwrap();
/// serializer.serialize(&"x").unwrap();
/// assert_eq!(serializer.finish(), "[1,2]\n\"x\"\n");
/// ```
///
/// To write to a [`Write`](std::io::Write) use a
/// [`deser::io::Writer`] with the configuration.
#[derive(Debug, Clone)]
pub struct Serializer {
    config: SerializerConfig,
    // only holds the output of `encode_value` which is valid UTF-8
    out: Vec<u8>,
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
            out: Vec::new(),
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
        // SAFETY: the output is valid UTF-8, see `SerializerConfig::encode_value`
        unsafe { std::str::from_utf8_unchecked(&self.out) }
    }

    /// Returns the output.
    pub fn finish(self) -> String {
        // SAFETY: the output is valid UTF-8, see `SerializerConfig::encode_value`
        unsafe { String::from_utf8_unchecked(self.out) }
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        let len = self.out.len();
        match self
            .config
            .encode_value(driver, self.written, &mut self.out)
        {
            Ok(()) => {
                self.written += 1;
                Ok(())
            }
            Err(err) => {
                self.out.truncate(len);
                Err(err)
            }
        }
    }
}

impl SerializerConfig {
    /// Serializes a value as the value with the given index of a stream.
    ///
    /// This writes what separates the values of a stream.  Only valid UTF-8
    /// is appended to the output.
    pub(crate) fn encode_value(
        &self,
        driver: &mut SerializeDriver<'_>,
        index: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let trailing = self.trailing_mode();
        match trailing {
            Trailing::Strict if index > 0 => {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "with Trailing::Strict only a single value can be written",
                ));
            }
            Trailing::Stop if index > 0 => out.push(b'\n'),
            _ => {}
        }
        let json = self.serialize_driver(driver)?;
        out.extend_from_slice(json.as_bytes());
        if trailing == Trailing::Newline {
            out.push(b'\n');
        }
        Ok(())
    }
}

/// The output of the serializer.
pub(crate) struct Output {
    pub(crate) out: Buffer,
    bytes: BytesFormat,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Top,
    Seq,
    Map,
}

/// Holds the state of the serializer while writing.
struct Writer {
    ser: Output,
    // the state of the current container is held here, the state of the
    // outer containers is saved on the stack.
    stack: Vec<Container>,
    container: Container,
    first: bool,
    is_key: bool,
}

impl Writer {
    #[inline(always)]
    fn event(&mut self, event: Event) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => {
                if self.is_key {
                    self.ser.write_key_atom(atom, self.first)?;
                    self.is_key = false;
                } else {
                    match self.container {
                        Container::Seq => {
                            if !self.first {
                                self.ser.write_char(',');
                            }
                        }
                        Container::Map => self.is_key = true,
                        Container::Top => {}
                    }
                    self.ser.write_atom(atom)?;
                }
                self.first = false;
                Ok(())
            }
            Event::MapStart(_) => self.start(true),
            Event::SeqStart(_) => self.start(false),
            Event::MapEnd => self.end(true),
            Event::SeqEnd => self.end(false),
        }
    }

    #[inline(never)]
    fn start(&mut self, is_map: bool) -> Result<(), Error> {
        if self.is_key {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "JSON does not support this value for map keys",
            ));
        }
        if self.container == Container::Seq && !self.first {
            self.ser.write_char(',');
        }
        self.stack.push(self.container);
        self.first = true;
        if is_map {
            self.container = Container::Map;
            self.is_key = true;
            self.ser.write_char('{');
        } else {
            self.container = Container::Seq;
            self.ser.write_char('[');
        }
        Ok(())
    }

    #[inline(never)]
    fn end(&mut self, is_map: bool) -> Result<(), Error> {
        if is_map {
            if self.container != Container::Map || !self.is_key {
                return Err(Error::new(ErrorKind::Unexpected, "unexpected map end"));
            }
            self.ser.write_char('}');
        } else {
            if self.container != Container::Seq {
                return Err(Error::new(ErrorKind::Unexpected, "unexpected array end"));
            }
            self.ser.write_char(']');
        }
        self.container = self.stack.pop().unwrap_or(Container::Top);
        // a container is never a key, so after it the next item in a map is
        // a key again.
        self.first = false;
        self.is_key = self.container == Container::Map;
        Ok(())
    }
}

impl Output {
    /// Writes an atom in key position including separator and colon.
    #[inline(always)]
    fn write_key_atom(&mut self, atom: Atom, first: bool) -> Result<(), Error> {
        // borrowed strings do not need to be dropped, the atom is only
        // dropped for the other values.
        let atom = ManuallyDrop::new(atom);
        match *atom {
            // fast path for the common case of string keys
            Atom::Str(Cow::Borrowed(val)) => {
                self.write_key(val, first);
                Ok(())
            }
            _ => self.write_other_key_atom(ManuallyDrop::into_inner(atom), first),
        }
    }

    #[inline(never)]
    fn write_other_key_atom(&mut self, atom: Atom, first: bool) -> Result<(), Error> {
        if !first {
            self.write_char(',');
        }
        self.write_key_text(atom)?;
        self.write_char(':');
        Ok(())
    }

    /// Writes an atom as map key without separator and colon.
    pub(crate) fn write_key_text(&mut self, atom: Atom) -> Result<(), Error> {
        match atom {
            Atom::Str(ref val) => self.write_escaped_str(val),
            Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => {
                self.write_char('"');
                self.write_u64(val);
                self.write_char('"');
            }
            Atom::I64(val) => {
                self.write_char('"');
                self.write_i64(val);
                self.write_char('"');
            }
            Atom::Ext(ref ext) => self.write_ext_key(ext)?,
            Atom::Bytes(ref val) => self.write_bytes_str(val, val.fallback),
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "JSON does not support this value for map keys",
                ));
            }
        }
        Ok(())
    }

    /// Writes an atom in value position.
    #[inline(always)]
    pub(crate) fn write_atom(&mut self, atom: Atom) -> Result<(), Error> {
        // borrowed strings and scalars do not need to be dropped, the atom
        // is only dropped for the other values.
        let atom = ManuallyDrop::new(atom);
        match *atom {
            Atom::Null => self.write_str("null"),
            Atom::Bool(true) => self.write_str("true"),
            Atom::Bool(false) => self.write_str("false"),
            Atom::Str(Cow::Borrowed(val)) => self.write_escaped_str(val),
            Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => self.write_u64(val),
            Atom::I64(val) => self.write_i64(val),
            Atom::F64(val) => self.write_float(val),
            Atom::F32(val) => self.write_float(val),
            _ => return self.write_other_atom(ManuallyDrop::into_inner(atom)),
        }
        Ok(())
    }

    #[inline(never)]
    fn write_other_atom(&mut self, atom: Atom) -> Result<(), Error> {
        match atom {
            Atom::Str(ref val) => self.write_escaped_str(val),
            Atom::Ext(ref ext) => self.write_ext_value(ext)?,
            Atom::Bytes(ref val) => {
                self.write_bytes(val, val.fallback.copied().unwrap_or(self.bytes))
            }
            _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        }
        Ok(())
    }

    /// Writes bytes in the given format.
    fn write_bytes(&mut self, bytes: &[u8], format: BytesFormat) {
        match format.encode(bytes) {
            Some(encoded) => self.write_escaped_str(&encoded),
            None => {
                self.write_char('[');
                for (idx, &byte) in bytes.iter().enumerate() {
                    if idx > 0 {
                        self.write_char(',');
                    }
                    self.write_u64(byte.into());
                }
                self.write_char(']');
            }
        }
    }

    /// Writes bytes as string, for instance as map key.
    ///
    /// Bytes that would be sequences are base64.
    fn write_bytes_str(&mut self, bytes: &[u8], fallback: Option<&BytesFormat>) {
        let format = fallback.copied().unwrap_or(self.bytes);
        let encoded = format
            .encode(bytes)
            .or_else(|| BytesFormat::BASE64.encode(bytes))
            .unwrap_or_default();
        self.write_escaped_str(&encoded);
    }

    #[inline(always)]
    pub(crate) fn write_str(&mut self, s: &str) {
        self.out.push_str(s);
    }

    #[inline(always)]
    pub(crate) fn write_char(&mut self, c: char) {
        debug_assert!(c.is_ascii());
        self.out.push(c as u8);
    }

    /// Writes a map key including the separator and the colon.
    #[inline]
    fn write_key(&mut self, key: &str, first: bool) {
        if find_escape(key.as_bytes()) != key.len() {
            if !first {
                self.write_char(',');
            }
            self.write_escaped_str_slow(key);
            self.write_char(':');
            return;
        }
        self.out.reserve(key.len() + 4);
        // SAFETY: the capacity was reserved above
        unsafe {
            if !first {
                self.out.push_unchecked(b',');
            }
            self.out.push_unchecked(b'"');
            self.out.push_str_unchecked(key);
            self.out.push_unchecked(b'"');
            self.out.push_unchecked(b':');
        }
    }

    /// Writes a float with the shortest text that reads back as the same
    /// value of its type (`f32` or `f64`).
    #[inline]
    fn write_float<F: Float>(&mut self, val: F) {
        if val.is_finite() {
            #[cfg(feature = "speedups")]
            {
                self.write_str(zmij::Buffer::new().format_finite(val))
            }
            #[cfg(not(feature = "speedups"))]
            {
                self.write_str(&deser::__float::format_finite(val))
            }
        } else {
            self.write_str("null")
        }
    }

    /// Writes an extension value as map key.
    ///
    /// Extension values that JSON does not natively support are written in
    /// their fallback representation.
    #[cold]
    fn write_ext_key(&mut self, ext: &ExtValue) -> Result<(), Error> {
        if ext.is::<u128>() || ext.is::<i128>() {
            self.write_char('"');
            self.write_ext_value(ext)?;
            self.write_char('"');
            return Ok(());
        }
        match ext.fallback() {
            Atom::Str(val) => self.write_escaped_str(&val),
            Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => {
                self.write_char('"');
                self.write_u64(val);
                self.write_char('"');
            }
            Atom::I64(val) => {
                self.write_char('"');
                self.write_i64(val);
                self.write_char('"');
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "JSON does not support this value for map keys",
                ));
            }
        }
        Ok(())
    }

    /// Writes an extension value.
    ///
    /// Extension values that JSON does not natively support are written in
    /// their fallback representation.
    #[cold]
    fn write_ext_value(&mut self, ext: &ExtValue) -> Result<(), Error> {
        // JSON numbers have arbitrary precision, so wide integers and
        // decimals can be written natively.
        if let Some(&val) = ext.downcast_ref::<u128>() {
            self.write_int(val);
            return Ok(());
        } else if let Some(&val) = ext.downcast_ref::<i128>() {
            self.write_int(val);
            return Ok(());
        } else if let Some(val) = ext.downcast_ref::<BigInt>() {
            self.write_str(&val.to_string());
            return Ok(());
        } else if let Some(val) = ext.downcast_ref::<Decimal>() {
            // decimals use the syntax of JSON numbers
            self.write_str(val.as_str());
            return Ok(());
        } else if let Some(val) = ext.downcast_value_ref::<Number>() {
            // numbers keep their text, so they roundtrip exactly
            self.write_str(val.as_str());
            return Ok(());
        }
        match ext.fallback() {
            Atom::Null => self.write_str("null"),
            Atom::Bool(val) => self.write_str(if val { "true" } else { "false" }),
            Atom::Str(val) => self.write_escaped_str(&val),
            Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => self.write_u64(val),
            Atom::I64(val) => self.write_i64(val),
            Atom::F64(val) => self.write_float(val),
            Atom::F32(val) => self.write_float(val),
            // like in TOML the fallbacks of extension values are never
            // sequences
            Atom::Bytes(val) => self.write_bytes_str(&val, val.fallback),
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "JSON does not support this value",
                ));
            }
        }
        Ok(())
    }

    #[cfg(feature = "speedups")]
    fn write_int<I: itoa::Integer>(&mut self, val: I) {
        self.write_str(itoa::Buffer::new().format(val))
    }

    #[cfg(not(feature = "speedups"))]
    fn write_int<I: std::fmt::Display>(&mut self, val: I) {
        self.write_str(&val.to_string())
    }

    fn write_u64(&mut self, val: u64) {
        #[cfg(feature = "speedups")]
        {
            self.write_str(itoa::Buffer::new().format(val))
        }
        #[cfg(not(feature = "speedups"))]
        {
            self.write_str(&val.to_string())
        }
    }

    fn write_i64(&mut self, val: i64) {
        #[cfg(feature = "speedups")]
        {
            self.write_str(itoa::Buffer::new().format(val))
        }
        #[cfg(not(feature = "speedups"))]
        {
            self.write_str(&val.to_string())
        }
    }

    #[inline]
    fn write_escaped_str(&mut self, value: &str) {
        if find_escape(value.as_bytes()) != value.len() {
            return self.write_escaped_str_slow(value);
        }
        self.out.reserve(value.len() + 2);
        // SAFETY: the capacity was reserved above
        unsafe {
            self.out.push_unchecked(b'"');
            self.out.push_str_unchecked(value);
            self.out.push_unchecked(b'"');
        }
    }

    #[inline(never)]
    fn write_escaped_str_slow(&mut self, value: &str) {
        self.write_char('"');

        let bytes = value.as_bytes();
        let mut start = 0;

        loop {
            let next = skip_to_escape(bytes, start);
            if start < next {
                self.write_str(&value[start..next]);
            }
            if next == bytes.len() {
                break;
            }

            let byte = bytes[next];
            match ESCAPE[byte as usize] {
                self::BB => self.write_str("\\b"),
                self::TT => self.write_str("\\t"),
                self::NN => self.write_str("\\n"),
                self::FF => self.write_str("\\f"),
                self::RR => self.write_str("\\r"),
                self::QU => self.write_str("\\\""),
                self::BS => self.write_str("\\\\"),
                self::U => {
                    static HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";
                    self.write_str("\\u00");
                    self.write_char(HEX_DIGITS[(byte >> 4) as usize] as char);
                    self.write_char(HEX_DIGITS[(byte & 0xF) as usize] as char);
                }
                _ => unreachable!(),
            }

            start = next + 1;
        }

        self.write_char('"');
    }
}

const BB: u8 = b'b'; // \x08
const TT: u8 = b't'; // \x09
const NN: u8 = b'n'; // \x0A
const FF: u8 = b'f'; // \x0C
const RR: u8 = b'r'; // \x0D
const QU: u8 = b'"'; // \x22
const BS: u8 = b'\\'; // \x5C
const U: u8 = b'u'; // \x00...\x1F except the ones above

// Lookup table of escape sequences. A value of b'x' at index i means that byte
// i is escaped as "\x" in JSON. A value of 0 means that byte i is not escaped.
#[rustfmt::skip]
static ESCAPE: [u8; 256] = [
    //  1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    U,  U,  U,  U,  U,  U,  U,  U, BB, TT, NN,  U, FF, RR,  U,  U, // 0
    U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U, // 1
    0,  0, QU,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 2
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 3
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 4
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, BS,  0,  0,  0, // 5
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 6
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 7
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 8
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 9
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // A
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // B
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // C
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // D
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // E
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // F
];

/// The floats that are written (`f32` and `f64`).
#[cfg(feature = "speedups")]
trait Float: zmij::Float + deser::__float::Float {}

#[cfg(feature = "speedups")]
impl<F: zmij::Float + deser::__float::Float> Float for F {}

/// The floats that are written (`f32` and `f64`).
#[cfg(not(feature = "speedups"))]
trait Float: deser::__float::Float {}

#[cfg(not(feature = "speedups"))]
impl<F: deser::__float::Float> Float for F {}

/// Serializes a value to JSON.
///
/// This uses the default [`SerializerConfig`].
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}
