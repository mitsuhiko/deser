use std::borrow::Cow;

use deser_core::de::{
    self, ContentKey, Deserialize, DeserializeDriver, LexicalRules, deserialize_value,
};
use deser_core::{Atom, ContainerShape, Error, ErrorKind, Event, Order, Source, Text};
use quick_xml::XmlVersion;
use quick_xml::events::{BytesRef, BytesStart, Event as XmlEvent};
use quick_xml::name::{QName, ResolveResult};
use quick_xml::reader::NsReader;

use crate::Names;
use crate::mixed::WhitespaceDepths;
use crate::root::{Declarations, RootData};

/// Configures how XML documents are deserialized.
///
/// See the [crate documentation](crate) for how XML maps onto the data
/// model of deser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    pub(crate) names: Names,
    resolve_namespaces: bool,
    track_locations: bool,
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
            names: Names::new(),
            resolve_namespaces: false,
            track_locations: true,
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

    /// Sets the prefix of the keys of attributes.
    ///
    /// The default is `@`: `<a href="x"/>` is `{"@href": "x"}`.
    pub const fn set_attribute_prefix(&mut self, prefix: &'static str) {
        self.names.attribute_prefix = prefix;
    }

    /// Sets the key of the text of elements that are maps.
    ///
    /// The default is `$text`: `<a href="x">y</a>` is
    /// `{"@href": "x", "$text": "y"}`.
    pub const fn set_text_key(&mut self, key: &'static str) {
        self.names.text_key = key;
    }

    /// Sets the prefixes of namespaces.
    ///
    /// Names are passed on as written in the document (`atom:link`),
    /// unless their namespace has a prefix here: then they are written
    /// with this prefix, whichever prefix the document uses.  The empty
    /// prefix leaves only the local name.  Names in other namespaces are
    /// passed on as written or, if namespaces are
    /// [resolved](Self::set_resolve_namespaces), as `{uri}local`.  The table
    /// can be written with [`prefixes!`](crate::prefixes).
    ///
    /// ```
    /// use deser_xml::DeserializerConfig;
    ///
    /// #[derive(deser::Deserialize)]
    /// struct Feed {
    ///     title: String,
    ///     #[deser(rename = "dc:creator")]
    ///     creator: String,
    /// }
    ///
    /// const CONFIG: DeserializerConfig = DeserializerConfig::new().namespaces(&[
    ///     ("", "http://www.w3.org/2005/Atom"),
    ///     ("dc", "http://purl.org/dc/elements/1.1/"),
    /// ]);
    /// let feed: Feed = CONFIG.from_str(r#"
    ///     <a:feed xmlns:a="http://www.w3.org/2005/Atom"
    ///             xmlns:x="http://purl.org/dc/elements/1.1/">
    ///       <a:title>Example</a:title>
    ///       <x:creator>Jane</x:creator>
    ///     </a:feed>
    /// "#).unwrap();
    /// assert_eq!(feed.title, "Example");
    /// assert_eq!(feed.creator, "Jane");
    /// ```
    pub const fn namespaces(
        mut self,
        namespaces: &'static [(&'static str, &'static str)],
    ) -> DeserializerConfig {
        self.names.namespaces = namespaces;
        self
    }

    /// Enables or disables resolving namespaces.
    ///
    /// By default names are passed on as written in the document.  If
    /// namespaces are resolved, names in a namespace are passed on as
    /// `{uri}local` (the notation of James Clark, attributes are
    /// `@{uri}local`) unless the namespace has a
    /// [prefix](Self::namespaces), so the prefixes of the document do not
    /// matter.  Names without namespace are their local name, the `xml`
    /// prefix is kept (`@xml:lang`).  Prefixes that are not declared are
    /// an error.  The default is `false`.
    ///
    /// The names can be written with [`qname!`](crate::qname) and
    /// [`namespace!`](crate::namespace):
    ///
    /// ```
    /// use deser_xml::DeserializerConfig;
    ///
    /// deser_xml::namespace!(atom = "http://www.w3.org/2005/Atom");
    ///
    /// #[derive(deser::Deserialize)]
    /// struct Link {
    ///     #[deser(rename = "@href")]
    ///     href: String,
    /// }
    ///
    /// #[derive(deser::Deserialize)]
    /// struct Feed {
    ///     #[deser(rename = atom!("title"))]
    ///     title: String,
    ///     #[deser(rename = atom!("link"))]
    ///     link: Link,
    /// }
    ///
    /// const CONFIG: DeserializerConfig =
    ///     DeserializerConfig::builder().resolve_namespaces(true).build();
    /// let xml = r#"
    ///     <feed xmlns="http://www.w3.org/2005/Atom">
    ///       <title>Example</title>
    ///       <link href="/a"/>
    ///     </feed>
    /// "#;
    /// let feed: Feed = CONFIG.from_str(xml).unwrap();
    /// assert_eq!(feed.title, "Example");
    /// assert_eq!(feed.link.href, "/a");
    /// ```
    pub const fn set_resolve_namespaces(&mut self, yes: bool) {
        self.resolve_namespaces = yes;
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser_core::State::input_range)), this
    /// controls if the input is published as [`Source`] so that errors can
    /// be resolved into lines and columns.  The default is `true`.
    pub const fn set_track_locations(&mut self, yes: bool) {
        self.track_locations = yes;
    }

    /// Deserializes a value from a string with this configuration.
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

    /// Deserializes a value from bytes with this configuration.
    ///
    /// The input must be UTF-8.
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

    /// Sets the prefix of the keys of attributes.
    ///
    /// See [`DeserializerConfig::set_attribute_prefix`].
    pub const fn attribute_prefix(mut self, prefix: &'static str) -> DeserializerConfigBuilder {
        self.value.set_attribute_prefix(prefix);
        self
    }

    /// Sets the key of the text of elements that are maps.
    ///
    /// See [`DeserializerConfig::set_text_key`].
    pub const fn text_key(mut self, key: &'static str) -> DeserializerConfigBuilder {
        self.value.set_text_key(key);
        self
    }

    /// Enables or disables resolving namespaces.
    ///
    /// See [`DeserializerConfig::set_resolve_namespaces`].
    pub const fn resolve_namespaces(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_resolve_namespaces(yes);
        self
    }

    /// Enables or disables location tracking.
    ///
    /// See [`DeserializerConfig::set_track_locations`].
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfigBuilder {
        self.value.set_track_locations(yes);
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

/// Deserializes a value from an XML string.
///
/// ```
/// #[derive(deser::Deserialize)]
/// struct Link {
///     #[deser(rename = "@href")]
///     href: String,
/// }
///
/// let link: Link = deser_xml::from_str(r#"<a href="/x"/>"#).unwrap();
/// assert_eq!(link.href, "/x");
/// ```
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    DeserializerConfig::new().from_str(s)
}

