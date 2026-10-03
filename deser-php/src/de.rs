use alloc::string::String;
use alloc::vec::Vec;
use core::marker::PhantomData;

use deser_core::adapters::BytesEncoding;
use deser_core::de::{self, Deserialize, DeserializeDriver, deserialize_value};
use deser_core::{BytesFormat, Error};

use crate::parser::{self, syntax_error};

/// Configures how PHP's serialization format is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The method
/// [`from_slice`](Self::from_slice) works like the function of the same
/// name.  The only option is the [`Context`](deser_core::Context) (see
/// [`set_context`](Self::set_context)).
///
/// ```
/// use deser_php::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new();
/// assert_eq!(CONFIG.from_slice::<Vec<u32>>(b"a:2:{i:0;i:1;i:1;i:2;}").unwrap(), [1, 2]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeserializerConfig {
    context: deser_core::Context,
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
    /// with the configuration use this context.  A context set on the
    /// driver takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.context = context;
    }

    /// Returns the context the values are deserialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.context
    }

    /// Deserializes a value.
    ///
    /// See [`from_slice`].
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, input: &'de [u8]) -> Result<T, Error> {
        deserialize_value(|driver| self.drive_slice(input, driver))
    }

    /// Deserializes a value from a string.
    ///
    /// See [`from_str`].
    pub fn from_str<'de, T: Deserialize<'de>>(&self, input: &'de str) -> Result<T, Error> {
        self.from_slice(input.as_bytes())
    }

    /// The part of [`from_slice`](Self::from_slice) that does not depend on
    /// the type of the value, it exists once.
    fn drive_slice<'de>(
        &self,
        input: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut deserializer = Deserializer::from_slice_with_config(input, self.clone());
        de::Deserializer::drive(&mut deserializer, driver)?;
        deserializer.end()
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

/// Deserializes values of PHP's serialization format.
///
/// A deserializer reads values from a slice.  PHP reads a single value but
/// values can be concatenated, so a deserializer can be used to read more
/// than one:
///
/// ```
/// use deser_php::Deserializer;
///
/// let mut de = Deserializer::from_slice(b"i:1;s:2:\"hi\";");
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "hi");
/// assert!(de.is_end());
/// ```
///
/// To deserialize a single value, use [`from_slice`]
/// (or the method of the same name on [`DeserializerConfig`]).
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    config: DeserializerConfig,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer for a byte slice.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    pub fn from_slice_with_config(input: &'a [u8], config: DeserializerConfig) -> Deserializer<'a> {
        Deserializer {
            input,
            pos: 0,
            config,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the current offset in the input.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Returns `true` if the entire input was consumed.
    pub fn is_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Fails if the input was not consumed entirely.
    ///
    /// Unlike PHP (which ignores data after the value with a warning) the
    /// functions that deserialize a single value reject it.
    pub fn end(&self) -> Result<(), Error> {
        if self.is_end() {
            Ok(())
        } else {
            Err(syntax_error(self.pos, "trailing data after value"))
        }
    }

    /// Deserializes the next value.
    ///
    /// This does not check if there is more data after the value.  Use
    /// [`end`](Self::end) for this or [`from_slice`] which does it
    /// automatically.
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

    /// Returns an iterator over the remaining values.
    ///
    /// The iterator stops after the first error.
    ///
    /// ```
    /// let mut de = deser_php::Deserializer::from_slice(b"i:1;i:2;i:3;");
    /// let items = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    /// assert_eq!(items, [1, 2, 3]);
    /// ```
    pub fn iter<T: Deserialize<'a>>(&mut self) -> Iter<'_, 'a, T> {
        Iter {
            de: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Parses the next value and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser_core::de::Sink) or to wrap the sink of a value.
    ///
    /// The value is validated before the first event is emitted.  Strings
    /// are passed on borrowed from the input.  The byte ranges of the
    /// values are published as input ranges (see
    /// [`State::input_range`](deser_core::State::input_range)) and errors
    /// carry the offset in the input (see [`Error::offset`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        // strings are bytes in PHP, types that expect bytes take them as
        // they are
        driver
            .state_mut()
            .set_default(BytesFormat::encoded::<PhpStrings>());
        let scan = parser::scan(self.input, self.pos)?;
        parser::emit(self.input, self.pos, &scan.lists, driver)?;
        self.pos = scan.end;
        Ok(())
    }
}

/// An iterator over concatenated values.
///
/// See [`Deserializer::iter`].
pub struct Iter<'b, 'a, T> {
    de: &'b mut Deserializer<'a>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<'b, 'a, T: Deserialize<'a>> Iterator for Iter<'b, 'a, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.de.is_end() {
            return None;
        }
        let rv = self.de.deserialize();
        self.failed = rv.is_err();
        Some(rv)
    }
}

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if !self.config.context.is_empty() {
            driver.set_default_context(self.config.context.clone());
        }
        Deserializer::drive(self, driver)
    }
}

/// The bytes of PHP strings are the bytes of the text.
///
/// This is the default [`BytesFormat`] of the deserialization: PHP strings
/// are bytes, the ones that are valid UTF-8 are passed on as text.  Types
/// that expect bytes take their bytes rather than decoding them as base64.
struct PhpStrings;

impl BytesEncoding for PhpStrings {
    const NAME: &'static str = "php string";

    fn encode(bytes: &[u8], out: &mut String) {
        // only used for decoding, bytes that are not UTF-8 are bytes in PHP
        out.push_str(&String::from_utf8_lossy(bytes));
    }

    fn decode(s: &str) -> Result<Vec<u8>, Error> {
        Ok(s.as_bytes().to_vec())
    }
}

/// Deserializes a value of PHP's serialization format.
///
/// The input must contain exactly one value.  This uses the default
/// [`DeserializerConfig`].
///
/// ```
/// use std::collections::BTreeMap;
///
/// let input = br#"a:2:{s:4:"name";s:5:"deser";s:4:"tags";a:2:{i:0;s:1:"a";i:1;s:1:"b";}}"#;
/// #[derive(deser::Deserialize)]
/// struct Package {
///     name: String,
///     tags: Vec<String>,
/// }
/// let package: Package = deser_php::from_slice(input).unwrap();
/// assert_eq!(package.name, "deser");
/// assert_eq!(package.tags, ["a", "b"]);
/// ```
pub fn from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(input)
}

/// Deserializes a value of PHP's serialization format from a string.
///
/// This is [`from_slice`] for input that is held in a string.
///
/// ```
/// let value: Vec<u32> = deser_php::from_str("a:2:{i:0;i:1;i:1;i:2;}").unwrap();
/// assert_eq!(value, [1, 2]);
/// ```
pub fn from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    DeserializerConfig::new().from_str(input)
}
