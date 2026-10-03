use deser_core::ser::{self, SerializeDriver, SerializeRef};
use deser_core::{BytesFormat, Error, Serialize};

use crate::emit::Emitter;
use crate::resolve::Version;

/// How the output is indented.
///
/// See [`SerializerConfig::set_indent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Indent {
    /// No indentation: the document is written on a single line in flow
    /// style (`{name: web, ports: [80, 443]}`).
    None,
    /// Block style indented by the given number of spaces per level.
    ///
    /// YAML does not allow tabs for indentation.  Values outside of
    /// `1..=9` are clamped as indentation indicators of block scalars are
    /// single digits.
    Spaces(usize),
}

impl Default for Indent {
    fn default() -> Indent {
        Indent::Spaces(2)
    }
}

/// How strings are quoted when they cannot be written plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum QuoteStyle {
    /// Single quotes (`'yes'`), double quotes if the string needs escapes.
    #[default]
    Single,
    /// Double quotes (`"yes"`).
    Double,
}

/// How strings with line breaks are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MultilineStyle {
    /// As literal block scalars (`|`) if they only contain characters that
    /// can be written in them, otherwise double-quoted.
    #[default]
    Literal,
    /// Double-quoted with escapes (`"a\nb"`).
    Quoted,
}

/// When collections are written in flow style (`[a, b]`, `{a: 1}`).
///
/// Collections with the [`Layout::Compact`](deser_core::hints::Layout) hint are
/// always written in flow style, collections with
/// [`Layout::Expanded`](deser_core::hints::Layout) never (unless they are in a
/// flow collection, which can only contain flow collections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FlowPolicy {
    /// Only compact collections are written in flow style.
    #[default]
    Never,
    /// Collections which only contain scalars are written in flow style if
    /// they end before the given column.
    LeafIfFits(usize),
}

/// How null is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NullStyle {
    /// `null`
    #[default]
    Null,
    /// `~`
    Tilde,
    /// Nothing (`key:`, `-`).  Null keys and null documents are written as
    /// `null`.
    Empty,
}