/// Deserializes a value from UTF-8 encoded XML.
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(bytes)
}

/// Deserializes XML documents.
pub struct Deserializer<'a> {
    input: &'a str,
    error: Option<Error>,
    config: DeserializerConfig,
    // the context the values are deserialized in
    context: deser_core::Context,
}

impl<'a> Deserializer<'a> {
    /// Creates a deserializer for a string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'a str) -> Deserializer<'a> {
        Deserializer::from_str_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a deserializer for a string with the given configuration.
    pub fn from_str_with_config(input: &'a str, config: &DeserializerConfig) -> Deserializer<'a> {
        Deserializer {
            input,
            error: None,
            config: config.clone(),
            context: deser_core::Context::new(),
        }
    }

    /// Creates a deserializer for UTF-8 encoded bytes.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a deserializer for UTF-8 encoded bytes with the given
    /// configuration.
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        // a byte order mark is not part of the document
        let input = input.strip_prefix(b"\xef\xbb\xbf").unwrap_or(input);
        match std::str::from_utf8(input) {
            Ok(input) => Deserializer::from_str_with_config(input, config),
            Err(err) => Deserializer {
                input: "",
                error: Some(Error::with_offset(
                    ErrorKind::Syntax,
                    "input is not valid UTF-8",
                    err.valid_up_to(),
                )),
                config: config.clone(),
                context: deser_core::Context::new(),
            },
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Deserializes the document.
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the document with a configured driver.
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

    /// Parses the document and feeds the events into the driver.
    ///
    /// Events are emitted while the document is parsed.  Text is passed on
    /// borrowed from the input unless it has references or line breaks
    /// that are normalized.
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let state = driver.state_mut();
        if self.config.track_locations {
            Source(self.input.into()).set(state);
        }
        TEXT_RULES.set(state);
        // elements with attributes are text for types that expect text,
        // text is an element for types that expect maps
        ContentKey(self.config.names.text_key).set(state);
        *state.get_mut::<Names>() = self.config.names.clone();
        Parser {
            input: self.input,
            config: &self.config,
            reader: NsReader::from_str(self.input),
            stack: Vec::new(),
            root_done: false,
            root: None,
            declarations: None,
        }
        .run(driver)
        .map_err(|mut err| {
            err.resolve_position(self.input.as_bytes());
            err
        })
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

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        driver.state_mut().set_default_context(self.context.clone());
        Deserializer::drive(self, driver)
    }
}

/// How the text of XML is interpreted.
///
/// Booleans are `true` and `false` (XML Schema also allows `1` and `0`,
/// which are integers here), an empty element is a missing value for types
/// that do not accept it (`<age/>` is `None` for an `Option<u32>`).
const TEXT_RULES: LexicalRules = {
    let mut rules = LexicalRules::STRICT;
    rules.set_empty_is_null(true);
    rules
};

/// A byte range in the input.
type Range = (usize, usize);

/// The text of an element that was not passed on yet.
enum PendingText<'a> {
    None,
    Borrowed(&'a str, Range),
    Owned(String, Range),
}

impl<'a> PendingText<'a> {
    fn push(&mut self, text: Cow<'a, str>, range: Range) {
        *self = match (std::mem::replace(self, PendingText::None), text) {
            (PendingText::None, Cow::Borrowed(text)) => PendingText::Borrowed(text, range),
            (PendingText::None, Cow::Owned(text)) => PendingText::Owned(text, range),
            (PendingText::Borrowed(prev, (start, _)), text) => {
                PendingText::Owned(prev.to_string() + &text, (start, range.1))
            }
            (PendingText::Owned(mut prev, (start, _)), text) => {
                prev.push_str(&text);
                PendingText::Owned(prev, (start, range.1))
            }
        };
    }

