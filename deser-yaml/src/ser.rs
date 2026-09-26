use deser::adapters::bytes::BytesFormat;
use deser::ser::SerializeDriver;
use deser::{Error, Serialize};

use crate::emit::Emitter;
use crate::resolve::Version;

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
/// * block collections, sequences in mappings are indented
///   ([`indent_sequences`](Self::indent_sequences)), empty collections are
///   written as `{}` and `[]`.
/// * strings are plain if possible, otherwise single-quoted (double-quoted if
///   they need escapes).  Strings are quoted if readers of YAML 1.1 would
///   read them as something else (`yes`, `0777`, timestamps, see
///   [`compat`](Self::compat)).
/// * strings with line breaks are literal block scalars (`|`).
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
    pub(crate) indent: usize,
    pub(crate) indent_sequences: bool,
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
            indent: 2,
            indent_sequences: true,
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
        }
    }

    /// Sets the number of spaces per indentation level.
    ///
    /// The default is 2.  Values outside of `1..=9` are clamped.
    pub const fn indent(mut self, indent: usize) -> SerializerConfig {
        self.indent = if indent < 1 {
            1
        } else if indent > 9 {
            9
        } else {
            indent
        };
        self
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
    /// Documents of a [`Serializer`] after the first one always start with
    /// `---`.
    pub const fn document_start(mut self, yes: bool) -> SerializerConfig {
        self.document_start = yes;
        self
    }

    /// Starts documents with a `%YAML 1.2` directive.
    pub const fn version_directive(mut self, yes: bool) -> SerializerConfig {
        self.version_directive = yes;
        self
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
        let mut serializer = Serializer::new(self.clone());
        serializer.serialize_with(value, setup)?;
        Ok(serializer.finish())
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

/// Serializes a stream of documents.
///
/// Every value is written as a document, documents after the first start
/// with `---`.
///
/// ```
/// use deser_yaml::{Serializer, SerializerConfig};
///
/// let mut serializer = Serializer::new(SerializerConfig::new());
/// serializer.serialize(&"a").unwrap();
/// serializer.serialize(&vec![1, 2]).unwrap();
/// assert_eq!(serializer.finish(), "a\n---\n- 1\n- 2\n");
/// ```
#[derive(Debug)]
pub struct Serializer {
    config: SerializerConfig,
    out: String,
    documents: usize,
}

impl Serializer {
    /// Creates a serializer with the given configuration.
    pub fn new(config: SerializerConfig) -> Serializer {
        Serializer {
            config,
            out: String::new(),
            documents: 0,
        }
    }

    /// Serializes a value as document.
    pub fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        self.serialize_with(value, |_| {})
    }

    /// Serializes a value as document with a configured driver.
    ///
    /// See [`SerializerConfig::to_string_with`].  If the serialization fails,
    /// nothing is written.
    pub fn serialize_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut out = String::new();
        if self.config.version_directive {
            out.push_str("%YAML 1.2\n---\n");
        } else if self.config.document_start || self.documents > 0 {
            out.push_str("---\n");
        }
        let mut emitter = Emitter::new(&self.config, out);
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        driver.drive(|event, state| emitter.event(event, state))?;
        let document = emitter.finish()?;
        self.out.push_str(&document);
        self.documents += 1;
        Ok(())
    }

    /// Returns the written documents.
    pub fn finish(self) -> String {
        self.out
    }

    /// Returns the documents written so far.
    pub fn output(&self) -> &str {
        &self.out
    }
}
