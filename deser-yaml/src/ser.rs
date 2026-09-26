use deser::adapters::bytes::BytesFormat;
use deser::ser::SerializeDriver;
use deser::{Error, Serialize};

use crate::emit::Emitter;
use crate::resolve::Version;

/// How the output is indented.
///
/// See [`SerializerConfig::indent`].
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
/// Collections with the [`Layout::Compact`](deser::hints::Layout) hint are
/// always written in flow style, collections with
/// [`Layout::Expanded`](deser::hints::Layout) never (unless they are in a
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
/// * block collections indented by two spaces (see [`indent`](Self::indent)),
///   sequences in mappings are indented
///   ([`indent_sequences`](Self::indent_sequences)), empty collections are
///   written as `{}` and `[]`.  Compact collections (see
///   [`hints`](deser::hints)) are written in flow style, see
///   [`flow`](Self::flow).
/// * strings are plain if possible, otherwise single-quoted (double-quoted if
///   they need escapes).  Strings are quoted if readers of YAML 1.1 would
///   read them as something else (`yes`, `0777`, timestamps, see
///   [`compat`](Self::compat)).
/// * strings with line breaks are literal block scalars (`|`), the style of
///   individual strings can be requested (see [`style`](crate::style)).
/// * bytes are written as `!!binary` (see [`binary`](Self::binary)).
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_yaml::SerializerConfig;
///
/// let mut value = BTreeMap::new();
/// value.insert("items", vec!["a", "yes"]);
/// assert_eq!(deser_yaml::to_string(&value).unwrap(), "items:\n  - a\n  - 'yes'\n");
///
/// const INDENTLESS: SerializerConfig = SerializerConfig::new().indent_sequences(false);
/// assert_eq!(INDENTLESS.to_string(&value).unwrap(), "items:\n- a\n- 'yes'\n");
/// ```
///
/// [`to_string`](Self::to_string) works like the
/// [`to_string`](crate::to_string) function.
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
    pub(crate) bytes: BytesFormat,
    pub(crate) timestamp_tag: bool,
    pub(crate) document_start: bool,
    pub(crate) version_directive: bool,
    pub(crate) end_documents: bool,
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
            bytes: BytesFormat::BASE64,
            timestamp_tag: false,
            document_start: false,
            version_directive: false,
            end_documents: false,
        }
    }

    /// Sets how the output is indented.
    ///
    /// The default is [`Indent::Spaces(2)`](Indent::Spaces), values outside
    /// of `1..=9` are clamped.  With [`Indent::None`] documents are written
    /// on a single line in flow style, the [flow policy](Self::flow) and
    /// [`Layout`](deser::hints::Layout) hints have no effect then:
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
    /// const WIDE: SerializerConfig = SerializerConfig::new().indent(Indent::Spaces(4));
    /// assert_eq!(WIDE.to_string(&config).unwrap(), "name: web\nports:\n    - 80\n    - 443\n");
    /// const LINE: SerializerConfig = SerializerConfig::new().indent(Indent::None);
    /// assert_eq!(LINE.to_string(&config).unwrap(), "{name: web, ports: [80, 443]}\n");
    /// ```
    pub const fn indent(mut self, indent: Indent) -> SerializerConfig {
        self.indent = match indent {
            Indent::None => Indent::None,
            Indent::Spaces(0) => Indent::Spaces(1),
            Indent::Spaces(n) if n > 9 => Indent::Spaces(9),
            Indent::Spaces(n) => Indent::Spaces(n),
        };
        self
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
    pub const fn indent_sequences(mut self, yes: bool) -> SerializerConfig {
        self.indent_sequences = yes;
        self
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
    /// const FLOW: SerializerConfig = SerializerConfig::new().flow(FlowPolicy::LeafIfFits(80));
    /// assert_eq!(
    ///     FLOW.to_string(&config).unwrap(),
    ///     "ports: [80, 443]\ngroups:\n  - [1]\n  - [2, 3]\n"
    /// );
    /// ```
    pub const fn flow(mut self, policy: FlowPolicy) -> SerializerConfig {
        self.flow = policy;
        self
    }

    /// Folds long strings at the given width.
    ///
    /// Strings without line breaks that are longer than the width are
    /// written as folded block scalars (`>`) with lines that do not exceed
    /// the width if possible.  Strings are only folded if they read back
    /// unchanged.  By default strings are not folded.  The width is also used
    /// for strings with the [`Folded`](crate::style::Folded) hint (80 if not
    /// set).
    pub const fn fold_width(mut self, width: Option<usize>) -> SerializerConfig {
        self.fold_width = width;
        self
    }

    /// Sets how strings are quoted that cannot be written plain.
    pub const fn quote_style(mut self, style: QuoteStyle) -> SerializerConfig {
        self.quote_style = style;
        self
    }

    /// Quotes all strings, including strings with line breaks.
    pub const fn quote_all(mut self, yes: bool) -> SerializerConfig {
        self.quote_all = yes;
        self
    }

    /// Sets how strings with line breaks are written.
    pub const fn multiline(mut self, style: MultilineStyle) -> SerializerConfig {
        self.multiline = style;
        self
    }

    /// Sets how null is written.
    pub const fn null_style(mut self, style: NullStyle) -> SerializerConfig {
        self.null_style = style;
        self
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
    /// const V1_2: SerializerConfig = SerializerConfig::new().compat(Version::V1_2);
    /// assert_eq!(V1_2.to_string(&"yes").unwrap(), "yes\n");
    /// ```
    pub const fn compat(mut self, version: Version) -> SerializerConfig {
        self.compat = version;
        self
    }

    /// Writes bytes as `!!binary`.
    ///
    /// By default (`true`) YAML is a format with native bytes: bytes are
    /// written as base64 with the `!!binary` tag, also bytes that request a
    /// representation for formats without native bytes (see
    /// [`BytesFallback`](deser::adapters::bytes::BytesFallback)).  With
    /// `false` bytes are represented like in JSON: in the format they request
    /// or the format configured with [`bytes`](Self::bytes).
    ///
    /// ```
    /// use deser::adapters::bytes::{BytesFormat, Hex};
    /// use deser_yaml::SerializerConfig;
    ///
    /// assert_eq!(deser_yaml::to_string(&b"\x01\xff").unwrap(), "!!binary Af8=\n");
    /// const HEX: SerializerConfig = SerializerConfig::new()
    ///     .binary(false)
    ///     .bytes(BytesFormat::encoded::<Hex>());
    /// assert_eq!(HEX.to_string(&b"\x01\xff").unwrap(), "01ff\n");
    /// ```
    ///
    /// Bytes in other formats than base64 (or sequences) need to be
    /// deserialized with the same format (see
    /// [`DeserializerConfig::bytes`](crate::DeserializerConfig::bytes)).
    pub const fn binary(mut self, yes: bool) -> SerializerConfig {
        self.binary = yes;
        self
    }

    /// Sets how bytes are represented if [`binary`](Self::binary) is off.
    ///
    /// The default is [`BytesFormat::BASE64`].
    pub const fn bytes(mut self, format: BytesFormat) -> SerializerConfig {
        self.bytes = format;
        self
    }

    /// Writes date-times with the `!!timestamp` tag.
    ///
    /// Dates and date-times with offset ([`Datetime`](deser::ext::Datetime))
    /// are written as YAML timestamps.  By default they are plain which YAML
    /// 1.1 readers resolve as timestamps and YAML 1.2 readers as strings
    /// (which date / time types accept).  With the tag all readers resolve
    /// them as timestamps.  Local date-times and times are always strings.
    pub const fn timestamp_tag(mut self, yes: bool) -> SerializerConfig {
        self.timestamp_tag = yes;
        self
    }

    /// Always starts documents with `---`.
    ///
    /// When writing a stream of documents (see [`deser::io`]), documents
    /// after the first one always start with `---`.
    pub const fn document_start(mut self, yes: bool) -> SerializerConfig {
        self.document_start = yes;
        self
    }

    /// Starts documents with a `%YAML 1.2` directive.
    pub const fn version_directive(mut self, yes: bool) -> SerializerConfig {
        self.version_directive = yes;
        self
    }

    /// Ends documents with a document end marker (`...`).
    ///
    /// When a stream of documents is read (see [`deser::io`]), a document
    /// is complete once the next document starts or once it's ended with
    /// `...`.  For streams that stay open (like sockets) this allows the
    /// reader to see the end of a document without waiting for the next
    /// one.
    ///
    /// ```
    /// use deser::io::Writer;
    /// use deser_yaml::SerializerConfig;
    ///
    /// const ENDED: SerializerConfig = SerializerConfig::new().end_documents(true);
    /// let mut writer = Writer::new(Vec::new(), ENDED);
    /// writer.write(&"a").unwrap();
    /// writer.write(&"b").unwrap();
    /// assert_eq!(writer.into_inner(), b"a\n...\n---\nb\n...\n");
    /// ```
    pub const fn end_documents(mut self, yes: bool) -> SerializerConfig {
        self.end_documents = yes;
        self
    }

    /// Serializes the value of a driver as a document of a stream.
    ///
    /// `index` is the number of documents written before.
    pub(crate) fn document(
        &self,
        driver: &mut SerializeDriver<'_>,
        index: usize,
    ) -> Result<String, Error> {
        let mut out = String::new();
        if self.version_directive {
            // directives can only follow the end of a document
            if index > 0 && !self.end_documents {
                out.push_str("...\n");
            }
            out.push_str("%YAML 1.2\n---\n");
        } else if self.document_start || index > 0 {
            out.push_str("---\n");
        }
        let mut emitter = Emitter::new(self, out);
        driver.drive(|event, state| emitter.event(event, state))?;
        let mut document = emitter.finish()?;
        if self.end_documents {
            document.push_str("...\n");
        }
        Ok(document)
    }

    /// Serializes the given value.
    pub fn to_string(&self, value: &dyn Serialize) -> Result<String, Error> {
        self.to_string_with(value, |_| {})
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser::ser::Layer)s.
    pub fn to_string_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<String, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        self.document(&mut driver, 0)
    }
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
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}