    fn is_blank(&self) -> bool {
        match self {
            PendingText::None => true,
            PendingText::Borrowed(text, _) => is_blank(text),
            PendingText::Owned(text, _) => is_blank(text),
        }
    }

    /// Emits the text as lexical atom.
    fn emit(self, driver: &mut DeserializeDriver<'_, 'a>, fallback: Range) -> Result<(), Error> {
        match self {
            PendingText::None => emit_at(driver, Atom::Lexical(Text::borrowed("")), fallback),
            PendingText::Borrowed(text, range) => {
                driver.state_mut().set_input_range(range.0, range.1);
                driver.emit_borrowed(Atom::Lexical(Text::borrowed(text)))
            }
            PendingText::Owned(text, range) => {
                emit_at(driver, Atom::Lexical(Text::owned(text)), range)
            }
        }
    }
}

fn is_blank(text: &str) -> bool {
    text.bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

/// An open element.
struct Element<'a> {
    /// `true` once the element was passed on as map.
    is_map: bool,
    text: PendingText<'a>,
    /// The range of the start tag.
    start: Range,
}

struct Parser<'a, 'c> {
    input: &'a str,
    config: &'c DeserializerConfig,
    reader: NsReader<&'a [u8]>,
    stack: Vec<Element<'a>>,
    root_done: bool,
    /// The name and the namespaces of the root element until they are
    /// attached to its first event.
    root: Option<RootData>,
    /// The namespaces declared on the last element that was started until
    /// they are attached to its first event.
    declarations: Option<Vec<(String, String)>>,
}

impl<'a> Parser<'a, '_> {
    fn run(mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        loop {
            let start = self.position();
            let event = self
                .reader
                .read_event()
                .map_err(|err| xml_error(err, self.reader.error_position() as usize))?;
            let range = (start, self.position());
            match event {
                XmlEvent::Start(ref tag) => self.start(driver, tag, range)?,
                XmlEvent::Empty(ref tag) => {
                    self.start(driver, tag, range)?;
                    self.end(driver, range)?;
                }
                XmlEvent::End(_) => self.end(driver, range)?,
                XmlEvent::Text(text) => {
                    self.text(text.xml_content(XmlVersion::Implicit1_0), range)?
                }
                XmlEvent::CData(text) => {
                    self.text(text.xml_content(XmlVersion::Implicit1_0), range)?
                }
                XmlEvent::GeneralRef(reference) => {
                    let text = resolve_reference(&reference, start)?;
                    self.text(Cow::Owned(text.to_string()), range)?
                }
                XmlEvent::Decl(_)
                | XmlEvent::PI(_)
                | XmlEvent::Comment(_)
                | XmlEvent::DocType(_) => {}
                XmlEvent::Eof => {
                    if !self.stack.is_empty() {
                        return Err(Error::with_offset(
                            ErrorKind::EndOfFile,
                            "unexpected end of input, an element is not closed",
                            start,
                        ));
                    }
                    if !self.root_done {
                        return Err(Error::with_offset(
                            ErrorKind::EndOfFile,
                            "no root element",
                            start,
                        ));
                    }
                    return Ok(());
                }
            }
        }
    }

