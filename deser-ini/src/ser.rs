use std::borrow::Cow;
use std::collections::HashSet;

use deser_core::ext::Number;
use deser_core::ser::{self, SerializeDriver, SerializeRef};
use deser_core::{Atom, BytesFormat, Error, ErrorKind, Event, Serialize, State};

use crate::parser::comment_start;
use crate::{Continuation, InlineComments, Quotes, Syntax};

/// Configures how values are serialized to INI files.
///
/// The value has to serialize to a map (for instance a struct or a map
/// type).  Its entries with maps as values are written as sections, the
/// others before the first section.  The values of sections cannot be maps
/// (in git's syntax they can: they are written as subsections).  Sequences
/// are written as repeated keys (`tags = a` `tags = b`), their elements
/// cannot be maps or sequences.  Null values (like `None`) of map entries
/// are skipped, null values in sequences are written as keys without
/// value.  Empty maps are written as empty sections, empty sequences are
/// not written.
///
/// The options describe the dialect the file is written for, they are the
/// same as the ones of [`DeserializerConfig`](crate::DeserializerConfig)
/// and the file reads back with the same options.  Values that the dialect
/// cannot represent are an error, for instance values that start with
/// whitespace without [`Quotes::Value`].  Values are quoted only if needed
/// (whitespace at the start or end, comment characters), values with line
/// breaks are written with continuation lines.
///
/// Numbers are written with the shortest text that reads back as the same
/// value, booleans as `true` and `false` and bytes as base64 (or the
/// [`BytesFormat`](deser_core::BytesFormat) of the context).
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::SerializerConfig;
///
/// let value = BTreeMap::from([
///     ("tox", BTreeMap::from([("envlist", "py312, py313")])),
///     ("testenv", BTreeMap::from([("commands", "pytest\nruff check")])),
/// ]);
/// assert_eq!(
///     SerializerConfig::python().to_string(&value).unwrap(),
///     "[testenv]\ncommands = pytest\n    ruff check\n\n[tox]\nenvlist = py312, py313\n"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    syntax: Syntax,
    inline_comments: InlineComments,
    colon_delimiter: bool,
    continuation: Continuation,
    quotes: Quotes,
    context: deser_core::Context,
}

impl Default for SerializerConfig {
    fn default() -> SerializerConfig {
        SerializerConfig::new()
    }
}

impl SerializerConfig {
    /// Creates the default configuration (common INI files).
    ///
    /// The options are the ones of
    /// [`DeserializerConfig::new`](crate::DeserializerConfig::new).
    pub const fn new() -> SerializerConfig {
        SerializerConfig {
            syntax: Syntax::Ini,
            inline_comments: InlineComments::AfterWhitespace,
            colon_delimiter: true,
            continuation: Continuation::Indented,
            quotes: Quotes::Value,
            context: deser_core::Context::new(),
        }
    }

    /// Creates the configuration for the files of Python's `configparser`.
    ///
    /// See [`DeserializerConfig::python`](crate::DeserializerConfig::python).
    pub const fn python() -> SerializerConfig {
        SerializerConfig::builder()
            .inline_comments(InlineComments::None)
            .quotes(Quotes::None)
            .build()
    }

