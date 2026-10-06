use std::borrow::Cow;

use deser_core::Text;
use deser_core::de::{
    self, Deserialize, DeserializeDriver, DuplicateKeys, LexicalRules, deserialize_value,
};
use deser_core::{Atom, ContainerShape, Error, ErrorKind, Event, Source, TrackLocations};

use crate::parser::{self, Document, NodeKind, Range};
use crate::{Continuation, InlineComments, Quotes, Syntax};

/// Configures how INI files are deserialized.
///
/// The default ([`new`](Self::new)) reads the INI files that are common
/// today: `;` and `#` comments, `=` and `:` delimiters, comments after
/// values (` ; comment`), values continued on indented lines, quoted values
/// and keys without values.  The presets [`python`](Self::python) and
/// [`git`](Self::git) read the dialects of Python's `configparser` and of
/// git.  The configuration is independent of the input so it can be created
/// once (even as a constant) and used for many inputs.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::{DeserializerConfig, Quotes};
///
/// const CONFIG: DeserializerConfig =
///     DeserializerConfig::builder().quotes(Quotes::None).build();
/// let value: BTreeMap<String, BTreeMap<String, String>> =
///     CONFIG.from_str("[a]\nb = \"c\"").unwrap();
/// assert_eq!(value["a"]["b"], "\"c\"");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    pub(crate) syntax: Syntax,
    pub(crate) inline_comments: InlineComments,
    pub(crate) colon_delimiter: bool,
    pub(crate) continuation: Continuation,
    pub(crate) quotes: Quotes,
    pub(crate) allow_no_value: bool,
    pub(crate) lowercase_names: bool,

    context: deser_core::Context,
}

impl Default for DeserializerConfig {
    fn default() -> DeserializerConfig {
        DeserializerConfig::new()
    }
}