    fn position(&self) -> usize {
        self.reader.buffer_position() as usize
    }

    /// Passes on the element on top of the stack as map if it's not yet.
    fn make_map(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if !self.stack.last().unwrap().is_map {
            self.attach_element(driver);
            let element = self.stack.last_mut().unwrap();
            element.is_map = true;
            emit_at(
                driver,
                Event::MapStart({
                    let mut shape = ContainerShape::with_order(Order::Significant);
                    shape.set_multimap(true);
                    shape
                }),
                element.start,
            )?;
        }
        self.flush_text(driver)
    }

    /// Passes on the text of the element on top of the stack as entry.
    fn flush_text(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        let element = self.stack.last_mut().unwrap();
        // whitespace between elements is not text, unless the content is
        // mixed
        if matches!(element.text, PendingText::None)
            || (element.text.is_blank() && !WhitespaceDepths::applies(driver.state()))
        {
            element.text = PendingText::None;
            return Ok(());
        }
        let text = std::mem::replace(&mut element.text, PendingText::None);
        let range = match text {
            PendingText::Borrowed(_, range) | PendingText::Owned(_, range) => range,
            PendingText::None => unreachable!(),
        };
        emit_at(
            driver,
            Atom::Lexical(Text::borrowed(self.config.names.text_key)),
            range,
        )?;
        text.emit(driver, range)
    }

