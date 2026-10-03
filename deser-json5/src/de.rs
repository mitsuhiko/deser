// @generated from deser-template-json/src/de.rs by
// deser-template-json/generate.py.  Do not edit.
use alloc::string::String;
use alloc::sync::Arc;
use core::marker::PhantomData;
use core::str;

use deser_core::de::{self, Deserialize, DeserializeDriver, deserialize_value};
use deser_core::{Error, ErrorKind, Source};

use crate::Trailing;
use crate::parser::{Borrowing, Cursor, Options, Parser, Progress};
use crate::scan::LineScan;

/// Configures how JSON is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.  To create a [`Deserializer`] with
/// the configuration use [`Deserializer::from_str_with_config`] or
/// [`Deserializer::from_slice_with_config`].
///
/// ```
/// use deser_json5::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig =
///     DeserializerConfig::builder().exact_numbers(false).build();
/// let value: Vec<f64> = CONFIG.from_str("[0.10, 1e5]").unwrap();
/// assert_eq!(value, [0.1, 1e5]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    track_locations: bool,
    exact_numbers: bool,
    trailing: Trailing,
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
            exact_numbers: true,
            trailing: Trailing::Strict,
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

    /// Controls what may follow a value.
    ///
    /// By default ([`Trailing::Strict`]) only whitespace may follow the
    /// value.  [`Trailing::Newline`] reads [JSON
    /// Lines](https://jsonlines.org/) and [`Trailing::Stop`] stops after
    /// the value without looking at what follows:
    ///
    /// ```
    /// use deser_json5::{DeserializerConfig, Trailing};
    ///
    /// assert!(
    ///     deser_json5::from_str::<Vec<u32>>("[1] trash")
    ///         .is_err()
    /// );
    /// const STOP: DeserializerConfig =
    ///     DeserializerConfig::builder().trailing(Trailing::Stop).build();
    /// assert_eq!(STOP.from_str::<Vec<u32>>("[1] trash").unwrap(), [1]);
    /// ```
    ///
    /// With [`Trailing::Newline`] a [`Deserializer`] reads the lines one by
    /// one.  Errors only discard their line:
    ///
    /// ```
    /// use deser_json5::{
    ///     Deserializer, DeserializerConfig, Trailing,
    /// };
    ///
    /// const LINES: DeserializerConfig =
    ///     DeserializerConfig::builder().trailing(Trailing::Newline).build();
    /// let mut de =
    ///     Deserializer::from_str_with_config("1\n\nnope\n3\n", &LINES);
    /// let mut values = Vec::new();
    /// while !de.is_end() {
    ///     match de.deserialize::<u32>() {
    ///         Ok(value) => values.push(value),
    ///         Err(err) => assert_eq!(err.line(), Some(3)),
    ///     }
    /// }
    /// assert_eq!(values, [1, 3]);
    /// ```
    pub const fn set_trailing(&mut self, trailing: Trailing) {
        self.trailing = trailing;
    }

    /// Returns what may follow a value.
    pub(crate) fn trailing_mode(&self) -> Trailing {
        self.trailing
    }

    /// Returns `true` if exact numbers are enabled.
    pub(crate) fn exact_numbers_enabled(&self) -> bool {
        self.exact_numbers
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser_core::State::input_range)).  When
    /// enabled additionally the input is set as source (see
    /// [`Source`]) which allows resolving the
    /// ranges into lines and columns, for instance with the `Spanned` type
    /// of [`deser-location`](https://docs.rs/deser-location).  This copies
    /// the input.
    pub const fn set_track_locations(&mut self, yes: bool) {
        self.track_locations = yes;
    }

    /// Enables or disables exact numbers.
    ///
    /// When enabled (which is the default) floats which lose precision as
    /// `f64` and integers that do not fit into 128 bits are emitted as
    /// [`Number`](deser_core::ext::Number) extension values.  These carry the
    /// text of the number together with its value as `f64`, which is what
    /// types that do not know about exact numbers receive.  Types like
    /// [`Decimal`](deser_core::ext::Decimal) (and the types of `rust_decimal` or
    /// `bigdecimal`) use the text to deserialize the number exactly:
    ///
    /// ```
    /// use deser::ext::Decimal;
    ///
    /// let value: Decimal =
    ///     deser_json5::from_str("0.10000000000000000001")
    ///         .unwrap();
    /// assert_eq!(value.as_str(), "0.10000000000000000001");
    /// let value: f64 =
    ///     deser_json5::from_str("0.10000000000000000001")
    ///         .unwrap();
    /// assert_eq!(value, 0.1);
    /// ```
    ///
    /// Floats whose text is the shortest representation of their value (as
    /// formatted by `Debug`, for instance `0.5` or `3.14`) are emitted as
    /// plain floats as the text can be recovered from the value.  This keeps
    /// the common case fast.  When disabled, all floats are emitted as plain
    /// floats.
    pub const fn set_exact_numbers(&mut self, yes: bool) {
        self.exact_numbers = yes;
    }

    /// Deserializes JSON from the given string.
    ///
    /// What may follow the value depends on [`set_trailing`](Self::set_trailing).
    /// With [`Trailing::Newline`] this reads the first line.
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
        de::Deserializer::drive(&mut Deserializer::from_str_with_config(s, self), driver)
    }

    /// Deserializes JSON from the given bytes.
    ///
    /// The input must be UTF-8.  Rather than validating the input upfront,
    /// the strings are validated while parsing (see
    /// [`Deserializer::from_slice`]).
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
            &mut Deserializer::from_slice_with_config(bytes, self),
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

    /// Controls what may follow a value.
    ///
    /// See [`DeserializerConfig::set_trailing`].
    pub const fn trailing(mut self, trailing: Trailing) -> DeserializerConfigBuilder {
        self.value.set_trailing(trailing);
        self
    }

    /// Enables or disables location tracking.
    ///
    /// See [`DeserializerConfig::set_track_locations`].
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_track_locations(yes);
        self
    }

    /// Enables or disables exact numbers.
    ///
    /// See [`DeserializerConfig::set_exact_numbers`].
    pub const fn exact_numbers(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_exact_numbers(yes);
        self
    }

    /// Returns the built [`DeserializerConfig`].
    pub const fn build(self) -> DeserializerConfig {
        self.value
    }
}

