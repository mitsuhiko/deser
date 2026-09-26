use std::borrow::Cow;

use deser::adapters::bytes::BytesFormat;
use deser::de::{Deserialize, DeserializeDriver, Format};
use deser::ext::ExtValue;
use deser::{Atom, Error, ErrorKind, Event};

use crate::document::{Document, Item, Span, Value};
use crate::parser::{ROOT, parse};

/// Configures how TOML is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.  To create a [`Deserializer`] with
/// the configuration use [`Deserializer::from_str_with_config`] or
/// [`Deserializer::from_slice_with_config`].
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_toml::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new().track_locations(true);
/// let value: BTreeMap<String, u32> = CONFIG.from_str("a = 1").unwrap();
/// assert_eq!(value["a"], 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    track_locations: bool,
    bytes: BytesFormat,
}

impl Default for DeserializerConfig {
    fn default() -> DeserializerConfig {
        DeserializerConfig::new()
    }
}

impl DeserializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig {
            track_locations: false,
            bytes: BytesFormat::BASE64,
        }
    }

    /// Sets how strings are decoded into bytes.
    ///
    /// TOML has no bytes, types that expect bytes (like `Vec<u8>`) accept
    /// strings and arrays of integers instead.  By default strings are
    /// decoded as base64, both with the standard and the URL-safe alphabet
    /// and with or without padding.  This changes how strings are decoded,
    /// for [`BytesFormat::SEQ`] they are still decoded as base64.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser::adapters::bytes::{BytesFormat, Hex};
    /// use deser_toml::DeserializerConfig;
    ///
    /// let value: BTreeMap<String, Vec<u8>> = deser_toml::from_str("a = \"Af8=\"").unwrap();
    /// assert_eq!(value["a"], [1, 255]);
    ///
    /// const HEX: DeserializerConfig = DeserializerConfig::new().bytes(BytesFormat::encoded::<Hex>());
    /// let value: BTreeMap<String, Vec<u8>> = HEX.from_str("a = \"01ff\"").unwrap();
    /// assert_eq!(value["a"], [1, 255]);
    /// ```
    ///
    /// The format is placed into the state (see [`deser::adapters::bytes`]).  Values
    /// that use an adapter for bytes are not affected.
    pub const fn bytes(mut self, format: BytesFormat) -> DeserializerConfig {
        self.bytes = format;
        self
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser::State::input_range)).  When
    /// enabled additionally the input is set as source (see
    /// [`State::source`](deser::State::source)) which allows resolving the
    /// ranges into lines and columns, for instance with the `Spanned` type
    /// of [`deser-location`](https://docs.rs/deser-location).  This copies
    /// the input.
    ///
    /// Tables report the location of the header that defines them (the
    /// whole document for the root table), tables created by dotted keys
    /// report the location of the key.  Arrays of tables report the
    /// location of their first header.
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfig {
        self.track_locations = yes;
        self
    }

    /// Deserializes a value from TOML.
    ///
    /// See [`from_str`](crate::from_str).
    pub fn from_str<'de, T: Deserialize<'de>>(&self, s: &'de str) -> Result<T, Error> {
        Deserializer::from_str_with_config(s, self).deserialize()
    }

    /// Deserializes a value from TOML in a byte slice.
    ///
    /// See [`from_slice`](crate::from_slice).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, Error> {
        Deserializer::from_slice_with_config(bytes, self).deserialize()
    }
}

/// Deserializes TOML.
///
/// Most of the time the [`from_str`](crate::from_str) and
/// [`from_slice`](crate::from_slice) functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed.  The
/// deserializer is useful to [`drive`](Self::drive) a custom sink.
///
/// ```
/// use deser_toml::Deserializer;
/// use std::collections::BTreeMap;
///
/// let mut de = Deserializer::from_str("a = 1\nb = 2");
/// let value: BTreeMap<String, u32> = de.deserialize().unwrap();
/// assert_eq!(value["b"], 2);
/// ```
pub struct Deserializer<'a> {
    input: &'a str,
    /// An error that is reported instead of parsing (invalid UTF-8).
    error: Option<Error>,
    config: DeserializerConfig,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer for a string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'a str) -> Deserializer<'a> {
        Deserializer::from_str_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a string with the given configuration.
    pub fn from_str_with_config(input: &'a str, config: &DeserializerConfig) -> Deserializer<'a> {
        Deserializer {
            input,
            error: None,
            config: config.clone(),
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        match str_from_utf8(input) {
            Ok(input) => Deserializer::from_str_with_config(input, config),
            Err(err) => Deserializer {
                input: "",
                error: Some(err),
                config: config.clone(),
            },
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Deserializes the document.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`Format::deserialize_with`].
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        Format::deserialize(self)
    }

    /// Parses the input and feeds the events into the given driver.
    ///
    /// The whole document is parsed before the first event is emitted, so
    /// syntax errors are reported before any value is deserialized.  Keys
    /// and strings without escape sequences are passed on borrowed from the
    /// input (see [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    /// Errors carry the location in the input (see [`Error::line`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let doc = parse(self.input)?;

        if self.config.track_locations {
            driver.state_mut().set_source(self.input);
        }
        if self.config.bytes != BytesFormat::BASE64 {
            *driver.state_mut().get_mut::<BytesFormat>() = self.config.bytes;
        }
        emit(&doc, driver).map_err(|err| err.resolve_position(self.input.as_bytes()))
    }
}

impl<'a> Format<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Emits an event with the byte range of its span.
#[inline(always)]
fn emit_at<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver<'_, '_>,
    event: E,
    span: Span,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(span.start, span.end);
    driver.emit(event)
}

