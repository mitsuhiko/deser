use std::borrow::Cow;

use deser_core::Text;
use deser_core::de::{self, Deserialize, DeserializeDriver, deserialize_value};
use deser_core::ext::ExtValue;
use deser_core::hints::Layout;
use deser_core::{Atom, ContainerShape, Error, ErrorKind, Event, Source, TrackLocations};

use crate::document::{Document, Item, Span, TableKind, Value};
use crate::parser::{ROOT, parse};

/// Configures how TOML is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.  To create a [`Deserializer`] with
/// the configuration use [`Deserializer::from_str_with_config`] or
/// [`Deserializer::from_slice_with_config`].  The only option is the
/// [`Context`](deser_core::Context) (see [`set_context`](Self::set_context)),
/// which holds what is configured from the outside (like
/// [`TrackLocations`]).
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_toml::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new();
/// let value: BTreeMap<String, u32> = CONFIG.from_str("a = 1").unwrap();
/// assert_eq!(value["a"], 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    context: deser_core::Context,
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
            context: deser_core::Context::new(),
        }
    }

    /// Returns a builder for the configuration (see [`DeserializerConfigBuilder`]).
    pub const fn builder() -> DeserializerConfigBuilder {
        DeserializerConfigBuilder::new()
    }

    /// Returns a builder that starts with this configuration.
    pub const fn into_builder(self) -> DeserializerConfigBuilder {
        DeserializerConfigBuilder { value: self }
    }

    /// Sets the context the values are deserialized in.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)), for instance
    /// the variants of open enums.  The deserializers and readers created
    /// with the configuration use this context.  A context set
    /// on the driver takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.context = context;
    }

    /// Returns the configuration without its context (for the frames of
    /// streams, which get the context of the stream).
    pub(crate) fn without_context(&self) -> DeserializerConfig {
        let mut config = self.clone();
        config.context = deser_core::Context::default();
        config
    }

    /// Returns the context the values are deserialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.context
    }

    /// Deserializes a value from TOML.
    ///
    /// See [`from_str`].
    pub fn from_str<'de, T: Deserialize<'de>>(&self, s: &'de str) -> Result<T, Error> {
        deserialize_value(|driver| self.drive_str(s, driver))
    }

    /// The part of [`from_str`](Self::from_str) that does not depend on the type
    /// of the value, it exists once.
    fn drive_str<'de>(
        &self,
        s: &'de str,
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        de::Deserializer::drive(
            &mut Deserializer::from_str_with_config(s, self.clone()),
            driver,
        )
    }

    /// Deserializes a value from TOML in a byte slice.
    ///
    /// See [`from_slice`].
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, Error> {
        deserialize_value(|driver| self.drive_slice(bytes, driver))
    }

    /// The part of [`from_slice`](Self::from_slice) that does not depend on the type
    /// of the value, it exists once.
    fn drive_slice<'de>(
        &self,
        bytes: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        de::Deserializer::drive(
            &mut Deserializer::from_slice_with_config(bytes, self.clone()),
            driver,
        )
    }
}

/// Builds a [`DeserializerConfig`].
///
/// The methods have the names of the setters of [`DeserializerConfig`] (without `set_`).
#[derive(Debug, Clone)]
#[must_use]
pub struct DeserializerConfigBuilder {
    value: DeserializerConfig,
}

impl DeserializerConfigBuilder {
    /// Creates a builder that starts with the default.
    pub const fn new() -> DeserializerConfigBuilder {
        DeserializerConfigBuilder {
            value: DeserializerConfig::new(),
        }
    }

    /// Sets the context the values are deserialized in.
    ///
    /// See [`DeserializerConfig::set_context`].
    pub fn context(mut self, context: deser_core::Context) -> DeserializerConfigBuilder {
        self.value.set_context(context);
        self
    }

    /// Returns the built [`DeserializerConfig`].
    pub const fn build(self) -> DeserializerConfig {
        // the value cannot be moved out of the builder in a const fn as the
        // builder needs dropping (the context has a destructor)
        // SAFETY: the value is read once and the builder is forgotten
        let value = unsafe { core::ptr::read(&self.value) };
        core::mem::forget(self);
        value
    }
}

impl Default for DeserializerConfigBuilder {
    fn default() -> DeserializerConfigBuilder {
        DeserializerConfigBuilder::new()
    }
}