impl DeserializerConfig {
    /// Creates the default configuration (common INI files).
    ///
    /// * [`Syntax::Ini`]
    /// * [`InlineComments::AfterWhitespace`]
    /// * `=` and `:` are delimiters
    /// * [`Continuation::Indented`]
    /// * [`Quotes::Value`]
    /// * keys without values are allowed
    /// * names keep their case
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig {
            syntax: Syntax::Ini,
            inline_comments: InlineComments::AfterWhitespace,
            colon_delimiter: true,
            continuation: Continuation::Indented,
            quotes: Quotes::Value,
            allow_no_value: true,
            lowercase_names: false,

            context: deser_core::Context::new(),
        }
    }

    /// Creates the configuration for the files of Python's `configparser`.
    ///
    /// These are `setup.cfg`, `tox.ini`, `pytest.ini` and the like.  This is
    /// the default but without inline comments and quotes, like
    /// `RawConfigParser(strict=False, allow_no_value=True,
    /// allow_unnamed_section=True, interpolation=None)` with keys that keep
    /// their case.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_ini::DeserializerConfig;
    ///
    /// let value: BTreeMap<String, BTreeMap<String, String>> =
    ///     DeserializerConfig::python()
    ///         .from_str("[tox]\nenvlist = py312 ; py313\n")
    ///         .unwrap();
    /// assert_eq!(value["tox"]["envlist"], "py312 ; py313");
    /// ```
    pub const fn python() -> DeserializerConfig {
        DeserializerConfig::builder()
            .inline_comments(InlineComments::None)
            .quotes(Quotes::None)
            .build()
    }

    /// Creates the configuration for git's config files.
    ///
    /// This is [`Syntax::Git`] which reads `.gitconfig`, `.git/config` and
    /// `.gitmodules` like git does.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_ini::DeserializerConfig;
    ///
    /// type Config = BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>;
    /// let config: Config = DeserializerConfig::git()
    ///     .from_str("[remote \"origin\"]\n\turl = https://example.com/x.git\n")
    ///     .unwrap();
    /// assert_eq!(config["remote"]["origin"]["url"], "https://example.com/x.git");
    /// ```
    pub const fn git() -> DeserializerConfig {
        DeserializerConfig::builder().syntax(Syntax::Git).build()
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

    /// Sets the syntax.
    ///
    /// The default is [`Syntax::Ini`].  With [`Syntax::Git`] the other
    /// options (except for the context) are ignored.
    pub const fn set_syntax(&mut self, syntax: Syntax) {
        self.syntax = syntax;
    }

    /// Returns the syntax.
    pub const fn syntax(&self) -> Syntax {
        self.syntax
    }

    /// Sets where comments start after values.
    ///
    /// The default is [`InlineComments::AfterWhitespace`].  Lines that
    /// start with `;` or `#` are always comments.
    pub const fn set_inline_comments(&mut self, comments: InlineComments) {
        self.inline_comments = comments;
    }

    /// Returns where comments start after values.
    pub const fn inline_comments(&self) -> InlineComments {
        self.inline_comments
    }

    /// Sets if `:` separates keys and values (like `=`).
    ///
    /// The default is `true`, the first `=` or `:` of a line separates the
    /// key and the value (`url = http://x` is the key `url`).
    pub const fn set_colon_delimiter(&mut self, yes: bool) {
        self.colon_delimiter = yes;
    }

    /// Returns if `:` separates keys and values.
    pub const fn colon_delimiter(&self) -> bool {
        self.colon_delimiter
    }

    /// Sets how values continue on the next lines.
    ///
    /// The default is [`Continuation::Indented`].
    pub const fn set_continuation(&mut self, continuation: Continuation) {
        self.continuation = continuation;
    }

    /// Returns how values continue on the next lines.
    pub const fn continuation(&self) -> Continuation {
        self.continuation
    }

    /// Sets how quoted values are read.
    ///
    /// The default is [`Quotes::Value`].
    pub const fn set_quotes(&mut self, quotes: Quotes) {
        self.quotes = quotes;
    }

    /// Returns how quoted values are read.
    pub const fn quotes(&self) -> Quotes {
        self.quotes
    }

    /// Sets if keys can be given without value.
    ///
    /// The default is `true`: a line with a key but no delimiter (like
    /// `skip-name-resolve` in MySQL's configuration) is the key with a null
    /// value.  Optionals are `None` and the
    /// [`Flag`](deser_core::adapters::Flag) adapter is `true` for it.  If
    /// `false`, such lines are an error.
    pub const fn set_allow_no_value(&mut self, yes: bool) {
        self.allow_no_value = yes;
    }

    /// Returns if keys can be given without value.
    pub const fn allow_no_value(&self) -> bool {
        self.allow_no_value
    }

    /// Sets if the names of sections and keys are lowercased.
    ///
    /// The default is `false`.  Only ASCII letters are lowercased.  This
    /// makes names case insensitive, like they are for Windows and Python's
    /// `configparser` (which lowercases keys).
    pub const fn set_lowercase_names(&mut self, yes: bool) {
        self.lowercase_names = yes;
    }

    /// Returns if the names of sections and keys are lowercased.
    pub const fn lowercase_names(&self) -> bool {
        self.lowercase_names
    }

    /// Deserializes a value from an INI file.
    ///
    /// See [`from_str`](crate::from_str).
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

    /// Deserializes a value from an INI file in a byte slice.
    ///
    /// See [`from_slice`](crate::from_slice).
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

    /// Sets the syntax.
    ///
    /// See [`DeserializerConfig::set_syntax`].
    pub const fn syntax(mut self, syntax: Syntax) -> DeserializerConfigBuilder {
        self.value.set_syntax(syntax);
        self
    }

    /// Sets where comments start after values.
    ///
    /// See [`DeserializerConfig::set_inline_comments`].
    pub const fn inline_comments(mut self, comments: InlineComments) -> DeserializerConfigBuilder {
        self.value.set_inline_comments(comments);
        self
    }

    /// Sets if `:` separates keys and values (like `=`).
    ///
    /// See [`DeserializerConfig::set_colon_delimiter`].
    pub const fn colon_delimiter(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_colon_delimiter(yes);
        self
    }

    /// Sets how values continue on the next lines.
    ///
    /// See [`DeserializerConfig::set_continuation`].
    pub const fn continuation(mut self, continuation: Continuation) -> DeserializerConfigBuilder {
        self.value.set_continuation(continuation);
        self
    }

    /// Sets how quoted values are read.
    ///
    /// See [`DeserializerConfig::set_quotes`].
    pub const fn quotes(mut self, quotes: Quotes) -> DeserializerConfigBuilder {
        self.value.set_quotes(quotes);
        self
    }

    /// Sets if keys can be given without value.
    ///
    /// See [`DeserializerConfig::set_allow_no_value`].
    pub const fn allow_no_value(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_allow_no_value(yes);
        self
    }

    /// Sets if the names of sections and keys are lowercased.
    ///
    /// See [`DeserializerConfig::set_lowercase_names`].
    pub const fn lowercase_names(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_lowercase_names(yes);
        self
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

/// Deserializes INI files.
///
/// Most of the time the [`from_str`](crate::from_str) and
/// [`from_slice`](crate::from_slice) functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed.  The deserializer
/// is useful to configure the driver, for instance to add layers, or to
/// update a value (see [`update`](deser_core::de::Deserializer::update)):
///
/// ```
/// use deser_path::{Path, PathLayer};
/// use deser_ini::Deserializer;
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Config {
///     server: Server,
/// }
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Server {
///     port: u16,
/// }
///
/// let err = Deserializer::from_str("[server]\nport = http\n")
///     .deserialize_with::<Config, _>(|driver| {
///         driver.push_layer(PathLayer::new())
///     })
///     .unwrap_err();
/// assert_eq!(err.message(), "invalid value \"http\", expected u16");
/// assert_eq!(err.attachment::<Path>().unwrap().to_string(), "server.port");
/// assert_eq!(err.line(), Some(2));
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
    /// The input must be UTF-8 (a byte order mark is skipped), otherwise
    /// deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    pub fn from_slice_with_config(input: &'a [u8], config: DeserializerConfig) -> Deserializer<'a> {
        match std::str::from_utf8(input) {
            Ok(input) => Deserializer::from_str_with_config(input, config),
            Err(err) => Deserializer {
                input: "",
                error: Some(Error::with_offset(
                    ErrorKind::Syntax,
                    "input is not valid UTF-8",
                    err.valid_up_to(),
                )),
                config,
            },
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Deserializes the input.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the input with a configured driver.
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
    /// The whole input is parsed before the first event is emitted, so
    /// malformed input is reported before any value is deserialized.  Keys
    /// and values that are not changed (by quotes, escapes, continuation
    /// lines or lowercasing) are passed on borrowed from the input (see
    /// [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
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
        let doc = parser::parse(self.input, &self.config).map_err(|mut err| {
            err.resolve_position(self.input.as_bytes());
            err
        })?;
        let state = driver.state_mut();
        if TrackLocations::of(state) {
            Source(self.input.into()).set(state);
        }
        // the last value of repeated keys is used and everything is text
        // unless the context says otherwise
        DuplicateKeys::Last.set_default(state);
        LexicalRules::LENIENT.set_default(state);
        emit(&doc, self.input.len(), driver).map_err(|mut err| {
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

/// Returns the shape of a table.
///
/// Tables are multimaps, the keys are given once per value.
fn table_shape(doc: &Document<'_>, children: &[usize]) -> ContainerShape {
    let len = children
        .iter()
        .map(|&child| doc.nodes[child].values.len().max(1))
        .sum();
    let mut shape = ContainerShape::with_len(len);
    shape.set_multimap(true);
    shape
}

/// Emits the events of the document.
fn emit<'a>(
    doc: &Document<'a>,
    input_len: usize,
    driver: &mut DeserializeDriver<'_, 'a>,
) -> Result<(), Error> {
    struct Frame {
        node: usize,
        pos: usize,
    }

    let root = &doc.nodes[0];
    emit_at(
        driver,
        Event::MapStart(table_shape(doc, &root.children)),
        root.range,
    )?;
    let mut stack = vec![Frame { node: 0, pos: 0 }];
    while let Some(frame) = stack.last_mut() {
        let table = &doc.nodes[frame.node];
        let Some(&child) = table.children.get(frame.pos) else {
            let range = if frame.node == 0 {
                (input_len, input_len)
            } else {
                table.range
            };
            stack.pop();
            emit_at(driver, Event::MapEnd, range)?;
            continue;
        };
        frame.pos += 1;
        let node = &doc.nodes[child];
        match node.kind {
            NodeKind::Key => {
                // the key is emitted for every value (tables are multimaps)
                for (value, range) in &node.values {
                    emit_text(driver, &node.name, node.range)?;
                    match *value {
                        Some(ref value) => emit_text(driver, value, *range)?,
                        None => emit_at(driver, Atom::Null, *range)?,
                    }
                }
            }
            NodeKind::Table => {
                emit_text(driver, &node.name, node.range)?;
                emit_at(
                    driver,
                    Event::MapStart(table_shape(doc, &node.children)),
                    node.range,
                )?;
                stack.push(Frame {
                    node: child,
                    pos: 0,
                });
            }
        }
    }
    Ok(())
}

/// Emits an event with a byte range.
#[inline]
fn emit_at<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver<'_, '_>,
    event: E,
    range: Range,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(range.0, range.1);
    driver.emit(event)
}

/// Emits text as lexical atom, borrowed if it's a slice of the input.
// the text is a `Cow` as borrowed text is passed on for `'a`
#[allow(clippy::ptr_arg)]
#[inline]
fn emit_text<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    text: &Cow<'a, str>,
    range: Range,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(range.0, range.1);
    match *text {
        Cow::Borrowed(text) => driver.emit_borrowed(Atom::Lexical(Text::borrowed(text))),
        Cow::Owned(ref text) => driver.emit(Atom::Lexical(Text::borrowed(text.as_str()))),
    }
}