    fn start(
        &mut self,
        driver: &mut DeserializeDriver<'_, 'a>,
        tag: &BytesStart<'a>,
        range: Range,
    ) -> Result<(), Error> {
        if self.stack.is_empty() {
            if self.root_done {
                return Err(Error::with_offset(
                    ErrorKind::Syntax,
                    "more than one root element",
                    range.0,
                ));
            }
            self.root = Some(self.root_data(tag, range.0)?);
        } else {
            self.make_map(driver)?;
            let name = self.name(tag.name(), false, range.0)?;
            emit_key(driver, name, range)?;
            // most elements declare nothing, the attributes are only
            // looked at if they might
            if tag.attributes_raw().contains("xmlns") {
                let declarations = self.declarations(tag, false, range.0)?;
                if !declarations.is_empty() {
                    self.declarations = Some(declarations);
                }
            }
        }
        self.stack.push(Element {
            is_map: false,
            text: PendingText::None,
            start: range,
        });

        for attr in tag.attributes() {
            let attr = attr.map_err(|err| {
                Error::with_offset(
                    ErrorKind::Syntax,
                    format!("invalid attribute: {err}"),
                    range.0,
                )
            })?;
            let raw = attr.key.as_ref();
            // namespace declarations are not data
            if raw == "xmlns" || raw.starts_with("xmlns:") {
                continue;
            }
            self.make_map(driver)?;
            let name = self.name(attr.key, true, range.0)?;
            emit_key(driver, name, range)?;
            let value = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|err| xml_error(err, range.0))?;
            match reborrow(self.input, &value) {
                Some(value) => {
                    driver.state_mut().set_input_range(range.0, range.1);
                    driver.emit_borrowed(Atom::Lexical(Text::borrowed(value)))?;
                }
                None => emit_at(driver, Atom::Lexical(Text::borrowed(&value)), range)?,
            }
        }
        Ok(())
    }

    /// Returns the name of the root element and the namespaces declared
    /// on it.
    ///
    /// Namespaces with a configured prefix are declared with it as that's
    /// how names in them are passed on.
    fn root_data(&self, tag: &BytesStart<'a>, offset: usize) -> Result<RootData, Error> {
        Ok(RootData {
            name: Some(self.name(tag.name(), false, offset)?.into_owned()),
            namespaces: self.declarations(tag, true, offset)?,
        })
    }

    /// Returns the namespaces declared on an element.
    ///
    /// Namespaces with a configured prefix are declared with it as that's
    /// how names in them are passed on.  Undeclaring the default namespace
    /// (`xmlns=""`) is a declaration with an empty URI, except on the root
    /// where there is nothing to undeclare.
    fn declarations(
        &self,
        tag: &BytesStart<'a>,
        is_root: bool,
        offset: usize,
    ) -> Result<Vec<(String, String)>, Error> {
        let mut namespaces: Vec<(String, String)> = Vec::new();
        // invalid attributes are reported when the attributes are passed on
        for attr in tag.attributes().flatten() {
            let raw: &str = attr.key.as_ref();
            let prefix = match raw.strip_prefix("xmlns") {
                Some("") => "",
                Some(rest) => match rest.strip_prefix(':') {
                    Some(prefix) => prefix,
                    None => continue,
                },
                None => continue,
            };
            let uri = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|err| xml_error(err, offset))?;
            if uri.is_empty() && (is_root || !prefix.is_empty()) {
                continue;
            }
            let prefix = match self.config.names.namespaces.iter().find(|(_, x)| *x == uri) {
                Some((alias, _)) => alias,
                None => prefix,
            };
            if !namespaces.iter().any(|(x, _)| x == prefix) {
                namespaces.push((prefix.to_string(), uri.into_owned()));
            }
        }
        Ok(namespaces)
    }

    /// Attaches the name and the namespaces of the root element or the
    /// namespaces declared on another element to its first event.
    ///
    /// The first event of an element comes before the events of the
    /// elements in it, so the element they are for is the last one that was
    /// started.
    fn attach_element(&mut self, driver: &mut DeserializeDriver<'_, 'a>) {
        if let Some(root) = self.root.take() {
            *driver.state_mut().event_mut::<RootData>() = root;
        }
        if let Some(declarations) = self.declarations.take() {
            driver.state_mut().event_mut::<Declarations>().0 = declarations;
        }
    }

    fn end(&mut self, driver: &mut DeserializeDriver<'_, 'a>, range: Range) -> Result<(), Error> {
        if self.stack.last().unwrap().is_map {
            self.flush_text(driver)?;
            self.stack.pop();
            emit_at(driver, Event::MapEnd, range)?;
            WhitespaceDepths::prune(driver.state_mut());
        } else {
            self.attach_element(driver);
            let element = self.stack.pop().unwrap();
            element.text.emit(driver, element.start)?;
        }
        if self.stack.is_empty() {
            self.root_done = true;
        }
        Ok(())
    }

    fn text(&mut self, text: Cow<'a, str>, range: Range) -> Result<(), Error> {
        match self.stack.last_mut() {
            Some(element) => {
                element.text.push(text, range);
                Ok(())
            }
            None if is_blank(&text) => Ok(()),
            None => Err(Error::with_offset(
                ErrorKind::Syntax,
                "text outside of the root element",
                range.0,
            )),
        }
    }

    /// Returns the key of an element or attribute.
    fn name(
        &self,
        name: QName<'_>,
        is_attribute: bool,
        offset: usize,
    ) -> Result<Cow<'a, str>, Error> {
        let names = &self.config.names;
        let written: &str = name.as_ref();
        let local_name = name.local_name();
        let local: &str = local_name.as_ref();
        let mut key = match self.namespace(name, is_attribute) {
            Namespace::Alias("") => Cow::Owned(local.to_string()),
            Namespace::Alias(alias) => Cow::Owned(format!("{alias}:{local}")),
            Namespace::Uri(uri) => Cow::Owned(format!("{{{uri}}}{local}")),
            Namespace::Unknown if self.config.resolve_namespaces => {
                return Err(Error::with_offset(
                    ErrorKind::Syntax,
                    format!("the prefix of `{written}` is not declared"),
                    offset,
                ));
            }
            Namespace::Written | Namespace::Unknown => match reborrow(self.input, written) {
                Some(written) => Cow::Borrowed(written),
                None => Cow::Owned(written.to_string()),
            },
        };
        if is_attribute && !names.attribute_prefix.is_empty() {
            key = Cow::Owned(format!("{}{}", names.attribute_prefix, key));
        }
        Ok(key)
    }

    /// Returns how the namespace of a name is written.
    fn namespace(&self, name: QName<'_>, is_attribute: bool) -> Namespace {
        let namespaces = self.config.names.namespaces;
        let resolve = self.config.resolve_namespaces;
        if namespaces.is_empty() && !resolve {
            return Namespace::Written;
        }
        let resolver = self.reader.resolver();
        let (ns, _) = if is_attribute {
            resolver.resolve_attribute(name)
        } else {
            resolver.resolve_element(name)
        };
        match ns {
            ResolveResult::Bound(ns) => {
                let uri: &str = ns.as_ref();
                if let Some((alias, _)) = namespaces.iter().find(|(_, x)| *x == uri) {
                    Namespace::Alias(alias)
                } else if !resolve {
                    Namespace::Written
                } else if uri == XML_NAMESPACE {
                    Namespace::Alias("xml")
                } else {
                    Namespace::Uri(uri.to_string())
                }
            }
            ResolveResult::Unbound => Namespace::Written,
            ResolveResult::Unknown(_) => Namespace::Unknown,
        }
    }
}