    /// Creates the configuration for git's config files.
    ///
    /// See [`Syntax::Git`].
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_ini::SerializerConfig;
    ///
    /// let value = BTreeMap::from([(
    ///     "remote",
    ///     BTreeMap::from([("origin", BTreeMap::from([("url", "git@x:y.git")]))]),
    /// )]);
    /// assert_eq!(
    ///     SerializerConfig::git().to_string(&value).unwrap(),
    ///     "[remote \"origin\"]\n\turl = git@x:y.git\n"
    /// );
    /// ```
    pub const fn git() -> SerializerConfig {
        SerializerConfig::builder().syntax(Syntax::Git).build()
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
    /// writers created with the configuration use this context.  A context
    /// set on the driver takes precedence.
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

    /// Sets the syntax.
    ///
    /// The default is [`Syntax::Ini`].  With [`Syntax::Git`] the other
    /// options (except for the context) are ignored.
    pub const fn set_syntax(&mut self, syntax: Syntax) {
        self.syntax = syntax;
    }

    /// Sets where comments start after values.
    ///
    /// The default is [`InlineComments::AfterWhitespace`], values that
    /// would be read as comments are quoted.
    pub const fn set_inline_comments(&mut self, comments: InlineComments) {
        self.inline_comments = comments;
    }

    /// Sets if `:` separates keys and values (like `=`).
    ///
    /// The default is `true`, keys that contain `:` are an error.
    pub const fn set_colon_delimiter(&mut self, yes: bool) {
        self.colon_delimiter = yes;
    }

    /// Sets how values continue on the next lines.
    ///
    /// The default is [`Continuation::Indented`] which writes values with
    /// line breaks with continuation lines.  Otherwise values with line
    /// breaks are an error.
    pub const fn set_continuation(&mut self, continuation: Continuation) {
        self.continuation = continuation;
    }

    /// Sets if values can be quoted.
    ///
    /// The default is [`Quotes::Value`].  With [`Quotes::None`] values that
    /// need quotes are an error.
    pub const fn set_quotes(&mut self, quotes: Quotes) {
        self.quotes = quotes;
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
        self.serialize_driver(&mut driver)
    }

    /// Serializes a value whose type is erased (see
    /// [`to_string`](Self::to_string)).
    ///
    /// This is not generic: the code that exists for every type only
    /// erases it.
    fn to_string_ref(&self, value: SerializeRef<'_>) -> Result<String, Error> {
        let mut driver = SerializeDriver::from_ref(value);
        self.apply_context(&mut driver);
        self.serialize_driver(&mut driver)
    }

    /// Serializes the value of a driver.
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
    ) -> Result<String, Error> {
        let mut writer = Writer {
            config: self,
            bytes: BytesFormat::of(driver.state()),
            stack: Vec::new(),
            out: String::new(),
        };
        driver.drive(|event, state| writer.event(event, state))?;
        Ok(writer.out)
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

    /// Sets the syntax.
    ///
    /// See [`SerializerConfig::set_syntax`].
    pub const fn syntax(mut self, syntax: Syntax) -> SerializerConfigBuilder {
        self.value.set_syntax(syntax);
        self
    }

    /// Sets where comments start after values.
    ///
    /// See [`SerializerConfig::set_inline_comments`].
    pub const fn inline_comments(mut self, comments: InlineComments) -> SerializerConfigBuilder {
        self.value.set_inline_comments(comments);
        self
    }

    /// Sets if `:` separates keys and values (like `=`).
    ///
    /// See [`SerializerConfig::set_colon_delimiter`].
    pub const fn colon_delimiter(mut self, yes: bool) -> SerializerConfigBuilder {
        self.value.set_colon_delimiter(yes);
        self
    }

    /// Sets how values continue on the next lines.
    ///
    /// See [`SerializerConfig::set_continuation`].
    pub const fn continuation(mut self, continuation: Continuation) -> SerializerConfigBuilder {
        self.value.set_continuation(continuation);
        self
    }

    /// Sets if values can be quoted.
    ///
    /// See [`SerializerConfig::set_quotes`].
    pub const fn quotes(mut self, quotes: Quotes) -> SerializerConfigBuilder {
        self.value.set_quotes(quotes);
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

/// Serializes values into INI files.
///
/// An INI file holds a single value, writing a second one fails.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&BTreeMap::from([("a", 1)])).unwrap();
/// assert!(serializer.serialize(&BTreeMap::from([("b", 2)])).is_err());
/// assert_eq!(serializer.finish(), "a = 1\n");
/// ```
///
/// The serializer is also the stream serializer of INI files (see
/// [`StreamSerializer`](ser::StreamSerializer)).  An INI file cannot be
/// written in parts: the keys before the first section can come last in the
/// value.  To write to a [`Write`](std::io::Write) use
/// [`SerializerConfig::writer`].
#[derive(Debug, Clone)]
pub struct Serializer {
    config: SerializerConfig,
    out: String,
    written: bool,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: SerializerConfig) -> Serializer {
        Serializer {
            config,
            out: String::new(),
            written: false,
        }
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

    /// Returns the configuration.
    pub fn config(&self) -> &SerializerConfig {
        &self.config
    }

    /// Returns the output written so far (that was not cleared).
    pub fn as_str(&self) -> &str {
        &self.out
    }

    /// Returns the output.
    pub fn finish(self) -> String {
        self.out
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        if !self.config.context.is_empty() {
            driver.set_default_context(self.config.context.clone());
        }
        if self.written {
            return Err(Error::new(
                ErrorKind::InvalidState,
                "an INI file holds a single value",
            ));
        }
        let ini = self.config.serialize_driver(driver)?;
        self.out.push_str(&ini);
        self.written = true;
        Ok(())
    }
}

impl ser::StreamSerializer for Serializer {
    fn output(&self) -> &[u8] {
        self.out.as_bytes()
    }

    fn clear_output(&mut self) {
        self.out.clear();
    }
}

#[cfg(feature = "io")]
impl SerializerConfig {
    /// Creates a writer of an INI file (see
    /// [`deser::io::Writer`](deser_core::io::Writer)).
    ///
    /// A stream holds a single file, writing a second value fails.  The file
    /// is written with a single write once it's complete.
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
/// The file is written with a single write once it's complete.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let mut out = Vec::new();
/// deser_ini::to_writer(&mut out, &BTreeMap::from([("a", 1)])).unwrap();
/// assert_eq!(out, b"a = 1\n");
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// Serializes a value to an INI file.
///
/// This uses the default [`SerializerConfig`], see there for more
/// information.
///
/// ```
/// #[derive(deser::Serialize)]
/// struct Config {
///     debug: bool,
///     server: Server,
/// }
///
/// #[derive(deser::Serialize)]
/// struct Server {
///     host: &'static str,
///     greeting: &'static str,
/// }
///
/// let config = Config {
///     debug: false,
///     server: Server { host: "localhost", greeting: " hello ; world" },
/// };
/// assert_eq!(
///     deser_ini::to_string(&config).unwrap(),
///     "debug = false\n\n[server]\nhost = localhost\ngreeting = \" hello ; world\"\n"
/// );
/// ```
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// A map that is being written: the root, a section or a subsection.
struct Table {
    /// The name of a section.
    name: String,
    /// 0 for the root, 1 for sections and 2 for subsections.
    depth: usize,
    /// The header (with line break).
    header: String,
    /// The lines of the keys.
    body: String,
    /// The sections or subsections in the table.
    tables: String,
    /// The key of the next value.
    key: Option<String>,
    /// The names of the keys and tables in git's config files (lowercased
    /// like git reads them, except for subsections).
    names: HashSet<String>,
}

enum Frame {
    Table(Table),
    /// A sequence (written as repeated key).
    Seq(String),
}

/// Writes the events of a value.
struct Writer<'c> {
    config: &'c SerializerConfig,
    /// How bytes are written (from the state).
    bytes: BytesFormat,
    stack: Vec<Frame>,
    out: String,
}

impl Writer<'_> {
    fn event(&mut self, event: Event, _state: &State) -> Result<(), Error> {
        /// What the event is for.
        enum Position {
            Start,
            Key,
            Value,
            Element,
        }

        let position = match self.stack.last() {
            None => Position::Start,
            Some(Frame::Table(table)) if table.key.is_none() => Position::Key,
            Some(Frame::Table(_)) => Position::Value,
            Some(Frame::Seq(_)) => Position::Element,
        };
        match position {
            Position::Start => match event {
                Event::MapStart(_) => self.stack.push(Frame::Table(Table {
                    name: String::new(),
                    depth: 0,
                    header: String::new(),
                    body: String::new(),
                    tables: String::new(),
                    key: None,
                    names: HashSet::new(),
                })),
                Event::Atom(Atom::Null) => {}
                _ => {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "INI files hold maps (like structs)",
                    ));
                }
            },
            Position::Key => match event {
                Event::Atom(ref atom) => {
                    let key = self.key_text(atom)?.into_owned();
                    self.top_table().key = Some(key);
                }
                Event::MapEnd => self.end_table(),
                _ => return Err(unsupported_key()),
            },
            Position::Value => {
                let key = self.top_table().key.take().unwrap_or_default();
                match event {
                    Event::Atom(ref atom) => {
                        if let Some(value) = self.value_text(atom)? {
                            let mut line = String::new();
                            self.write_entry(&mut line, &key, Some(&value))?;
                            self.claim_name(&key, false)?;
                            self.top_table().body.push_str(&line);
                        }
                    }
                    Event::SeqStart(_) => {
                        self.check_key(&key)?;
                        self.claim_name(&key, false)?;
                        self.stack.push(Frame::Seq(key));
                    }
                    Event::MapStart(_) => self.start_table(key)?,
                    Event::MapEnd | Event::SeqEnd => {
                        return Err(Error::new(ErrorKind::InvalidState, "unexpected end event"));
                    }
                }
            }
            Position::Element => match event {
                Event::SeqEnd => {
                    self.stack.pop();
                }
                Event::Atom(ref atom) => {
                    let key = match self.stack.last() {
                        Some(Frame::Seq(key)) => key.clone(),
                        _ => unreachable!(),
                    };
                    let value = self.value_text(atom)?;
                    let mut line = String::new();
                    self.write_entry(&mut line, &key, value.as_deref())?;
                    let len = self.stack.len();
                    match self.stack[len - 2] {
                        Frame::Table(ref mut table) => table.body.push_str(&line),
                        Frame::Seq(_) => unreachable!("sequences are in tables"),
                    }
                }
                Event::MapStart(_) | Event::SeqStart(_) => {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "INI files cannot hold sequences of maps or sequences",
                    ));
                }
                Event::MapEnd => {
                    return Err(Error::new(ErrorKind::InvalidState, "unexpected end event"));
                }
            },
        }
        Ok(())
    }

    /// Returns the table on the top of the stack.
    fn top_table(&mut self) -> &mut Table {
        match self.stack.last_mut() {
            Some(Frame::Table(table)) => table,
            _ => unreachable!("values of maps are written into tables"),
        }
    }

    /// Starts a section or subsection.
    fn start_table(&mut self, key: String) -> Result<(), Error> {
        let parent = self.top_table();
        let depth = parent.depth + 1;
        let git = self.config.syntax == Syntax::Git;
        let header = match depth {
            1 => {
                self.check_section(&key)?;
                format!("[{}]\n", key)
            }
            2 if git => {
                if key.contains(['\n', '\0']) {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        format!("subsection {:?} cannot be written", key),
                    ));
                }
                let mut header = format!("[{} \"", self.top_table().name);
                for c in key.chars() {
                    if c == '\\' || c == '"' {
                        header.push('\\');
                    }
                    header.push(c);
                }
                header.push_str("\"]\n");
                header
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    if git {
                        "subsections of git's config files cannot hold maps"
                    } else {
                        "sections of INI files cannot hold maps"
                    },
                ));
            }
        };
        self.claim_name(&key, depth == 2)?;
        self.stack.push(Frame::Table(Table {
            name: key,
            depth,
            header,
            body: String::new(),
            tables: String::new(),
            key: None,
            names: HashSet::new(),
        }));
        Ok(())
    }

    /// Ends the table on the top of the stack.
    fn end_table(&mut self) {
        let table = match self.stack.pop() {
            Some(Frame::Table(table)) => table,
            _ => unreachable!("tables are ended by MapEnd"),
        };
        // tables are separated by empty lines (also from the keys before
        // them, which can be written after them)
        let mut text = String::new();
        if table.depth > 0 && (!table.body.is_empty() || table.tables.is_empty()) {
            // a section that only has subsections needs no header
            text.push_str(&table.header);
        }
        text.push_str(&table.body);
        if !table.body.is_empty() && !table.tables.is_empty() {
            text.push('\n');
        }
        text.push_str(&table.tables);
        if table.depth == 0 {
            self.out.push_str(&text);
            return;
        }
        let parent = self.top_table();
        if !parent.tables.is_empty() {
            parent.tables.push('\n');
        }
        parent.tables.push_str(&text);
    }

    /// Records the name of a key or table in the table on the top of the
    /// stack.
    ///
    /// The names of keys and sections in git's config files are case
    /// insensitive, different keys (like `a` and `A`) cannot have the same
    /// name.  The names of subsections are case sensitive.
    fn claim_name(&mut self, name: &str, case_sensitive: bool) -> Result<(), Error> {
        if self.config.syntax != Syntax::Git {
            return Ok(());
        }
        let name = if case_sensitive {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        };
        if self.top_table().names.insert(name) {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::UnsupportedType,
                "different keys have the same name in git's config files",
            ))
        }
    }

    /// Checks if a key can be written.
    fn check_key(&self, key: &str) -> Result<(), Error> {
        let valid = match self.config.syntax {
            Syntax::Git => {
                key.starts_with(|c: char| c.is_ascii_alphabetic())
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            }
            // a byte order mark at the start of the file is skipped
            Syntax::Ini => {
                !key.is_empty()
                    && key.trim_matches([' ', '\t']) == key
                    && !key.starts_with(['[', ';', '#', '\u{feff}'])
                    && !key.contains(['=', '\n', '\r'])
                    && !(self.config.colon_delimiter && key.contains(':'))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("key {:?} cannot be written", key),
            ))
        }
    }

    /// Checks if the name of a section can be written.
    fn check_section(&self, name: &str) -> Result<(), Error> {
        let valid = match self.config.syntax {
            Syntax::Git => {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            }
            // the first `]` that is followed by nothing or a comment
            // ends the name when it's read
            Syntax::Ini => {
                !name.contains(['\n', '\r'])
                    && name.match_indices(']').all(|(pos, _)| {
                        !name[pos + 1..]
                            .trim_start_matches([' ', '\t'])
                            .starts_with([';', '#'])
                    })
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("section {:?} cannot be written", name),
            ))
        }
    }

    /// Writes the line of a key (without value for `None`).
    fn write_entry(&self, out: &mut String, key: &str, value: Option<&str>) -> Result<(), Error> {
        self.check_key(key)?;
        let git = self.config.syntax == Syntax::Git;
        if git {
            out.push('\t');
        }
        out.push_str(key);
        if let Some(value) = value {
            out.push_str(" =");
            let value = if git {
                git_value(value)
            } else {
                self.ini_value(value)?
            };
            // `;` only starts a comment after whitespace
            if !value.is_empty() && !value.starts_with(';') {
                out.push(' ');
            }
            out.push_str(&value);
        }
        out.push('\n');
        Ok(())
    }

    /// Returns `true` if a value of a single line does not read back
    /// without quotes.
    fn needs_quotes(&self, value: &str) -> bool {
        // a value that starts with `;` is written without whitespace in
        // front, so it's not a comment
        value.trim_matches([' ', '\t']) != value
            || comment_start(value, self.config.inline_comments) < value.len()
            || (self.config.quotes == Quotes::Value && value.starts_with(['"', '\'']))
            || (self.config.continuation == Continuation::Backslash && value.ends_with('\\'))
    }

    /// Returns how a value is written in INI files.
    fn ini_value<'v>(&self, value: &'v str) -> Result<Cow<'v, str>, Error> {
        if value.contains(['\n', '\r']) {
            return self.multiline_value(value).map(Cow::Owned);
        }
        if !self.needs_quotes(value) {
            return Ok(Cow::Borrowed(value));
        }
        if self.config.quotes == Quotes::None {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("value {:?} cannot be written without quotes", value),
            ));
        }
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for c in value.chars() {
            if c == '\\' || c == '"' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        Ok(Cow::Owned(out))
    }

    /// Writes a value with line breaks as continuation lines.
    fn multiline_value(&self, value: &str) -> Result<String, Error> {
        let lines: Vec<&str> = value.split('\n').collect();
        let valid = self.config.continuation == Continuation::Indented
            && !value.contains('\r')
            && !lines[0].is_empty()
            && !lines[lines.len() - 1].is_empty()
            && lines.iter().all(|line| {
                line.is_empty()
                    || (!self.needs_quotes(line)
                        && !line.starts_with(['#', ';'])
                        && !(self.config.quotes == Quotes::Value && line.starts_with(['"', '\''])))
            });
        if !valid {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("value {:?} cannot be written as continuation lines", value),
            ));
        }
        let mut out = String::from(lines[0]);
        for line in &lines[1..] {
            out.push('\n');
            if !line.is_empty() {
                out.push_str("    ");
                out.push_str(line);
            }
        }
        Ok(out)
    }

    /// Returns the text of a map key.
    fn key_text<'a>(&self, atom: &'a Atom<'_>) -> Result<Cow<'a, str>, Error> {
        match atom {
            Atom::Null | Atom::Bytes(_) => Err(unsupported_key()),
            atom => self.value_text(atom)?.ok_or_else(unsupported_key),
        }
    }

    /// Returns the text of a value, `None` for null.
    fn value_text<'a>(&self, atom: &'a Atom<'_>) -> Result<Option<Cow<'a, str>>, Error> {
        Ok(Some(match *atom {
            Atom::Null => return Ok(None),
            // values whose type was inferred from text are written as value
            Atom::Implicit(ref value) => {
                return Ok(self
                    .value_text(&value.value().to_atom())?
                    .map(|text| Cow::Owned(text.into_owned())));
            }
            Atom::Bool(value) => Cow::Borrowed(if value { "true" } else { "false" }),
            Atom::Str(ref value) | Atom::Lexical(ref value) => Cow::Borrowed(&**value),
            Atom::Char(value) => Cow::Owned(value.to_string()),
            Atom::U64(value) => Cow::Owned(value.to_string()),
            Atom::I64(value) => Cow::Owned(value.to_string()),
            Atom::F32(value) => Cow::Owned(zmij::Buffer::new().format(value).into()),
            Atom::F64(value) => Cow::Owned(zmij::Buffer::new().format(value).into()),
            Atom::Bytes(ref bytes) => {
                let format = bytes.fallback.copied().unwrap_or(self.bytes);
                Cow::Owned(
                    format
                        .encode(bytes)
                        .or_else(|| BytesFormat::BASE64.encode(bytes))
                        .unwrap_or_default(),
                )
            }
            Atom::Ext(ref ext) => {
                if let Some(number) = ext.downcast_value_ref::<Number>() {
                    // numbers keep their text
                    Cow::Owned(number.as_str().to_string())
                } else if let Some(value) = ext.downcast_ref::<u128>() {
                    Cow::Owned(value.to_string())
                } else if let Some(value) = ext.downcast_ref::<i128>() {
                    Cow::Owned(value.to_string())
                } else {
                    match ext.fallback() {
                        Atom::Ext(_) => {
                            return Err(Error::new(
                                ErrorKind::UnsupportedType,
                                format!("INI files do not support {}", ext.name()),
                            ));
                        }
                        fallback => match self.value_text(&fallback)? {
                            Some(text) => Cow::Owned(text.into_owned()),
                            None => return Ok(None),
                        },
                    }
                }
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("INI files do not support {}", atom.name()),
                ));
            }
        }))
    }
}

/// Returns how a value is written in git's config files.
///
/// Values with whitespace at the start or end or with comment characters
/// are quoted.  Backslashes, quotes and line breaks, tabs and backspaces are
/// escaped.
fn git_value(value: &str) -> Cow<'_, str> {
    let quote = value.starts_with([' ', '\t'])
        || value.ends_with([' ', '\t'])
        || value.contains([';', '#', '\r']);
    if !quote && !value.contains(['\\', '"', '\n', '\t', '\x08']) {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len() + 2);
    if quote {
        out.push('"');
    }
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            c => out.push(c),
        }
    }
    if quote {
        out.push('"');
    }
    Cow::Owned(out)
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "keys of INI files must be strings, numbers or booleans",
    )
}
