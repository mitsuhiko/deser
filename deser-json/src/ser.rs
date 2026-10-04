use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;

use deser_core::ext::{BigInt, Decimal, ExtValue, Number, RawInput};
use deser_core::ser::SerializeRef;
use deser_core::ser::{self, EventSink, SerializeDriver};
use deser_core::{Atom, BytesFormat, Error, ErrorKind, Event, Implicit, ImplicitValue, Serialize};

use crate::Trailing;
use crate::buf::Buffer;
use crate::escape::find_escape;
use crate::pretty::PrettyWriter;
use crate::scan::skip_to_escape;

/// How the output is indented.
///
/// See [`SerializerConfig::set_indent`] and [`SerializerConfig::set_pretty`].
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
/// Maps and sequences with the [`Layout::Compact`](deser_core::hints::Layout)
/// hint are always written on a single line, the ones with
/// [`Layout::Expanded`](deser_core::hints::Layout) never (unless they are in a
/// map or sequence on a single line).  See [`SerializerConfig::set_inline`].
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
/// spaces.  [`set_pretty`](Self::set_pretty) writes every entry on a line of its
/// own:
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_json::{Indent, SerializerConfig};
///
/// let value = BTreeMap::from([("name", vec!["a", "b"])]);
/// assert_eq!(
///     deser_json::to_string(&value).unwrap(),
///     r#"{"name":["a","b"]}"#
/// );
///
/// const PRETTY: SerializerConfig =
///     SerializerConfig::builder().pretty(Indent::Spaces(2)).build();
/// assert_eq!(
///     PRETTY.to_string(&value).unwrap(),
///     "{\n  \"name\": [\n    \"a\",\n    \"b\"\n  ]\n}"
/// );
/// ```
///
/// In indented output maps and sequences with the
/// [`Layout::Compact`](deser_core::hints::Layout) hint (see
/// [`hints`](deser_core::hints)) are written on a single line.  The output never
/// ends with a line break.
///
/// [`to_string`](Self::to_string) works like the
/// [`to_string`] function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    indent: Indent,
    compact: bool,
    inline: InlinePolicy,
    trailing: Trailing,
    non_finite_floats: bool,
    context: deser_core::Context,
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
            indent: Indent::None,
            compact: true,
            inline: InlinePolicy::Never,
            trailing: Trailing::Strict,
            non_finite_floats: false,
            context: deser_core::Context::new(),
        }
    }

    /// Returns a builder for the configuration (see [`SerializerConfigBuilder`]).
    pub const fn builder() -> SerializerConfigBuilder {
        SerializerConfigBuilder::new()
    }

    /// Returns a builder that starts with this configuration.
    pub const fn into_builder(self) -> SerializerConfigBuilder {
        SerializerConfigBuilder { value: self }
    }

    /// Sets the context the values are serialized in.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)), for instance
    /// the [`BytesFormat`](deser_core::BytesFormat).  The serializers and
    /// writers created with the configuration use this context.  A context set on
    /// the driver takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.context = context;
    }

    /// Returns the context the values are serialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.context
    }

    /// Gives the context to a driver which has none.
    #[inline]
    fn apply_context(&self, driver: &mut SerializeDriver<'_>) {
        if !self.context.is_empty() {
            driver.set_default_context(self.context.clone());
        }
    }

    /// Sets what follows the values of a stream.
    ///
    /// This is the counterpart of
    /// [`DeserializerConfig::set_trailing`](crate::DeserializerConfig::set_trailing)
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
    /// const LINES: SerializerConfig =
    ///     SerializerConfig::builder().trailing(Trailing::Newline).build();
    /// let mut serializer = Serializer::with_config(LINES);
    /// serializer.serialize(&vec![1, 2]).unwrap();
    /// serializer.serialize(&vec![3]).unwrap();
    /// assert_eq!(serializer.finish(), "[1,2]\n[3]\n");
    /// ```
    pub const fn set_trailing(&mut self, trailing: Trailing) {
        self.trailing = trailing;
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
    /// separators, see [`set_compact`](Self::set_compact).  To indent with spaces
    /// after separators use [`set_pretty`](Self::set_pretty).
    ///
    /// ```
    /// use deser_json::{Indent, SerializerConfig};
    ///
    /// const TAB: SerializerConfig = SerializerConfig::builder().indent(Indent::Tab).build();
    /// assert_eq!(TAB.to_string(&vec![1, 2]).unwrap(), "[\n\t1,\n\t2\n]");
    /// ```
    pub const fn set_indent(&mut self, indent: Indent) {
        self.indent = indent;
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
    /// const SPACED: SerializerConfig = SerializerConfig::builder().compact(false).build();
    /// assert_eq!(SPACED.to_string(&value).unwrap(), r#"{"a": [1, 2]}"#);
    /// ```
    pub const fn set_compact(&mut self, yes: bool) {
        self.compact = yes;
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
    /// const CONFIG: SerializerConfig = SerializerConfig::builder()
    ///     .pretty(Indent::Spaces(2))
    ///     .inline(InlinePolicy::LeafIfFits(80)).build();
    /// assert_eq!(CONFIG.to_string(&shape).unwrap(), r#"{
    ///   "name": "line",
    ///   "points": [
    ///     [0, 0],
    ///     [3, 4]
    ///   ]
    /// }"#);
    /// ```
    ///
    /// This has no effect without [indentation](Self::set_indent).
    pub const fn set_inline(&mut self, policy: InlinePolicy) {
        self.inline = policy;
    }

    /// Enables or disables pretty printing.
    ///
    /// This sets the [indentation](Self::set_indent) and writes spaces after
    /// separators (see [`set_compact`](Self::set_compact)) unless the indentation
    /// is [`Indent::None`], in which case the output is compact again.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_json::{Indent, SerializerConfig};
    ///
    /// let value = BTreeMap::from([("a", 1)]);
    /// const PRETTY: SerializerConfig =
    ///     SerializerConfig::builder().pretty(Indent::Spaces(4)).build();
    /// assert_eq!(PRETTY.to_string(&value).unwrap(), "{\n    \"a\": 1\n}");
    /// const NOT_PRETTY: SerializerConfig = PRETTY.into_builder().pretty(Indent::None).build();
    /// assert_eq!(NOT_PRETTY.to_string(&value).unwrap(), r#"{"a":1}"#);
    /// ```
    pub const fn set_pretty(&mut self, indent: Indent) {
        self.indent = indent;
        self.compact = matches!(indent, Indent::None);
    }

    /// Writes NaN and infinite floats as `NaN`, `Infinity` and `-Infinity`.
    ///
    /// JSON cannot represent these values, by default (`false`) they are
    /// written as `null`.  [JSON5](https://json5.org/) (and for instance
    /// the `json` module of Python) supports them with these literals, the
    /// output is then no longer JSON.  The serialization functions of
    /// [`deser-json5`](https://docs.rs/deser-json5) enable this.
    ///
    /// ```
    /// use deser_json::SerializerConfig;
    ///
    /// let values = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    /// assert_eq!(deser_json::to_string(&values).unwrap(), "[null,null,null]");
    /// const NON_FINITE: SerializerConfig =
    ///     SerializerConfig::builder().non_finite_floats(true).build();
    /// assert_eq!(
    ///     NON_FINITE.to_string(&values).unwrap(),
    ///     "[NaN,Infinity,-Infinity]"
    /// );
    /// ```
    pub const fn set_non_finite_floats(&mut self, yes: bool) {
        self.non_finite_floats = yes;
    }

    /// Serializes the given value.
    // inlined so that the pretty writer is not linked for constant compact
    // configurations (see `serialize_driver`)
    #[inline]
    pub fn to_string<T: Serialize + ?Sized>(&self, value: &T) -> Result<String, Error> {
        // the code that exists for every type only erases it.  If the
        // configuration is a constant (like the one of `to_string`) only
        // the writer that is used ends up in the binary.
        let value = SerializeRef::new(&value);
        if self.is_compact() {
            self.to_string_compact(value)
        } else {
            self.to_string_pretty(value)
        }
    }

    /// Serializes a value without indentation (see `to_string`).
    #[inline(never)]
    fn to_string_compact(&self, value: SerializeRef<'_>) -> Result<String, Error> {
        let mut driver = SerializeDriver::from_ref(value);
        self.apply_context(&mut driver);
        accept_raw(&mut driver);
        self.serialize_compact(&mut driver)
    }

    /// Serializes a value with the pretty writer (see `to_string`).
    #[inline(never)]
    fn to_string_pretty(&self, value: SerializeRef<'_>) -> Result<String, Error> {
        let mut driver = SerializeDriver::from_ref(value);
        self.apply_context(&mut driver);
        accept_raw(&mut driver);
        self.serialize_pretty(&mut driver)
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
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
    ///     fn event(
    ///         &mut self,
    ///         event: Event<'_>,
    ///         next: &mut Next<'_>,
    ///     ) -> Result<(), Error> {
    ///         match event {
    ///             Event::Atom(Atom::U64(value)) => {
    ///                 next.emit(value.to_string().into())
    ///             }
    ///             event => next.emit(event),
    ///         }
    ///     }
    /// }
    ///
    /// let json = SerializerConfig::new()
    ///     .to_string_with(&vec![1u64, 2], |driver| {
    ///         driver.push_layer(NumbersAsStrings)
    ///     })
    ///     .unwrap();
    /// assert_eq!(json, r#"["1","2"]"#);
    /// ```
    pub fn to_string_with<F, T: Serialize + ?Sized>(
        &self,
        value: &T,
        setup: F,
    ) -> Result<String, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(&value);
        setup(&mut driver);
        self.apply_context(&mut driver);
        self.serialize_driver(&mut driver)
    }

    /// Serializes the value of a driver.
    ///
    /// This is inlined: if the configuration is a constant (like the one
    /// of `to_string`) only the writer that is used ends up in the binary.
    #[inline]
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
    ) -> Result<String, Error> {
        accept_raw(driver);
        if self.is_compact() {
            self.serialize_compact(driver)
        } else {
            self.serialize_pretty(driver)
        }
    }

    /// Returns `true` if the output is written by `Writer`.
    #[inline(always)]
    fn is_compact(&self) -> bool {
        self.indent == Indent::None && self.compact
    }

    /// Serializes the value of a driver without indentation.
    #[inline(never)]
    fn serialize_compact(&self, driver: &mut SerializeDriver<'_>) -> Result<String, Error> {
        let bytes = BytesFormat::of(driver.state());
        let mut writer = self.compact_writer(Buffer::with_capacity(128), bytes);
        driver.drive_sink(&mut writer)?;
        Ok(writer.ser.out.into_string())
    }

    /// Serializes the value of a driver with the pretty writer.
    #[inline(never)]
    fn serialize_pretty(&self, driver: &mut SerializeDriver<'_>) -> Result<String, Error> {
        let bytes = BytesFormat::of(driver.state());
        let mut writer = self.pretty_writer(Buffer::with_capacity(128), bytes);
        driver.drive(|event, state| writer.event(event, state))?;
        Ok(writer.finish())
    }

    /// Creates the writer for compact output.
    fn compact_writer(&self, out: Buffer, bytes: BytesFormat) -> Writer {
        Writer {
            ser: Output {
                out,
                bytes,
                non_finite_floats: self.non_finite_floats,
            },
            stack: Vec::new(),
            container: Container::Top,
            first: true,
            is_key: false,
            limit: usize::MAX,
        }
    }

    /// Creates the writer for everything but compact output.
    fn pretty_writer(&self, out: Buffer, bytes: BytesFormat) -> PrettyWriter {
        let ser = Output {
            out,
            bytes,
            non_finite_floats: self.non_finite_floats,
        };
        let inline_width = match self.inline {
            InlinePolicy::Never => None,
            InlinePolicy::LeafIfFits(width) => Some(width),
        };
        PrettyWriter::new(ser, self.indent, self.compact, inline_width)
    }

    /// Creates the writer for a value which writes into the buffer.
    pub(crate) fn value_writer(&self, out: Buffer, bytes: BytesFormat) -> ValueWriter {
        if self.is_compact() {
            ValueWriter::Compact(self.compact_writer(out, bytes))
        } else {
            ValueWriter::Pretty(self.pretty_writer(out, bytes))
        }
    }
}

/// Builds a [`SerializerConfig`].
///
/// The methods have the names of the setters of [`SerializerConfig`] (without `set_`).
#[derive(Debug, Clone)]
#[must_use]
pub struct SerializerConfigBuilder {
    value: SerializerConfig,
}

impl SerializerConfigBuilder {
    /// Creates a builder that starts with the default.
    pub const fn new() -> SerializerConfigBuilder {
        SerializerConfigBuilder {
            value: SerializerConfig::new(),
        }
    }

    /// Sets what follows the values of a stream.
    ///
    /// See [`SerializerConfig::set_trailing`].
    pub const fn trailing(mut self, trailing: Trailing) -> SerializerConfigBuilder {
        self.value.set_trailing(trailing);
        self
    }

    /// Sets how the output is indented.
    ///
    /// See [`SerializerConfig::set_indent`].
    pub const fn indent(mut self, indent: Indent) -> SerializerConfigBuilder {
        self.value.set_indent(indent);
        self
    }

    /// Controls the spaces after separators.
    ///
    /// See [`SerializerConfig::set_compact`].
    pub const fn compact(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_compact(yes);
        self
    }

    /// Sets when maps and sequences are written on a single line in
    ///
    /// See [`SerializerConfig::set_inline`].
    pub const fn inline(mut self, policy: InlinePolicy) -> SerializerConfigBuilder {
        self.value.set_inline(policy);
        self
    }

    /// Enables or disables pretty printing.
    ///
    /// See [`SerializerConfig::set_pretty`].
    pub const fn pretty(mut self, indent: Indent) -> SerializerConfigBuilder {
        self.value.set_pretty(indent);
        self
    }

    /// Writes NaN and infinite floats as `NaN`, `Infinity` and `-Infinity`.
    ///
    /// See [`SerializerConfig::set_non_finite_floats`].
    pub const fn non_finite_floats(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_non_finite_floats(yes);
        self
    }

    /// Sets the context the values are serialized in.
    ///
    /// See [`SerializerConfig::set_context`].
    pub fn context(mut self, context: deser_core::Context) -> SerializerConfigBuilder {
        self.value.set_context(context);
        self
    }

    /// Returns the built [`SerializerConfig`].
    pub const fn build(self) -> SerializerConfig {
        // the value cannot be moved out of the builder in a const fn as the
        // builder needs dropping (the context has a destructor)
        // SAFETY: the value is read once and the builder is forgotten
        let value = unsafe { core::ptr::read(&self.value) };
        core::mem::forget(self);
        value
    }
}

impl Default for SerializerConfigBuilder {
    fn default() -> SerializerConfigBuilder {
        SerializerConfigBuilder::new()
    }
}

/// Declares that raw values of JSON are written as they are.
fn accept_raw(driver: &mut SerializeDriver<'_>) {
    driver.state_mut().declare_raw_format(&crate::raw::ID);
}

/// Writes the events of a value.
pub(crate) enum ValueWriter {
    /// Compact output (no indentation, no spaces).
    Compact(Writer),
    /// Everything else.
    Pretty(PrettyWriter),
}

impl ValueWriter {
    /// Writes the events of the driver.
    ///
    /// Returns `false` if the driver was paused as the output holds at
    /// least `limit` bytes (see `take_output`).  With a limit of
    /// `usize::MAX` the value is written at once.
    pub(crate) fn drive(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<bool, Error> {
        if limit == usize::MAX {
            return self.drive_whole(driver).map(|()| true);
        }
        match self {
            ValueWriter::Compact(writer) => {
                writer.limit = limit;
                driver.drive_until(writer)
            }
            ValueWriter::Pretty(writer) => {
                writer.limit = limit;
                driver.drive_until(writer)
            }
        }
    }

    /// Writes the events of the driver at once.
    ///
    /// Unlike `drive` this does not refer to the pausable instances of the
    /// driver which are only needed by stream serializers.
    pub(crate) fn drive_whole(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        match self {
            ValueWriter::Compact(writer) => driver.drive_sink(writer),
            ValueWriter::Pretty(writer) => driver.drive(|event, state| writer.event(event, state)),
        }
    }

    /// Returns the output buffer.
    pub(crate) fn output(&mut self) -> &mut Buffer {
        match self {
            ValueWriter::Compact(writer) => &mut writer.ser.out,
            ValueWriter::Pretty(writer) => writer.output(),
        }
    }

    /// Takes the output written so far (which is final after `drive`
    /// returned), the writer continues with an empty output.
    pub(crate) fn take_output(&mut self) -> Vec<u8> {
        match self {
            ValueWriter::Compact(writer) => writer.ser.out.take(),
            ValueWriter::Pretty(writer) => writer.take_output(),
        }
    }
}

/// Serializes values into JSON.
///
/// Every call to [`serialize`](Self::serialize) writes a value.  What
/// follows the values depends on [`SerializerConfig::set_trailing`]: by default
/// only a single value can be written, with [`Trailing::Newline`] every
/// value is followed by a line break ([JSON Lines](https://jsonlines.org/)).
///
/// ```
/// use deser_json::{Serializer, SerializerConfig, Trailing};
///
/// const LINES: SerializerConfig =
///     SerializerConfig::builder().trailing(Trailing::Newline).build();
/// let mut serializer = Serializer::with_config(LINES);
/// serializer.serialize(&vec![1, 2]).unwrap();
/// serializer.serialize(&"x").unwrap();
/// assert_eq!(serializer.finish(), "[1,2]\n\"x\"\n");
/// ```
///
/// The serializer is also the stream serializer of JSON (see
/// [`StreamSerializer`](ser::StreamSerializer)): the output can be taken
/// while values are written, and large values can be written in parts.
/// To write to a [`Write`](std::io::Write) use
/// [`SerializerConfig::writer`]:
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_json::SerializerConfig;
///
/// let mut writer = SerializerConfig::new().writer(Vec::new());
/// writer.set_buffer_limit(4);
/// writer.write(&vec!["a", "b", "c"]).unwrap();
/// assert_eq!(writer.into_inner(), br#"["a","b","c"]"#);
/// # }
/// ```
pub struct Serializer {
    config: SerializerConfig,
    // only holds the output of value writers and line breaks, which is
    // valid UTF-8
    out: Vec<u8>,
    written: usize,
    // the value that is written in parts
    value: Option<Box<ValueWriter>>,
    // a value was started with `drive_partial` and is not complete
    in_progress: bool,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Clone for Serializer {
    /// Clones the serializer.
    ///
    /// The clone of a serializer that writes a value in parts cannot write
    /// more values (see
    /// [`StreamSerializer::in_progress`](ser::StreamSerializer::in_progress)).
    fn clone(&self) -> Serializer {
        Serializer {
            config: self.config.clone(),
            out: self.out.clone(),
            written: self.written,
            value: None,
            in_progress: self.in_progress,
        }
    }
}

impl core::fmt::Debug for Serializer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Serializer")
            .field("config", &self.config)
            .field("output", &self.as_str())
            .field("written", &self.written)
            .field("in_progress", &self.in_progress)
            .finish()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: SerializerConfig) -> Serializer {
        Serializer::with_written(config, 0)
    }

    /// Creates a serializer for a stream that continues after the given
    /// number of values.
    ///
    /// This is useful to append to a stream that was written before (see
    /// [`SerializerConfig::set_trailing`] for what separates the values).
    pub fn with_written(config: SerializerConfig, written: usize) -> Serializer {
        Serializer {
            config,
            out: Vec::new(),
            written,
            value: None,
            in_progress: false,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SerializerConfig {
        &self.config
    }

    /// Returns the number of values that were written.
    pub fn written(&self) -> usize {
        self.written
    }

    /// Serializes a value.
    ///
    /// If the value fails to serialize, nothing is written.
    pub fn serialize<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn serialize_with<F, T: Serialize + ?Sized>(
        &mut self,
        value: &T,
        setup: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the output written so far (that was not cleared).
    pub fn as_str(&self) -> &str {
        // SAFETY: the output is valid UTF-8, see `out`
        unsafe { core::str::from_utf8_unchecked(&self.out) }
    }

    /// Returns the output.
    pub fn finish(self) -> String {
        // SAFETY: the output is valid UTF-8, see `out`
        unsafe { String::from_utf8_unchecked(self.out) }
    }

    /// Starts a value: writes what separates it from the previous value.
    fn start_value(&mut self) -> Result<(), Error> {
        if self.in_progress {
            return Err(Error::in_progress());
        }
        match self.config.trailing_mode() {
            Trailing::Strict if self.written > 0 => Err(Error::new(
                ErrorKind::InvalidState,
                "with Trailing::Strict only a single value can be written",
            )),
            Trailing::Stop if self.written > 0 => {
                self.out.push(b'\n');
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Completes a value.
    fn finish_value(&mut self) {
        if self.config.trailing_mode() == Trailing::Newline {
            self.out.push(b'\n');
        }
        self.written += 1;
        self.in_progress = false;
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        // only `drive_partial` continues a value
        if self.in_progress {
            return Err(Error::in_progress());
        }
        ser::StreamSerializer::drive_partial(self, driver, usize::MAX).map(|_| ())
    }
}

impl ser::StreamSerializer for Serializer {
    fn output(&self) -> &[u8] {
        &self.out
    }

    fn clear_output(&mut self) {
        self.out.clear();
    }

    fn supports_partial(&self) -> bool {
        true
    }

    fn drive_partial(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<bool, Error> {
        if !self.config.context.is_empty() {
            driver.set_default_context(self.config.context.clone());
        }
        accept_raw(driver);
        let rollback = self.out.len();
        let (mut local, adopt) = match self.value.take() {
            // the writer writes into the output directly if it's empty,
            // otherwise its output is appended
            Some(mut writer) => {
                let adopt = self.out.is_empty();
                if adopt {
                    *writer.output() = Buffer::from_vec(core::mem::take(&mut self.out));
                }
                (writer, adopt)
            }
            None => {
                self.start_value()?;
                // a writer that completes the value at once is not boxed
                let out = Buffer::from_vec(core::mem::take(&mut self.out));
                let writer = self
                    .config
                    .value_writer(out, BytesFormat::of(driver.state()));
                if limit == usize::MAX {
                    return self.drive_whole(writer, driver, rollback);
                }
                (Box::new(writer), true)
            }
        };
        let rv = local.drive(driver, limit);
        match rv {
            Ok(done) => {
                let output = local.take_output();
                if adopt {
                    self.out = output;
                } else {
                    self.out.extend_from_slice(&output);
                }
                if done {
                    self.finish_value();
                    Ok(true)
                } else {
                    self.value = Some(local);
                    self.in_progress = true;
                    Ok(false)
                }
            }
            Err(err) => {
                // the value is abandoned, what was written of it is
                // discarded.  If parts of it were taken the stream stays
                // broken (`in_progress`).
                if adopt {
                    self.out = local.output().take();
                    self.out.truncate(rollback);
                }
                Err(err)
            }
        }
    }

    fn in_progress(&self) -> bool {
        self.in_progress
    }
}

impl Serializer {
    /// Writes a value at once (see `drive_partial`).
    fn drive_whole(
        &mut self,
        mut writer: ValueWriter,
        driver: &mut SerializeDriver<'_>,
        rollback: usize,
    ) -> Result<bool, Error> {
        let rv = writer.drive_whole(driver);
        self.out = writer.output().take();
        match rv {
            Ok(_) => {
                self.finish_value();
                Ok(true)
            }
            Err(err) => {
                self.out.truncate(rollback);
                Err(err)
            }
        }
    }
}

#[cfg(feature = "io")]
impl SerializerConfig {
    /// Creates a writer of a stream of values (see
    /// [`deser::io::Writer`](deser_core::io::Writer)).
    ///
    /// What follows the values depends on [`set_trailing`](Self::set_trailing).
    /// The output of large values is written in parts while they are
    /// serialized.
    ///
    /// ```
    /// use deser_json::{SerializerConfig, Trailing};
    ///
    /// const LINES: SerializerConfig =
    ///     SerializerConfig::builder().trailing(Trailing::Newline).build();
    /// let mut writer = LINES.writer(Vec::new());
    /// writer.write(&1).unwrap();
    /// writer.write(&"x").unwrap();
    /// assert_eq!(writer.into_inner(), b"1\n\"x\"\n");
    /// ```
    pub fn writer<W: std::io::Write>(&self, writer: W) -> deser_core::io::Writer<W, Serializer> {
        deser_core::io::Writer::new(writer, Serializer::with_config(self.clone()))
    }

    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`].
    pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
        &self,
        writer: W,
        value: &T,
    ) -> Result<(), Error> {
        self.writer(writer).write(value)
    }
}

/// Serializes a value to a writer.
///
/// The output of large values is written in parts while they are
/// serialized (see [`deser::io`](deser_core::io)), the writer does not
/// need to be buffered.
///
/// ```
/// let mut out = Vec::new();
/// deser_json::to_writer(&mut out, &vec![1, 2, 3]).unwrap();
/// assert_eq!(out, b"[1,2,3]");
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// The output of the serializer.
pub(crate) struct Output {
    pub(crate) out: Buffer,
    bytes: BytesFormat,
    // NaN and infinities are written as JSON5 literals instead of `null`
    non_finite_floats: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Top,
    Seq,
    Map,
}

/// Holds the state of the serializer while writing.
pub(crate) struct Writer {
    ser: Output,
    // the state of the current container is held here, the state of the
    // outer containers is saved on the stack.
    stack: Vec<Container>,
    container: Container,
    first: bool,
    is_key: bool,
    // the output is passed on once it's this long (see `EventSink`)
    limit: usize,
}

impl EventSink for Writer {
    #[inline(always)]
    fn event(
        &mut self,
        event: Event<'_>,
        _value: SerializeRef<'_>,
        _state: &mut deser_core::State,
    ) -> Result<(), Error> {
        Writer::event(self, event)
    }

    #[inline(always)]
    fn pause(&mut self) -> bool {
        self.ser.out.len() >= self.limit
    }
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

    #[inline]
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

    #[inline]
    fn end(&mut self, is_map: bool) -> Result<(), Error> {
        if is_map {
            if self.container != Container::Map || !self.is_key {
                return Err(Error::new(ErrorKind::InvalidState, "unexpected map end"));
            }
            self.ser.write_char('}');
        } else {
            if self.container != Container::Seq {
                return Err(Error::new(ErrorKind::InvalidState, "unexpected array end"));
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
            Atom::Str(ref val) | Atom::Lexical(ref val) if val.is_borrowed() => {
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
            Atom::Str(ref val) | Atom::Lexical(ref val) => self.write_escaped_str(val),
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
            Atom::Bool(val) => self.write_str(if val { "\"true\"" } else { "\"false\"" }),
            Atom::Ext(ref ext) => self.write_ext_key(ext)?,
            Atom::Bytes(ref val) => self.write_bytes_str(val, val.fallback),
            Atom::Implicit(ref val) => match json_literal(val) {
                Some(text) if val.value() != ImplicitValue::Null => self.write_escaped_str(text),
                _ => return self.write_key_text(val.value().to_atom()),
            },
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
            Atom::Str(ref val) if val.is_borrowed() => self.write_escaped_str(val),
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
            Atom::Str(ref val) | Atom::Lexical(ref val) => self.write_escaped_str(val),
            Atom::Ext(ref ext) => self.write_ext_value(ext)?,
            Atom::Bytes(ref val) => {
                self.write_bytes(val, val.fallback.copied().unwrap_or(self.bytes))
            }
            // values whose type was inferred from text keep their text if
            // it's the same value in JSON, otherwise they are written as
            // their value
            Atom::Implicit(ref val) => match json_literal(val) {
                Some(text) => self.write_str(text),
                None => return self.write_atom(val.value().to_atom()),
            },
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
    fn write_float<F: zmij::Float + Into<f64>>(&mut self, val: F) {
        let wide: f64 = val.into();
        if wide.is_finite() {
            self.write_str(zmij::Buffer::new().format_finite(val))
        } else {
            self.write_non_finite(wide)
        }
    }

    /// Writes NaN or an infinite float.
    #[cold]
    fn write_non_finite(&mut self, val: f64) {
        self.write_str(if !self.non_finite_floats {
            "null"
        } else if val.is_nan() {
            "NaN"
        } else if val > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        })
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
            Atom::Str(val) | Atom::Lexical(val) => self.write_escaped_str(&val),
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
            Atom::Bool(val) => self.write_str(if val { "\"true\"" } else { "\"false\"" }),
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
        // raw values of JSON are written as they are, the serialization
        // only passes them on if they are (see `accept_raw`)
        if let Some(raw) = ext.downcast_value_ref::<RawInput>()
            && raw.is_format(&crate::raw::ID)
            && let Some(text) = raw.as_str()
        {
            self.write_str(text);
            return Ok(());
        }
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
            Atom::Str(val) | Atom::Lexical(val) => self.write_escaped_str(&val),
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

    fn write_int<I: core::fmt::Display>(&mut self, val: I) {
        self.write_str(&val.to_string())
    }

    #[inline]
    fn write_u64(&mut self, val: u64) {
        self.write_str(itoa::Buffer::new().format(val))
    }

    #[inline]
    fn write_i64(&mut self, val: i64) {
        self.write_str(itoa::Buffer::new().format(val))
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
#[inline]
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// Returns the text of an implicit value if it's a JSON literal for the
/// same value.
///
/// This keeps the text of numbers like `1.10` which would otherwise be
/// written as `1.1`.  Text that is not JSON (like `0x1F` or `~`) is not.
fn json_literal<'a>(value: &'a Implicit) -> Option<&'a str> {
    let text = value.text().as_str();
    let same = match value.value() {
        ImplicitValue::Null => text == "null",
        ImplicitValue::Bool(value) => text == if value { "true" } else { "false" },
        ImplicitValue::U64(value) => is_json_int(text) && text.parse::<u64>() == Ok(value),
        ImplicitValue::I64(value) => is_json_int(text) && text.parse::<i64>() == Ok(value),
        ImplicitValue::F64(value) => {
            value.is_finite()
                && Number::parse(text)
                    .is_ok_and(|x| !x.is_integer() && x.value().to_bits() == value.to_bits())
        }
        _ => false,
    };
    same.then_some(text)
}

/// Checks the syntax of JSON integers (an optional minus and digits
/// without leading zeros).
fn is_json_int(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text).as_bytes();
    match digits {
        [b'0'] => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}