/// Emits a string, borrowed if it's a slice of the input.
// the `Cow` tells if the string is a slice of the input
#[allow(clippy::ptr_arg)]
#[inline(always)]
fn emit_str<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    value: &Cow<'a, str>,
    span: Span,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(span.start, span.end);
    match *value {
        Cow::Borrowed(value) => driver.emit_borrowed(value),
        Cow::Owned(ref value) => driver.emit(value.as_str()),
    }
}

/// A container whose events are emitted, with the index of the next child.
enum Frame {
    Table(usize, usize),
    Array(usize, usize),
}

/// Emits the events of a document.
fn emit<'a>(doc: &Document<'a>, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
    let mut stack = vec![Frame::Table(ROOT, 0)];
    emit_at(driver, Event::map_start(), doc.tables[ROOT].span)?;

    while let Some(frame) = stack.last_mut() {
        let item: &Item = match *frame {
            Frame::Table(id, ref mut index) => {
                let table = &doc.tables[id];
                match table.entries.get(*index) {
                    Some(entry) => {
                        *index += 1;
                        emit_str(driver, &entry.key, entry.key_span)?;
                        &entry.item
                    }
                    None => {
                        stack.pop();
                        emit_at(driver, Event::MapEnd, table.span)?;
                        continue;
                    }
                }
            }
            Frame::Array(id, ref mut index) => {
                let array = &doc.arrays[id];
                match array.items.get(*index) {
                    Some(item) => {
                        *index += 1;
                        item
                    }
                    None => {
                        stack.pop();
                        emit_at(driver, Event::SeqEnd, array.span)?;
                        continue;
                    }
                }
            }
        };

        match item.value {
            Value::Table(id) => {
                emit_at(driver, Event::map_start(), doc.tables[id].span)?;
                stack.push(Frame::Table(id, 0));
            }
            Value::Array(id) => {
                emit_at(driver, Event::seq_start(), doc.arrays[id].span)?;
                stack.push(Frame::Array(id, 0));
            }
            Value::Str(ref value) => emit_str(driver, value, item.span)?,
            ref scalar => {
                let atom = match *scalar {
                    Value::Int(value) if value >= 0 => Atom::U64(value as u64),
                    Value::Int(value) => Atom::I64(value),
                    Value::UInt(value) => Atom::U64(value),
                    Value::Float(value) => Atom::F64(value),
                    Value::Bool(value) => Atom::Bool(value),
                    Value::Datetime(ref value) => Atom::Ext(ExtValue::borrowed(value)),
                    Value::Str(_) | Value::Table(_) | Value::Array(_) => unreachable!(),
                    Value::FloatText(_) => unreachable!("only used when serializing"),
                };
                emit_at(driver, atom, item.span)?;
            }
        }
    }

    Ok(())
}

fn str_from_utf8(bytes: &[u8]) -> Result<&str, Error> {
    #[cfg(feature = "simdutf8")]
    {
        if simdutf8::basic::from_utf8(bytes).is_ok() {
            // SAFETY: validated above
            return Ok(unsafe { std::str::from_utf8_unchecked(bytes) });
        }
    }
    std::str::from_utf8(bytes).map_err(|err| {
        Error::new(ErrorKind::Unexpected, "invalid UTF-8").with_offset(err.valid_up_to())
    })
}

/// Deserializes a value from TOML.
///
/// A TOML document is a table, so the value has to be deserializable from
/// a map (such as a struct or a map type).
///
/// This uses the default [`DeserializerConfig`].
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    Deserializer::from_str(s).deserialize()
}

/// Deserializes a value from TOML in a byte slice.
///
/// The input must be UTF-8.  Otherwise this works like [`from_str`].  This
/// uses the default [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    Deserializer::from_slice(bytes).deserialize()
}
