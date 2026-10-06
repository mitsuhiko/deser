use core::marker::PhantomData;

use deser_core::Error;
use deser_core::de::{self, Deserialize, DeserializeDriver, deserialize_value};

use crate::parser::{Borrowing, Parser, Progress, syntax_error};

/// Configures how MessagePack is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The method
/// [`from_slice`](Self::from_slice) works like the function of the same
/// name.  To read multiple items with the configuration create a
/// [`Deserializer`] with [`Deserializer::from_slice_with_config`].
///
/// ```
/// use deser_msgpack::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new();
/// assert_eq!(CONFIG.from_slice::<Vec<u32>>(&[0x92, 0x01, 0x02]).unwrap(), [1, 2]);
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

    /// Deserializes a value from MessagePack.
    ///
    /// See [`from_slice`].
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, input: &'de [u8]) -> Result<T, Error> {
        deserialize_value(|driver| self.drive_slice(input, driver))
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

/// Deserializes a deserializable from MessagePack.
///
/// A deserializer reads items from a slice.  Because MessagePack streams are
/// just items following each other, a deserializer can be used to read more
/// than one item:
///
/// ```
/// use deser_msgpack::Deserializer;
///
/// let mut de = Deserializer::from_slice(&[0x01, 0xa2, b'h', b'i']);
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "hi");
/// assert!(de.is_end());
/// ```
///
/// To deserialize a single item, use [`from_slice`]
/// (or the method of the same name on [`DeserializerConfig`]).
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    config: DeserializerConfig,
    parser: Parser,
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
            parser: Parser::default(),
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
    pub fn end(&self) -> Result<(), Error> {
        if self.is_end() {
            Ok(())
        } else {
            Err(syntax_error(self.pos, "trailing data after item"))
        }
    }

    /// Deserializes the next item.
    ///
    /// This does not check if there is more data after the item.  Use
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

    /// Returns an iterator over the remaining items.
    ///
    /// This is useful to read streams of MessagePack items.  The iterator stops after
    /// the first error.
    ///
    /// ```
    /// let mut de = deser_msgpack::Deserializer::from_slice(&[0x01, 0x02, 0x03]);
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

    /// Parses the next item and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser_core::de::Sink) or to wrap the sink of a value.
    ///
    /// Strings and binary data are passed on borrowed from the input (see
    /// [`emit_borrowed`](DeserializeDriver::emit_borrowed)).  The byte
    /// ranges of the items are published as input ranges (see
    /// [`State::input_range`](deser_core::State::input_range)) and errors carry
    /// the offset in the input (see [`Error::offset`]).
    ///
    /// The context of the configuration is given to the driver (values that
    /// the context of the driver has take precedence, see
    /// [`DeserializeDriver::set_default_context`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if !self.config.context.is_empty() {
            driver.set_default_context(self.config.context.clone());
        }
        match self
            .parser
            .parse(self.input, self.pos, true, 0, &mut Borrowing(driver))
        {
            Ok(Progress::Done(pos)) => {
                self.pos = pos;
                Ok(())
            }
            Ok(Progress::NeedMore(_)) => unreachable!("the input is complete"),
            Err(err) => {
                // the next item is read from where the parser stopped
                self.pos = self.parser.position();
                self.parser.reset();
                Err(err)
            }
        }
    }
}

/// An iterator over the items of a MessagePack stream.
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
        Deserializer::drive(self, driver)
    }
}

/// Deserializes a value from MessagePack.
///
/// The input must contain exactly one item.  This uses the default
/// [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(input)
}