impl Default for DeserializerConfigBuilder {
    fn default() -> DeserializerConfigBuilder {
        DeserializerConfigBuilder::new()
    }
}

/// Deserializes a serializable from JSON.
///
/// Every call to [`deserialize`](Self::deserialize) reads the next value.
/// What may follow a value is controlled by
/// [`DeserializerConfig::set_trailing`].  By default only whitespace may follow
/// so there is only a single value.  With [`Trailing::Newline`] the
/// deserializer reads [JSON Lines](https://jsonlines.org/):
///
/// ```
/// use deser_json5::{
///     Deserializer, DeserializerConfig, Trailing,
/// };
///
/// let config = DeserializerConfig::builder().trailing(Trailing::Newline).build();
/// let mut de =
///     Deserializer::from_str_with_config("[1, 2]\n[3]\n", &config);
/// assert_eq!(de.deserialize::<Vec<u32>>().unwrap(), [1, 2]);
/// assert_eq!(de.deserialize::<Vec<u32>>().unwrap(), [3]);
/// assert!(de.is_end());
/// ```
///
/// To deserialize a single value, use [`from_str`] and
/// [`from_slice`] (or the methods of the same name on
/// [`DeserializerConfig`]).  The deserializer is also useful to
/// [`drive`](Self::drive) a custom sink.
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    parser: Parser,
    // `true` if the input is a byte slice which needs to be validated
    validate_utf8: bool,
    // `true` if a value failed and the stream cannot be continued
    failed: bool,
    // the input as source for location tracking, shared by all values
    source: Option<Arc<str>>,
    config: DeserializerConfig,
    // the context the values are deserialized in
    context: deser_core::Context,
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
            // the parser works on bytes but relies on the input being valid
            // UTF-8 when it hands out string slices.
            input: input.as_bytes(),
            validate_utf8: false,
            pos: 0,
            parser: Parser::default(),
            failed: false,
            source: None,
            config: config.clone(),
            context: deser_core::Context::new(),
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input is not validated upfront.  Instead strings are validated as
    /// UTF-8 when they are parsed (bytes outside of strings are only ever
    /// accepted if they are ASCII).  Invalid UTF-8 is an error.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    ///
    /// See [`from_slice`](Self::from_slice).
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        Deserializer {
            input,
            validate_utf8: true,
            pos: 0,
            parser: Parser::default(),
            failed: false,
            source: None,
            config: config.clone(),
            context: deser_core::Context::new(),
        }
    }

    /// Creates a deserializer for the frame of a value in a stream.
    ///
    /// Only whitespace may follow the value in the frame.
    pub(crate) fn from_frame(input: &'a [u8], config: &DeserializerConfig) -> Deserializer<'a> {
        let mut de = Deserializer::from_slice_with_config(input, config);
        de.config.trailing = Trailing::Strict;
        de
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the current offset in the input.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Returns `true` if there are no more values.
    ///
    /// This is the case if only whitespace is left or if a value failed and
    /// the stream cannot be continued (see [`deserialize`](Self::deserialize)).
    pub fn is_end(&self) -> bool {
        self.failed || self.next_token() == self.input.len()
    }

    /// Fails if there is more than whitespace left.
    ///
    /// This is useful with [`Trailing::Stop`] to check that the input was
    /// consumed.
    pub fn end(&self) -> Result<(), Error> {
        if self.is_end() {
            return Ok(());
        }
        let mut err =
            Error::with_offset(ErrorKind::Syntax, "garbage after input", self.next_token());
        err.resolve_position(self.input);
        Err(err)
    }

    /// Returns where the next token starts (or the end of the input).
    fn next_token(&self) -> usize {
        let mut cursor = Cursor::new(self.input, self.pos);
        cursor.parse_whitespace();
        cursor.pos
    }

    /// Returns the input as string for the source.
    fn source(&self) -> alloc::borrow::Cow<'a, str> {
        if self.validate_utf8 {
            // invalid UTF-8 fails the parsing when reached, the offsets of
            // the tokens before it are not affected by the replacements.
            String::from_utf8_lossy(self.input)
        } else {
            // SAFETY: the input was created from a string
            alloc::borrow::Cow::Borrowed(unsafe { str::from_utf8_unchecked(self.input) })
        }
    }

    /// Deserializes the next value.
    ///
    /// What may follow the value depends on
    /// [`DeserializerConfig::set_trailing`].  Fails with
    /// [`ErrorKind::EndOfFile`] if there are no more values.
    ///
    /// If a value fails to deserialize (because it's malformed or does not
    /// match the type), the stream ends: [`is_end`](Self::is_end) returns
    /// `true` and further calls fail.  With [`Trailing::Newline`] only the
    /// rest of the line is skipped and the next call continues with the
    /// next line.
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
    /// This is useful to read JSON Lines (see [`Trailing::Newline`]).  The
    /// iterator stops after the first error.
    ///
    /// ```
    /// use deser_json5::{
    ///     Deserializer, DeserializerConfig, Trailing,
    /// };
    ///
    /// let config = DeserializerConfig::builder().trailing(Trailing::Newline).build();
    /// let mut de = Deserializer::from_str_with_config("1\n2\n3\n", &config);
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
    /// This is useful to deserialize into a custom [`Sink`](deser_core::de::Sink).
    /// See also [`deserialize_with`](Self::deserialize_with).
    ///
    /// Strings without escape sequences are passed on borrowed from the
    /// input (see [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    /// Errors carry the location in the input (see [`Error::line`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if self.failed {
            return Err(Error::new(
                ErrorKind::InvalidState,
                "cannot continue after an error",
            ));
        }
        if self.config.track_locations {
            let source = match self.source {
                Some(ref source) => source.clone(),
                None => {
                    let source: Arc<str> = self.source().into();
                    self.source = Some(source.clone());
                    source
                }
            };
            Source(source).set(driver.state_mut());
        }

        // for JSON Lines the input is cut off at the end of the line.  The
        // parser then fails if the value does not end on the line.
        let input = self.input;
        let line_end = if self.config.trailing == Trailing::Newline {
            self.skip_whitespace();
            // line breaks in comments and strings do not end the line
            let end = LineScan::default()
                .find_end(input, self.pos)
                .unwrap_or(input.len());
            self.input = &input[..end];
            Some(end)
        } else {
            None
        };

        let rv = self.drive_impl(driver);
        self.input = input;

        let rv = rv.map_err(|err| self.locate_error(err));
        match line_end {
            // the rest of the line is skipped, even after errors
            Some(end) => self.pos = end,
            // after an error the position within the value is unknown
            None => self.failed = rv.is_err() && !self.is_end(),
        }
        rv
    }

    /// Attaches the location to an error.
    #[cold]
    fn locate_error(&self, mut err: Error) -> Error {
        // errors of the parser are located at the current position, errors
        // of the sinks at the event that failed
        if err.offset().is_none() {
            err.set_offset(self.pos);
        }
        err.resolve_position(self.input);
        err
    }

    fn drive_impl(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        let options = Options {
            validate_utf8: self.validate_utf8,
            exact_numbers: self.config.exact_numbers,
        };
        let mut out = Borrowing(driver);
        match self
            .parser
            .parse(self.input, self.pos, true, 0, options, &mut out)
        {
            Ok(Progress::Done(pos)) => {
                self.pos = pos;
                self.finish_value()
            }
            Ok(Progress::NeedMore(_)) => unreachable!("the input is complete"),
            Err(err) => {
                self.parser.reset();
                Err(err)
            }
        }
    }

    /// Skips whitespace.
    fn skip_whitespace(&mut self) {
        self.pos = self.next_token();
    }

    /// Checks what follows a complete value.
    #[inline]
    fn finish_value(&mut self) -> Result<(), Error> {
        let msg = match self.config.trailing {
            Trailing::Strict => "garbage after input",
            // the input was cut off at the end of the line
            Trailing::Newline => "expected end of line after value",
            Trailing::Stop => return Ok(()),
        };
        // an unterminated comment does not reach the end of the input
        self.skip_whitespace();
        if self.pos < self.input.len() {
            return Err(Error::new(ErrorKind::Syntax, msg));
        }
        Ok(())
    }

    /// Sets the context the values are deserialized in.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)).  A context set
    /// on the driver (for instance in the setup callback of `deserialize_with`)
    /// takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.context = context;
    }

    /// Returns the context the values are deserialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.context
    }
}

/// An iterator over the values of a JSON stream.
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
        if !self.context.is_empty() {
            driver.state_mut().set_default_context(self.context.clone());
        }
        Deserializer::drive(self, driver)
    }
}

/// Deserializes JSON from the given string.
///
/// The input must contain exactly one value, only whitespace may follow it.
/// To read multiple values (for instance JSON Lines) use a [`Deserializer`]
/// with [`Trailing::Newline`].  This uses the default
/// [`DeserializerConfig`].
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    DeserializerConfig::new().from_str(s)
}

/// Deserializes JSON from the given bytes.
///
/// The input must be UTF-8.  Rather than validating the input upfront, the
/// strings are validated while parsing (see [`Deserializer::from_slice`]).
/// This uses the default [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(bytes)
}
