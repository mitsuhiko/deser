use core::marker::PhantomData;

use deser_core::Error;
use deser_core::de::{self, Deserialize, DeserializeDriver, deserialize_value};

use crate::emit::emit;
use crate::vm::{Machine, syntax_error};

/// The default of [`DeserializerConfig::set_max_shared_events`].
const DEFAULT_MAX_SHARED_EVENTS: usize = 1 << 20;

/// Configures how pickles are deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The method
/// [`from_slice`](Self::from_slice) works like the function of the same
/// name.
///
/// ```
/// use deser_pickle::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new();
/// // `pickle.dumps([1, 2], 4)`
/// let input = b"\x80\x04\x95\x09\x00\x00\x00\x00\x00\x00\x00]\x94(K\x01K\x02e.";
/// assert_eq!(CONFIG.from_slice::<Vec<u32>>(input).unwrap(), [1, 2]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    context: deser_core::Context,
    max_shared_events: usize,
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
            max_shared_events: DEFAULT_MAX_SHARED_EVENTS,
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

    /// Limits the events of values that are emitted more than once.
    ///
    /// Values that are reached more than once are emitted at every place
    /// (see [References](crate#references)).  A small pickle can reach the
    /// same values very often (each level of a list that holds the level
    /// below twice doubles the output), this limits the number of events
    /// that are emitted for values that were emitted before.  The default
    /// is 1048576.
    pub fn set_max_shared_events(&mut self, max: usize) {
        self.max_shared_events = max;
    }

    /// Returns the limit of the events of values that are emitted more
    /// than once.
    pub fn max_shared_events(&self) -> usize {
        self.max_shared_events
    }

    /// Deserializes a value.
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

    /// Limits the events of values that are emitted more than once.
    ///
    /// See [`DeserializerConfig::set_max_shared_events`].
    pub const fn max_shared_events(mut self, max: usize) -> DeserializerConfigBuilder {
        self.value.max_shared_events = max;
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

/// Deserializes pickles.
///
/// A deserializer reads values from a slice.  Pickles end with a `STOP`
/// opcode, so they can be concatenated (like the pickles `pickle.dump`
/// writes to a file one after another) and a deserializer can read more
/// than one:
///
/// ```
/// use deser_pickle::Deserializer;
///
/// // `pickle.dumps(1, 4) + pickle.dumps("hi", 4)`
/// let input = b"\x80\x04K\x01.\x80\x04\x95\x06\x00\x00\x00\x00\x00\x00\x00\x8c\x02hi\x94.";
/// let mut de = Deserializer::from_slice(input);
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
    /// Unlike Python (which ignores data after the `STOP` opcode) the
    /// functions that deserialize a single value reject it.
    pub fn end(&self) -> Result<(), Error> {
        if self.is_end() {
            Ok(())
        } else {
            Err(syntax_error(self.pos, "trailing data after STOP"))
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
    /// let mut de = deser_pickle::Deserializer::from_slice(b"K\x01.K\x02.K\x03.");
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

    /// Runs the next pickle and feeds the events of its value into the
    /// given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser_core::de::Sink) or to wrap the sink of a value.
    ///
    /// The pickle is run to its end before the first event is emitted.
    /// Strings are passed on borrowed from the input where they can be.
    /// The byte ranges of the opcodes that created the values are
    /// published as input ranges (see
    /// [`State::input_range`](deser_core::State::input_range)) and errors
    /// carry the offset in the input (see [`Error::offset`]).
    ///
    /// The context of the configuration is given to the driver (values that
    /// the context of the driver has take precedence, see
    /// [`DeserializeDriver::set_default_context`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if !self.config.context.is_empty() {
            driver.set_default_context(self.config.context.clone());
        }
        let (graph, end) = Machine::new(self.input, self.pos).run()?;
        emit(&graph, driver, self.config.max_shared_events)?;
        self.pos = end;
        Ok(())
    }
}

/// An iterator over concatenated pickles.
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

/// Deserializes a pickle.
///
/// The input must contain exactly one pickle.  This uses the default
/// [`DeserializerConfig`].
///
/// ```
/// #[derive(deser::Deserialize)]
/// struct Package {
///     name: String,
///     tags: Vec<String>,
/// }
///
/// // `pickle.dumps({"name": "deser", "tags": ["a", "b"]}, 4)`
/// let input = b"\x80\x04\x95'\x00\x00\x00\x00\x00\x00\x00}\x94(\x8c\x04name\x94\x8c\x05deser\x94\x8c\x04tags\x94]\x94(\x8c\x01a\x94\x8c\x01b\x94eu.";
/// let package: Package = deser_pickle::from_slice(input).unwrap();
/// assert_eq!(package.name, "deser");
/// assert_eq!(package.tags, ["a", "b"]);
/// ```
pub fn from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(input)
}