/// Deserializes TOML.
///
/// Most of the time the [`from_str`] and
/// [`from_slice`] functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed.  The
/// deserializer is useful to [`drive`](Self::drive) a custom sink.
///
/// With [`TrackLocations`] in the context, tables report the location of
/// the header that defines them (the whole document for the root table),
/// tables created by dotted keys report the location of the key.  Arrays
/// of tables report the location of their first header.
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
        Deserializer::from_str_with_config(input, DeserializerConfig::new())
    }

    /// Creates a new deserializer for a string with the given configuration.
    pub fn from_str_with_config(input: &'a str, config: DeserializerConfig) -> Deserializer<'a> {
        Deserializer {
            input,
            error: None,
            config,
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice_with_config(input: &'a [u8], config: DeserializerConfig) -> Deserializer<'a> {
        match str_from_utf8(input) {
            Ok(input) => Deserializer::from_str_with_config(input, config),
            Err(err) => Deserializer {
                input: "",
                error: Some(err),
                config,
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
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the next value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser_core::de::Layer)s.
    pub fn deserialize_with<T, F>(&mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        de::Deserializer::deserialize_with(self, setup)
    }

    /// Parses the input and feeds the events into the given driver.
    ///
    /// The whole document is parsed before the first event is emitted, so
    /// syntax errors are reported before any value is deserialized.  Keys
    /// and strings without escape sequences are passed on borrowed from the
    /// input (see [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    /// Errors carry the location in the input (see [`Error::line`]).
    ///
    /// The context of the configuration is given to the driver (values that
    /// the context of the driver has take precedence, see
    /// [`DeserializeDriver::set_default_context`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if !self.config.context.is_empty() {
            driver.set_default_context(self.config.context.clone());
        }
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let doc = parse(self.input)?;

        if TrackLocations::of(driver.state()) {
            Source(self.input.into()).set(driver.state_mut());
        }
        emit(&doc, driver).map_err(|mut err| {
            err.resolve_position(self.input.as_bytes());
            err
        })
    }
}

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
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

/// Emits a key.
///
/// Keys are always strings in TOML, they are lexical: they can stand for
/// values of other types (like integers).
// the key is a `Cow` as borrowed keys are passed on for `'a`
#[allow(clippy::ptr_arg)]
fn emit_key<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    key: &Cow<'a, str>,
    span: Span,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(span.start, span.end);
    match *key {
        Cow::Borrowed(key) => driver.emit_borrowed(Atom::Lexical(Text::borrowed(key))),
        Cow::Owned(ref key) => driver.emit(Atom::Lexical(Text::borrowed(key.as_str()))),
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
    let shape = ContainerShape::with_len(doc.tables[ROOT].entries.len());
    emit_at(driver, Event::MapStart(shape), doc.tables[ROOT].span)?;

    while let Some(frame) = stack.last_mut() {
        let item: &Item = match *frame {
            Frame::Table(id, ref mut index) => {
                let table = &doc.tables[id];
                match table.entries.get(*index) {
                    Some(entry) => {
                        *index += 1;
                        emit_key(driver, &entry.key, entry.key_span)?;
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
                let table = &doc.tables[id];
                // inline tables are compact so that they stay inline when
                // they are serialized again
                if table.kind == TableKind::Inline {
                    Layout::Compact.set(driver.state_mut());
                }
                let shape = ContainerShape::with_len(table.entries.len());
                emit_at(driver, Event::MapStart(shape), table.span)?;
                stack.push(Frame::Table(id, 0));
            }
            Value::Array(id) => {
                let array = &doc.arrays[id];
                // same for inline arrays of tables
                if !array.of_tables
                    && array
                        .items
                        .first()
                        .is_some_and(|x| matches!(x.value, Value::Table(_)))
                {
                    Layout::Compact.set(driver.state_mut());
                }
                let shape = ContainerShape::with_len(array.items.len());
                emit_at(driver, Event::SeqStart(shape), array.span)?;
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
                    Value::Float32(_) | Value::FloatText(_) => {
                        unreachable!("only used when serializing")
                    }
                };
                emit_at(driver, atom, item.span)?;
            }
        }
    }

    Ok(())
}

fn str_from_utf8(bytes: &[u8]) -> Result<&str, Error> {
    #[cfg(feature = "speedups")]
    {
        if simdutf8::basic::from_utf8(bytes).is_ok() {
            // SAFETY: validated above
            return Ok(unsafe { std::str::from_utf8_unchecked(bytes) });
        }
    }
    std::str::from_utf8(bytes)
        .map_err(|err| Error::with_offset(ErrorKind::Syntax, "invalid UTF-8", err.valid_up_to()))
}

/// Deserializes a value from TOML.
///
/// A TOML document is a table, so the value has to be deserializable from
/// a map (such as a struct or a map type).
///
/// This uses the default [`DeserializerConfig`].
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    DeserializerConfig::new().from_str(s)
}

/// Deserializes a value from TOML in a byte slice.
///
/// The input must be UTF-8.  Otherwise this works like [`from_str`].  This
/// uses the default [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(bytes)
}