/// Configures how values are serialized to YAML.
///
/// YAML allows the same data to be written in many ways.  The defaults
/// follow what is common for hand-written YAML:
///
/// * block collections indented by two spaces (see [`set_indent`](Self::set_indent)),
///   sequences in mappings are indented
///   ([`set_indent_sequences`](Self::set_indent_sequences)), empty collections are
///   written as `{}` and `[]`.  Compact collections (see
///   [`hints`](deser_core::hints)) are written in flow style, see
///   [`set_flow`](Self::set_flow).
/// * strings are plain if possible, otherwise single-quoted (double-quoted if
///   they need escapes).  Strings are quoted if readers of YAML 1.1 would
///   read them as something else (`yes`, `0777`, timestamps, see
///   [`set_compat`](Self::set_compat)).
/// * strings with line breaks are literal block scalars (`|`), the style of
///   individual strings can be requested (see [`style`](crate::style)).
/// * bytes are written as `!!binary` (see [`set_binary`](Self::set_binary)).
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_yaml::SerializerConfig;
///
/// let mut value = BTreeMap::new();
/// value.insert("items", vec!["a", "yes"]);
/// assert_eq!(
///     deser_yaml::to_string(&value).unwrap(),
///     "items:\n  - a\n  - 'yes'\n"
/// );
///
/// const INDENTLESS: SerializerConfig =
///     SerializerConfig::builder().indent_sequences(false).build();
/// assert_eq!(
///     INDENTLESS.to_string(&value).unwrap(),
///     "items:\n- a\n- 'yes'\n"
/// );
/// ```
///
/// [`to_string`](Self::to_string) works like the
/// [`to_string`] function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    pub(crate) indent: Indent,
    pub(crate) indent_sequences: bool,
    pub(crate) flow: FlowPolicy,
    pub(crate) fold_width: Option<usize>,
    pub(crate) quote_style: QuoteStyle,
    pub(crate) quote_all: bool,
    pub(crate) multiline: MultilineStyle,
    pub(crate) null_style: NullStyle,
    pub(crate) compat: Version,
    pub(crate) binary: bool,
    pub(crate) timestamp_tag: bool,
    pub(crate) document_start: bool,
    pub(crate) version_directive: bool,
    pub(crate) end_documents: bool,
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
            indent: Indent::Spaces(2),
            indent_sequences: true,
            flow: FlowPolicy::Never,
            fold_width: None,
            quote_style: QuoteStyle::Single,
            quote_all: false,
            multiline: MultilineStyle::Literal,
            null_style: NullStyle::Null,
            compat: Version::V1_1,
            binary: true,
            timestamp_tag: false,
            document_start: false,
            version_directive: false,
            end_documents: false,
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
    /// writers created with the configuration start with this context
    /// (their `set_context` replaces it).  A context set on the driver
    /// takes precedence.
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

    /// Sets how the output is indented.
    ///
    /// The default is [`Indent::Spaces(2)`](Indent::Spaces), values outside
    /// of `1..=9` are clamped.  With [`Indent::None`] documents are written
    /// on a single line in flow style, the [flow policy](Self::set_flow) and
    /// [`Layout`](deser_core::hints::Layout) hints have no effect then:
    ///
    /// ```
    /// use deser::Serialize;
    /// use deser_yaml::{Indent, SerializerConfig};
    ///
    /// #[derive(Serialize)]
    /// struct Config {
    ///     name: String,
    ///     ports: Vec<u16>,
    /// }
    ///
    /// let config = Config { name: "web".into(), ports: vec![80, 443] };
    /// const WIDE: SerializerConfig =
    ///     SerializerConfig::builder().indent(Indent::Spaces(4)).build();
    /// assert_eq!(
    ///     WIDE.to_string(&config).unwrap(),
    ///     "name: web\nports:\n    - 80\n    - 443\n"
    /// );
    /// const LINE: SerializerConfig =
    ///     SerializerConfig::builder().indent(Indent::None).build();
    /// assert_eq!(
    ///     LINE.to_string(&config).unwrap(),
    ///     "{name: web, ports: [80, 443]}\n"
    /// );
    /// ```
    pub const fn set_indent(&mut self, indent: Indent) {
        self.indent = match indent {
            Indent::None => Indent::None,
            Indent::Spaces(0) => Indent::Spaces(1),
            Indent::Spaces(n) if n > 9 => Indent::Spaces(9),
            Indent::Spaces(n) => Indent::Spaces(n),
        };
    }

    /// Returns the number of spaces per indentation level of block
    /// collections.
    pub(crate) fn indent_width(&self) -> usize {
        match self.indent {
            Indent::Spaces(n) => n,
            // nothing is written in block style
            Indent::None => 0,
        }
    }

    /// Indents sequences that are values of mappings.
    ///
    /// By default (`true`) the dashes of such sequences are indented like
    /// the keys of nested mappings (`key:\n  - a`).  With `false` they are at
    /// the column of the key (`key:\n- a`), which is how libyaml, PyYAML and
    /// `kubectl` write YAML.
    pub const fn set_indent_sequences(&mut self, yes: bool) {
        self.indent_sequences = yes;
    }

    /// Sets when collections are written in flow style.
    ///
    /// This has no effect with [`Indent::None`] where everything is written
    /// in flow style.
    ///
    /// ```
    /// use deser::Serialize;
    /// use deser_yaml::{FlowPolicy, SerializerConfig};
    ///
    /// #[derive(Serialize)]
    /// struct Config {
    ///     ports: Vec<u16>,
    ///     groups: Vec<Vec<u16>>,
    /// }
    ///
    /// let config = Config {
    ///     ports: vec![80, 443],
    ///     groups: vec![vec![1], vec![2, 3]],
    /// };
    /// const FLOW: SerializerConfig =
    ///     SerializerConfig::builder().flow(FlowPolicy::LeafIfFits(80)).build();
    /// assert_eq!(
    ///     FLOW.to_string(&config).unwrap(),
    ///     "ports: [80, 443]\ngroups:\n  - [1]\n  - [2, 3]\n"
    /// );
    /// ```
    pub const fn set_flow(&mut self, policy: FlowPolicy) {
        self.flow = policy;
    }

    /// Folds long strings at the given width.
    ///
    /// Strings without line breaks that are longer than the width are
    /// written as folded block scalars (`>`) with lines that do not exceed
    /// the width if possible.  Strings are only folded if they read back
    /// unchanged.  By default strings are not folded.  The width is also used
    /// for strings with the [`Folded`](crate::style::Folded) hint (80 if not
    /// set).
    pub const fn set_fold_width(&mut self, width: Option<usize>) {
        self.fold_width = width;
    }

    /// Sets how strings are quoted that cannot be written plain.
    pub const fn set_quote_style(&mut self, style: QuoteStyle) {
        self.quote_style = style;
    }

    /// Quotes all strings, including strings with line breaks.
    pub const fn set_quote_all(&mut self, yes: bool) {
        self.quote_all = yes;
    }

    /// Sets how strings with line breaks are written.
    pub const fn set_multiline(&mut self, style: MultilineStyle) {
        self.multiline = style;
    }

    /// Sets how null is written.
    pub const fn set_null_style(&mut self, style: NullStyle) {
        self.null_style = style;
    }

    /// Sets the oldest YAML version that readers of the output may use.
    ///
    /// Plain strings are quoted if a reader of this (or a later) version
    /// would read them as something else than a string.  The default is
    /// [`Version::V1_1`]: strings like `yes`, `on`, `0777`, `1:30` or
    /// `2001-12-14` are quoted.  With [`Version::V1_2`] they are plain.
    ///
    /// ```
    /// use deser_yaml::{SerializerConfig, Version};
    ///
    /// assert_eq!(deser_yaml::to_string(&"yes").unwrap(), "'yes'\n");
    /// const V1_2: SerializerConfig =
    ///     SerializerConfig::builder().compat(Version::V1_2).build();
    /// assert_eq!(V1_2.to_string(&"yes").unwrap(), "yes\n");
    /// ```
    pub const fn set_compat(&mut self, version: Version) {
        self.compat = version;
    }

    /// Writes bytes as `!!binary`.
    ///
    /// By default (`true`) YAML is a format with native bytes: bytes are
    /// written as base64 with the `!!binary` tag, also bytes that request a
    /// representation for formats without native bytes (see
    /// [`BytesFallback`](deser_core::adapters::BytesFallback)).  With
    /// `false` bytes are represented like in JSON: in the format they request
    /// or the [`BytesFormat`] of the [`Context`](deser_core::Context).
    ///
    /// ```
    /// use deser::adapters::Base64UrlNoPad;
    /// use deser::{BytesFormat, Context};
    /// use deser_yaml::SerializerConfig;
    ///
    /// assert_eq!(
    ///     deser_yaml::to_string(&b"\xfb\xff").unwrap(),
    ///     "!!binary +/8=\n"
    /// );
    /// let config = SerializerConfig::builder()
    ///     .binary(false)
    ///     .context(Context::with(BytesFormat::encoded::<Base64UrlNoPad>()))
    ///     .build();
    /// assert_eq!(config.to_string(&b"\xfb\xff").unwrap(), "-_8\n");
    /// ```
    ///
    /// More encodings (such as hex) are provided by
    /// [`deser-encoding`](https://docs.rs/deser-encoding).  Bytes in other
    /// formats than base64 (or sequences) need to be deserialized with the
    /// same format in the context.
    pub const fn set_binary(&mut self, yes: bool) {
        self.binary = yes;
    }

    /// Writes date-times with the `!!timestamp` tag.
    ///
    /// Dates and date-times with offset ([`Datetime`](deser_core::ext::Datetime))
    /// are written as YAML timestamps.  By default they are plain which YAML
    /// 1.1 readers resolve as timestamps and YAML 1.2 readers as strings
    /// (which date / time types accept).  With the tag all readers resolve
    /// them as timestamps.  Local date-times and times are always strings.
    pub const fn set_timestamp_tag(&mut self, yes: bool) {
        self.timestamp_tag = yes;
    }

    /// Always starts documents with `---`.
    ///
    /// When writing a stream of documents (see [`deser::io`](deser_core::io)), documents
    /// after the first one always start with `---`.
    pub const fn set_document_start(&mut self, yes: bool) {
        self.document_start = yes;
    }

    /// Starts documents with a `%YAML 1.2` directive.
    pub const fn set_version_directive(&mut self, yes: bool) {
        self.version_directive = yes;
    }

    /// Ends documents with a document end marker (`...`).
    ///
    /// When a stream of documents is read (see `deser::io`), a document
    /// is complete once the next document starts or once it's ended with
    /// `...`.  For streams that stay open (like sockets) this allows the
    /// reader to see the end of a document without waiting for the next
    /// one.
    ///
    /// ```
    /// use deser_yaml::{Serializer, SerializerConfig};
    ///
    /// const ENDED: SerializerConfig =
    ///     SerializerConfig::builder().end_documents(true).build();
    /// let mut serializer = Serializer::with_config(&ENDED);
    /// serializer.serialize(&"a").unwrap();
    /// serializer.serialize(&"b").unwrap();
    /// assert_eq!(serializer.finish(), "a\n...\n---\nb\n...\n");
    /// ```
    pub const fn set_end_documents(&mut self, yes: bool) {
        self.end_documents = yes;
    }

    /// Creates the emitter of a document which writes into the output.
    ///
    /// This writes what precedes the document, `index` is the number of
    /// documents written before.
    pub(crate) fn emitter(&self, index: usize, mut out: String, bytes: BytesFormat) -> Emitter {
        if self.version_directive {
            // directives can only follow the end of a document
            if index > 0 && !self.end_documents {
                out.push_str("...\n");
            }
            out.push_str("%YAML 1.2\n---\n");
        } else if self.document_start || index > 0 {
            out.push_str("---\n");
        }
        Emitter::new(self, out, bytes)
    }

    /// Writes the end of a document once its value was written.
    pub(crate) fn end_document(&self, emitter: &mut Emitter) -> Result<(), Error> {
        emitter.finish()?;
        if self.end_documents {
            emitter.out.push_str("...\n");
        }
        Ok(())
    }

    /// Serializes the value of a driver as a document of a stream at once
    /// and appends it to the output.
    ///
    /// Unlike `document_part` this does not refer to the pausable instance
    /// of the driver which is only needed by stream serializers.  `index` is
    /// the number of documents written before.  If this fails, what was
    /// appended by the call is removed from the output.
    pub(crate) fn document_whole(
        &self,
        index: usize,
        driver: &mut SerializeDriver<'_>,
        out: &mut String,
    ) -> Result<(), Error> {
        let len = out.len();
        let mut emitter = self.emitter(index, std::mem::take(out), BytesFormat::of(driver.state()));
        let rv = driver
            .drive(|event, state| emitter.event(event, state))
            .and_then(|()| self.end_document(&mut emitter));
        *out = emitter.out;
        if rv.is_err() {
            out.truncate(len);
        }
        rv
    }

    /// Serializes (a part of) the value of a driver as a document of a
    /// stream and appends it to the output.
    ///
    /// The progress of the document is kept in `document` (see
    /// `StreamSerializer::drive_partial`), `true` is returned once the
    /// document is complete.  `index` is the number of documents written
    /// before.  If this fails, what was appended by the call is removed
    /// from the output.
    pub(crate) fn document_part(
        &self,
        index: usize,
        document: &mut Option<Box<Emitter>>,
        driver: &mut SerializeDriver<'_>,
        out: &mut String,
        limit: usize,
    ) -> Result<bool, Error> {
        // a document that is written at once is written into the output
        // directly without boxing the emitter
        if document.is_none() && limit == usize::MAX {
            return self.document_whole(index, driver, out).map(|()| true);
        }
        let len = out.len();
        // the emitter writes into an empty output directly, otherwise its
        // output is appended
        let adopt = out.is_empty();
        let mut emitter = match document.take() {
            Some(mut emitter) => {
                if adopt {
                    emitter.out = std::mem::take(out);
                }
                emitter
            }
            None => {
                let buffer = match adopt {
                    true => std::mem::take(out),
                    false => String::new(),
                };
                Box::new(self.emitter(index, buffer, BytesFormat::of(driver.state())))
            }
        };
        // after an error the document is abandoned, its emitter is dropped
        emitter.limit = limit;
        let rv = driver.drive_until(&mut *emitter).and_then(|done| {
            if done {
                self.end_document(&mut emitter)?;
            }
            Ok(done)
        });
        let done = match rv {
            Ok(done) => done,
            Err(err) => {
                if adopt {
                    *out = std::mem::take(&mut emitter.out);
                }
                out.truncate(len);
                return Err(err);
            }
        };
        let output = emitter.take_output();
        if adopt {
            *out = output;
        } else {
            out.push_str(&output);
        }
        if !done {
            *document = Some(emitter);
        }
        Ok(done)
    }

    /// Serializes the given value.
    pub fn to_string<T: Serialize + ?Sized>(&self, value: &T) -> Result<String, Error> {
        self.to_string_ref(SerializeRef::new(&value))
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
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
        let mut out = String::new();
        self.document_whole(0, &mut driver, &mut out)?;
        Ok(out)
    }

    /// Serializes a value whose type is erased (see
    /// [`to_string`](Self::to_string)).
    ///
    /// This is not generic: the code that exists for every type only
    /// erases it.
    fn to_string_ref(&self, value: SerializeRef<'_>) -> Result<String, Error> {
        let mut driver = SerializeDriver::from_ref(value);
        self.apply_context(&mut driver);
        let mut out = String::new();
        self.document_whole(0, &mut driver, &mut out)?;
        Ok(out)
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

    /// Sets how the output is indented.
    ///
    /// See [`SerializerConfig::set_indent`].
    pub const fn indent(mut self, indent: Indent) -> SerializerConfigBuilder {
        self.value.set_indent(indent);
        self
    }

    /// Indents sequences that are values of mappings.
    ///
    /// See [`SerializerConfig::set_indent_sequences`].
    pub const fn indent_sequences(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_indent_sequences(yes);
        self
    }

    /// Sets when collections are written in flow style.
    ///
    /// See [`SerializerConfig::set_flow`].
    pub const fn flow(mut self, policy: FlowPolicy) -> SerializerConfigBuilder {
        self.value.set_flow(policy);
        self
    }

    /// Folds long strings at the given width.
    ///
    /// See [`SerializerConfig::set_fold_width`].
    pub const fn fold_width(mut self, width: Option<usize>) -> SerializerConfigBuilder {
        self.value.set_fold_width(width);
        self
    }

    /// Sets how strings are quoted that cannot be written plain.
    ///
    /// See [`SerializerConfig::set_quote_style`].
    pub const fn quote_style(mut self, style: QuoteStyle) -> SerializerConfigBuilder {
        self.value.set_quote_style(style);
        self
    }

    /// Quotes all strings, including strings with line breaks.
    ///
    /// See [`SerializerConfig::set_quote_all`].
    pub const fn quote_all(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_quote_all(yes);
        self
    }

    /// Sets how strings with line breaks are written.
    ///
    /// See [`SerializerConfig::set_multiline`].
    pub const fn multiline(mut self, style: MultilineStyle) -> SerializerConfigBuilder {
        self.value.set_multiline(style);
        self
    }

    /// Sets how null is written.
    ///
    /// See [`SerializerConfig::set_null_style`].
    pub const fn null_style(mut self, style: NullStyle) -> SerializerConfigBuilder {
        self.value.set_null_style(style);
        self
    }

    /// Sets the oldest YAML version that readers of the output may use.
    ///
    /// See [`SerializerConfig::set_compat`].
    pub const fn compat(mut self, version: Version) -> SerializerConfigBuilder {
        self.value.set_compat(version);
        self
    }

    /// Writes bytes as `!!binary`.
    ///
    /// See [`SerializerConfig::set_binary`].
    pub const fn binary(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_binary(yes);
        self
    }

    /// Writes date-times with the `!!timestamp` tag.
    ///
    /// See [`SerializerConfig::set_timestamp_tag`].
    pub const fn timestamp_tag(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_timestamp_tag(yes);
        self
    }

    /// Always starts documents with `---`.
    ///
    /// See [`SerializerConfig::set_document_start`].
    pub const fn document_start(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_document_start(yes);
        self
    }

    /// Starts documents with a `%YAML 1.2` directive.
    ///
    /// See [`SerializerConfig::set_version_directive`].
    pub const fn version_directive(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_version_directive(yes);
        self
    }

    /// Ends documents with a document end marker (`...`).
    ///
    /// See [`SerializerConfig::set_end_documents`].
    pub const fn end_documents(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_end_documents(yes);
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

/// Serializes values into YAML documents.
///
/// Every call to [`serialize`](Self::serialize) writes a document,
/// documents after the first start with `---`.
///
/// ```
/// use deser_yaml::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&"a").unwrap();
/// serializer.serialize(&vec![1, 2]).unwrap();
/// assert_eq!(serializer.finish(), "a\n---\n- 1\n- 2\n");
/// ```
///
/// The serializer is also the stream serializer of YAML (see
/// [`StreamSerializer`](ser::StreamSerializer)): the output can be taken
/// while documents are written, and large documents can be written in
/// parts.  To write to a [`Write`](std::io::Write) use
/// [`SerializerConfig::writer`].
pub struct Serializer {
    config: SerializerConfig,
    out: String,
    written: usize,
    // the document that is written in parts
    document: Option<Box<Emitter>>,
    // a document was started with `drive_partial` and is not complete
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
    /// The clone of a serializer that writes a document in parts cannot
    /// write more documents (see
    /// [`StreamSerializer::in_progress`](ser::StreamSerializer::in_progress)).
    fn clone(&self) -> Serializer {
        Serializer {
            config: self.config.clone(),
            out: self.out.clone(),
            written: self.written,
            document: None,
            in_progress: self.in_progress,
        }
    }
}

impl std::fmt::Debug for Serializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Serializer")
            .field("config", &self.config)
            .field("output", &self.out)
            .field("written", &self.written)
            .field("in_progress", &self.in_progress)
            .finish()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(&SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: &SerializerConfig) -> Serializer {
        Serializer::with_written(config, 0)
    }

    /// Creates a serializer for a stream that continues after the given
    /// number of documents.
    ///
    /// This is useful to append to a stream that was written before: the
    /// next document starts with `---`.
    pub fn with_written(config: &SerializerConfig, written: usize) -> Serializer {
        Serializer {
            config: config.clone(),
            out: String::new(),
            written,
            document: None,
            in_progress: false,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SerializerConfig {
        &self.config
    }

    /// Returns the number of documents that were written.
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

    /// Returns the documents written so far (that were not cleared).
    pub fn as_str(&self) -> &str {
        &self.out
    }

    /// Returns the documents.
    pub fn finish(self) -> String {
        self.out
    }

    /// Sets the context the values are serialized in.
    ///
    /// This replaces the context of the configuration (see
    /// [`SerializerConfig::set_context`]).  The values of the context
    /// are the defaults of the extension values of the state (see
    /// [`Context`](deser_core::Context)).  A context set on the driver
    /// (for instance in the setup callback of `serialize_with`) takes
    /// precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.config.context = context;
    }

    /// Returns the context the values are serialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.config.context
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        // only `drive_partial` continues a document
        if self.in_progress {
            return Err(Error::in_progress());
        }
        ser::StreamSerializer::drive_partial(self, driver, usize::MAX).map(|_| ())
    }
}

impl ser::StreamSerializer for Serializer {
    fn output(&self) -> &[u8] {
        self.out.as_bytes()
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
        if self.document.is_none() && self.in_progress {
            return Err(Error::in_progress());
        }
        // the parts of a document that failed stay written (see
        // `in_progress`)
        if !self.config.document_part(
            self.written,
            &mut self.document,
            driver,
            &mut self.out,
            limit,
        )? {
            self.in_progress = true;
            return Ok(false);
        }
        self.in_progress = false;
        self.written += 1;
        Ok(true)
    }

    fn in_progress(&self) -> bool {
        self.in_progress
    }
}

#[cfg(feature = "io")]
impl SerializerConfig {
    /// Creates a writer of YAML documents (see
    /// [`deser::io::Writer`](deser_core::io::Writer)).
    ///
    /// Every value is written as a document, documents after the first
    /// start with `---`.  The output of large documents is written in parts
    /// while they are serialized.
    ///
    /// ```
    /// use deser_yaml::SerializerConfig;
    ///
    /// let mut writer = SerializerConfig::new().writer(Vec::new());
    /// writer.write(&"a").unwrap();
    /// writer.write(&vec![1, 2]).unwrap();
    /// assert_eq!(writer.into_inner(), b"a\n---\n- 1\n- 2\n");
    /// ```
    pub fn writer<W: std::io::Write>(&self, writer: W) -> deser_core::io::Writer<W, Serializer> {
        deser_core::io::Writer::new(writer, Serializer::with_config(self))
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
/// The output of large documents is written in parts while they are
/// serialized (see [`deser::io`](deser_core::io)), the writer does not
/// need to be buffered.
///
/// ```
/// let mut out = Vec::new();
/// deser_yaml::to_writer(&mut out, &vec![1, 2]).unwrap();
/// assert_eq!(out, b"- 1\n- 2\n");
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// Serializes a value to YAML.
///
/// This uses the default [`SerializerConfig`], see there for more
/// information.
///
/// ```
/// use deser::Serialize;
///
/// #[derive(Serialize)]
/// struct Service {
///     image: String,
///     ports: Vec<u16>,
///     command: Option<String>,
/// }
///
/// let service = Service {
///     image: "nginx".into(),
///     ports: vec![80, 443],
///     command: None,
/// };
/// assert_eq!(
///     deser_yaml::to_string(&service).unwrap(),
///     "image: nginx\nports:\n  - 80\n  - 443\ncommand: null\n"
/// );
/// ```
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}