/// The namespace of the `xml` prefix.
pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// How the namespace of a name is written.
enum Namespace {
    /// The name is passed on as written.
    Written,
    /// The name has the configured prefix.
    Alias(&'static str),
    /// The name is `{uri}local`.
    Uri(String),
    /// The prefix is not declared.
    Unknown,
}

/// Returns the text as a slice of the input if it is one.
fn reborrow<'a>(input: &'a str, text: &str) -> Option<&'a str> {
    let start = (text.as_ptr() as usize).checked_sub(input.as_ptr() as usize)?;
    let end = start.checked_add(text.len())?;
    input
        .get(start..end)
        .filter(|x| x.as_ptr() == text.as_ptr())
}

/// Resolves a character or entity reference.
///
/// Only the predefined entities are supported, the entities of document
/// types are never expanded.
fn resolve_reference(reference: &BytesRef<'_>, offset: usize) -> Result<char, Error> {
    if let Some(c) = reference
        .resolve_char_ref()
        .map_err(|err| xml_error(err, offset))?
    {
        return Ok(c);
    }
    match &*reference.xml_content(XmlVersion::Implicit1_0) {
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "amp" => Ok('&'),
        "apos" => Ok('\''),
        "quot" => Ok('"'),
        name => Err(Error::with_offset(
            ErrorKind::Syntax,
            format!("unknown entity `&{name};`"),
            offset,
        )),
    }
}

fn xml_error(err: quick_xml::Error, offset: usize) -> Error {
    Error::with_offset(ErrorKind::Syntax, format!("invalid XML: {err}"), offset)
}

fn emit_key<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    key: Cow<'a, str>,
    range: Range,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(range.0, range.1);
    match key {
        Cow::Borrowed(key) => driver.emit_borrowed(Atom::Lexical(Text::borrowed(key))),
        Cow::Owned(key) => driver.emit(Atom::Lexical(Text::owned(key))),
    }
}

fn emit_at<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver<'_, '_>,
    event: E,
    range: Range,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(range.0, range.1);
    driver.emit(event)
}
