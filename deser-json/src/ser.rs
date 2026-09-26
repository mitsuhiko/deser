use std::borrow::Cow;
use std::mem::ManuallyDrop;

use deser::adapters::bytes::BytesFormat;
use deser::ext::{BigInt, Decimal, ExtValue, Number};
use deser::ser::SerializeDriver;
use deser::{Atom, Descriptor, Error, ErrorKind, Event, Serialize};

use crate::buf::Buffer;
use crate::scan::{find_escape, skip_to_escape};

/// Configures how values are serialized to JSON.
///
/// [`to_string`](Self::to_string) works like the
/// [`to_string`](crate::to_string) function.
///
/// ```
/// use deser::adapters::bytes::BytesFormat;
/// use deser_json::SerializerConfig;
///
/// const CONFIG: SerializerConfig = SerializerConfig::new().bytes(BytesFormat::SEQ);
/// assert_eq!(CONFIG.to_string(&vec![1u8, 2]).unwrap(), "[1,2]");
/// ```
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
    /// use deser::{Atom, Descriptor, Error, Event};
    /// use deser_json::SerializerConfig;
    ///
    /// /// Writes all numbers as strings.
    /// struct NumbersAsStrings;
    ///
    /// impl Layer for NumbersAsStrings {
    ///     fn event(
    ///         &mut self,
    ///         event: Event<'_>,
    ///         descriptor: &'static dyn Descriptor,
    ///         next: &mut Next<'_>,
    ///     ) -> Result<(), Error> {
    ///         match event {
    ///             Event::Atom(Atom::U64(value)) => next.emit(value.to_string().into(), descriptor),
    ///             event => next.emit(event, descriptor),
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
        let mut writer = Writer {
            ser: Output {
                out: Buffer::with_capacity(128),
                bytes: self.bytes,
            },
            stack: Vec::new(),
            container: Container::Top,
            first: true,
            is_key: false,
        };
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        driver.drive(|event, descriptor, _| writer.event(event, descriptor))?;
        Ok(writer.ser.out.into_string())
    }
}

/// A descriptor which does not request a bytes format.
struct NoFormat;

impl Descriptor for NoFormat {}

/// The output of the serializer.
struct Output {
    out: Buffer,
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
    fn event(&mut self, event: Event, descriptor: &dyn Descriptor) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => {
                if self.is_key {
                    self.ser.write_key_atom(atom, descriptor, self.first)?;
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
                    self.ser.write_atom(atom, descriptor)?;
                }
                self.first = false;
                Ok(())
            }
            Event::MapStart => self.start(true),
            Event::SeqStart => self.start(false),
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
    fn write_key_atom(
        &mut self,
        atom: Atom,
        descriptor: &dyn Descriptor,
        first: bool,
    ) -> Result<(), Error> {
        // borrowed strings do not need to be dropped, the atom is only
        // dropped for the other values.
        let atom = ManuallyDrop::new(atom);
        match *atom {
            // fast path for the common case of string keys
            Atom::Str(Cow::Borrowed(val)) => {
                self.write_key(val, first);
                Ok(())
            }
            _ => self.write_other_key_atom(ManuallyDrop::into_inner(atom), descriptor, first),
        }
    }

    #[inline(never)]
    fn write_other_key_atom(
        &mut self,
        atom: Atom,
        descriptor: &dyn Descriptor,
        first: bool,
    ) -> Result<(), Error> {
        if !first {
            self.write_char(',');
        }
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
            Atom::Bytes(ref val) => self.write_bytes_str(val, descriptor),
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "JSON does not support this value for map keys",
                ));
            }
        }
        self.write_char(':');
        Ok(())
    }

    /// Writes an atom in value position.
    #[inline(always)]
    fn write_atom(&mut self, atom: Atom, descriptor: &dyn Descriptor) -> Result<(), Error> {
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
            Atom::F64(val) => self.write_float(val, descriptor),
            _ => return self.write_other_atom(ManuallyDrop::into_inner(atom), descriptor),
        }
        Ok(())
    }

    #[inline(never)]
    fn write_other_atom(&mut self, atom: Atom, descriptor: &dyn Descriptor) -> Result<(), Error> {
        match atom {
            Atom::Str(ref val) => self.write_escaped_str(val),
            Atom::Ext(ref ext) => self.write_ext_value(ext)?,
            Atom::Bytes(ref val) => {
                self.write_bytes(val, descriptor.bytes_format().unwrap_or(self.bytes))
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
    fn write_bytes_str(&mut self, bytes: &[u8], descriptor: &dyn Descriptor) {
        let format = descriptor.bytes_format().unwrap_or(self.bytes);
        let encoded = format
            .encode(bytes)
            .or_else(|| BytesFormat::BASE64.encode(bytes))
            .unwrap_or_default();
        self.write_escaped_str(&encoded);
    }

    #[inline(always)]
    fn write_str(&mut self, s: &str) {
        self.out.push_str(s);
    }

    #[inline(always)]
    fn write_char(&mut self, c: char) {
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

    /// Writes a float atom.
    ///
    /// Floats are widened to f64 in the data model, the descriptor tells us
    /// the original precision.
    #[inline(never)]
    fn write_float(&mut self, val: f64, descriptor: &dyn Descriptor) {
        if descriptor.precision() == Some(32) {
            self.write_f32(val as f32);
        } else {
            self.write_f64(val);
        }
    }

    fn write_f32(&mut self, val: f32) {
        if val.is_finite() {
            #[cfg(feature = "speedups")]
            {
                self.write_str(ryu::Buffer::new().format_finite(val))
            }
            #[cfg(not(feature = "speedups"))]
            {
                self.write_str(val.to_string().as_str())
            }
        } else {
            self.write_str("null")
        }
    }

    fn write_f64(&mut self, val: f64) {
        if val.is_finite() {
            #[cfg(feature = "speedups")]
            {
                self.write_str(ryu::Buffer::new().format_finite(val))
            }
            #[cfg(not(feature = "speedups"))]
            {
                self.write_str(val.to_string().as_str())
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
            Atom::F64(val) => self.write_f64(val),
            // like in TOML the fallbacks of extension values are never
            // sequences
            Atom::Bytes(val) => self.write_bytes_str(&val, &NoFormat),
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

/// Serializes a value to JSON.
///
/// This uses the default [`SerializerConfig`].
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}
