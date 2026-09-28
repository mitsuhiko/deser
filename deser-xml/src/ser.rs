use std::borrow::Cow;
use std::fmt::Write as _;

use deser_core::__format::{Float, format_finite};
use deser_core::adapters::BytesFormat;
use deser_core::ext::Number;
use deser_core::hints::Layout;
use deser_core::ser::{Describe, PausableSink, SerializeDriver};
use deser_core::{Atom, Error, ErrorKind, Event, Serialize, State};

use crate::Names;
use crate::de::XML_NAMESPACE;
use crate::mixed::KeepsWhitespace;

/// How the output is indented.
///
/// See [`SerializerConfig::indent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Indent {
    /// No indentation, the document is written on a single line.
    #[default]
    None,
    /// Child elements on lines of their own, indented by the given number
    /// of spaces per level.
    Spaces(usize),
    /// Child elements on lines of their own, indented by a tab per level.
    Tab,
}

/// Configures how values are serialized to XML.
///
/// The value becomes the root element.  Its name is the configured
/// [`root`](Self::root) or the name of the struct (or enum) that is
/// serialized.  Maps are elements: keys with the
/// [attribute prefix](Self::attribute_prefix) are attributes, the
/// [text key](Self::text_key) is text and all other keys are child
/// elements.  Sequences are elements with the same name, one per value.
/// Null values are left out.  Attributes can come after other keys, they
/// are still written into the start tag.
///
/// Names can be `{uri}local` (the notation of James Clark, attributes are
/// `@{uri}local`, see [`qname!`](crate::qname)): their namespace gets the
/// [configured prefix](Self::namespaces) or a generated one (`ns0`, ...).
/// Every namespace has one prefix in the document, all of them are
/// declared on the root element.  Other names are written as they are.
///
/// ```
/// #[derive(deser::Serialize)]
/// struct Link {
///     #[deser(rename = "@href")]
///     href: String,
///     #[deser(rename = "$text")]
///     title: String,
/// }
///
/// #[derive(deser::Serialize)]
/// #[deser(rename = "feed")]
/// struct Feed {
///     link: Vec<Link>,
///     updated: Option<String>,
/// }
///
/// let feed = Feed {
///     link: vec![Link { href: "/a".into(), title: "A & B".into() }],
///     updated: None,
/// };
/// assert_eq!(
///     deser_xml::to_string(&feed).unwrap(),
///     r#"<feed><link href="/a">A &amp; B</link></feed>"#
/// );
/// ```
///
/// By default the output is a single line, [`indent`](Self::indent) writes
/// child elements on lines of their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    names: Names,
    root: Option<&'static str>,
    declaration: bool,
    bytes: BytesFormat,
    indent: Indent,
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
            names: Names::new(),
            root: None,
            declaration: false,
            bytes: BytesFormat::BASE64,
            indent: Indent::None,
        }
    }

    /// Sets the name of the root element.
    ///
    /// By default it's the name of the struct or enum that is serialized,
    /// values without a name (like maps) need it.
    pub const fn root(mut self, name: &'static str) -> SerializerConfig {
        self.root = Some(name);
        self
    }

    /// Sets the prefix of the keys that are attributes (default `@`).
    pub const fn attribute_prefix(mut self, prefix: &'static str) -> SerializerConfig {
        self.names.attribute_prefix = prefix;
        self
    }

    /// Sets the key that is the text of an element (default `$text`).
    pub const fn text_key(mut self, key: &'static str) -> SerializerConfig {
        self.names.text_key = key;
        self
    }

    /// Sets the prefixes of namespaces that are declared on the root
    /// element.
    ///
    /// The empty prefix declares the default namespace.  Names that are
    /// `{uri}local` are written with these prefixes, attributes only with
    /// prefixes that are not empty.  Namespaces without prefix get
    /// generated ones.  The table can be written with
    /// [`prefixes!`](crate::prefixes).
    ///
    /// ```
    /// use deser_xml::SerializerConfig;
    ///
    /// deser_xml::namespace!(
    ///     atom = "http://www.w3.org/2005/Atom",
    ///     dc = "http://purl.org/dc/elements/1.1/",
    ///     media = "http://search.yahoo.com/mrss/",
    /// );
    ///
    /// #[derive(deser::Serialize)]
    /// #[deser(rename = atom!("feed"))]
    /// struct Feed {
    ///     #[deser(rename = atom!("title"))]
    ///     title: String,
    ///     #[deser(rename = dc!("creator"))]
    ///     creator: Vec<String>,
    ///     #[deser(rename = media!("thumbnail"))]
    ///     thumbnail: String,
    /// }
    ///
    /// const CONFIG: SerializerConfig = SerializerConfig::new()
    ///     .namespaces(deser_xml::prefixes![atom as "", dc]);
    /// let feed = Feed {
    ///     title: "x".into(),
    ///     creator: vec!["y".into(), "z".into()],
    ///     thumbnail: "t.png".into(),
    /// };
    /// assert_eq!(
    ///     CONFIG.to_string(&feed).unwrap(),
    ///     "<feed xmlns=\"http://www.w3.org/2005/Atom\" \
    ///      xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
    ///      xmlns:ns0=\"http://search.yahoo.com/mrss/\"><title>x</title>\
    ///      <dc:creator>y</dc:creator><dc:creator>z</dc:creator>\
    ///      <ns0:thumbnail>t.png</ns0:thumbnail></feed>"
    /// );
    /// ```
    pub const fn namespaces(
        mut self,
        namespaces: &'static [(&'static str, &'static str)],
    ) -> SerializerConfig {
        self.names.namespaces = namespaces;
        self
    }

    /// Sets if the XML declaration is written (default `false`).
    pub const fn declaration(mut self, yes: bool) -> SerializerConfig {
        self.declaration = yes;
        self
    }

    /// Sets how the output is indented.
    ///
    /// By default ([`Indent::None`]) the document is written on a single
    /// line.  Otherwise the child elements of an element are written on
    /// lines of their own, indented by their depth, and the end tag on a
    /// line of its own:
    ///
    /// ```
    /// use deser_xml::{Indent, SerializerConfig};
    ///
    /// #[derive(deser::Serialize)]
    /// #[deser(rename = "point")]
    /// struct Point {
    ///     #[deser(rename = "@id")]
    ///     id: u32,
    ///     x: i32,
    ///     y: i32,
    /// }
    ///
    /// const PRETTY: SerializerConfig =
    ///     SerializerConfig::new().indent(Indent::Spaces(2));
    /// assert_eq!(
    ///     PRETTY.to_string(&Point { id: 1, x: 3, y: 4 }).unwrap(),
    ///     "<point id=\"1\">\n  <x>3</x>\n  <y>4</y>\n</point>"
    /// );
    /// ```
    ///
    /// Unlike in JSON whitespace can be text in XML.  It is only added
    /// between tags where it's not text of the elements (the deserializer
    /// skips it), elements with text are written on a single line:
    ///
    /// * The text of elements is never changed, elements with text and
    ///   child elements (mixed content like `<p>x <b>y</b></p>`) are
    ///   written on a single line from the text on.  If the element is a
    ///   struct whose [text key](Self::text_key) field comes after the
    ///   child element, the element is written on a single line from the
    ///   start (unless it has [`Layout::Expanded`]).
    /// * [`Mixed`](crate::Mixed) keeps whitespace as text by default, its
    ///   content is written on a single line.
    /// * Elements and sequences with [`Layout::Compact`] (see
    ///   [`hints`](deser_core::hints)) are written on a single line, also
    ///   their content.
    ///
    /// With the [declaration](Self::declaration) the root element starts on
    /// a new line.  The output never ends with a line break.
    pub const fn indent(mut self, indent: Indent) -> SerializerConfig {
        self.indent = indent;
        self
    }

    /// Enables or disables pretty printing.
    ///
    /// This is the same as [`indent`](Self::indent), XML has no spaces
    /// after separators like JSON.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_xml::{Indent, SerializerConfig};
    ///
    /// let value = BTreeMap::from([("a", 1), ("b", 2)]);
    /// const PRETTY: SerializerConfig =
    ///     SerializerConfig::new().root("r").pretty(Indent::Tab);
    /// assert_eq!(
    ///     PRETTY.to_string(&value).unwrap(),
    ///     "<r>\n\t<a>1</a>\n\t<b>2</b>\n</r>"
    /// );
    /// ```
    pub const fn pretty(self, indent: Indent) -> SerializerConfig {
        self.indent(indent)
    }

    /// Sets how bytes are written (default base64).
    pub const fn bytes(mut self, format: BytesFormat) -> SerializerConfig {
        self.bytes = format;
        self
    }

    /// Serializes a value.
    pub fn to_string(&self, value: &dyn Serialize) -> Result<String, Error> {
        self.to_string_with(value, |_| {})
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_string_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<String, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        let mut writer = Writer::new(self);
        driver.drive_described(|event, value, state| writer.event(event, value, state))?;
        writer.finish()?;
        Ok(writer.out)
    }

    /// Serializes (a part of) the value of a driver and appends the output
    /// that is final.
    ///
    /// The progress of the value is kept in `value` (see
    /// `Encoder::encode_incremental`), `true` is returned once the value is
    /// complete.  The output of a value is final once its start tag is
    /// complete, which is the case once no more attributes can come for
    /// the element (see `final_until`).
    #[cfg_attr(not(feature = "io"), allow(dead_code))]
    pub(crate) fn serialize_part(
        &self,
        value: &mut Option<Box<Writer>>,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<bool, Error> {
        let mut writer = value.take().unwrap_or_else(|| Box::new(Writer::new(self)));
        // after an error the value is abandoned, its writer is dropped
        let done = if limit == usize::MAX {
            driver.drive_described(|event, value, state| writer.event(event, value, state))?;
            true
        } else {
            writer.limit = limit;
            driver.drive_until(&mut *writer)?
        };
        if let Some(err) = writer.error.take() {
            return Err(err);
        }
        if done {
            writer.finish()?;
            out.extend_from_slice(writer.out.as_bytes());
            return Ok(true);
        }
        writer.pass_on(out);
        *value = Some(writer);
        Ok(false)
    }
}

/// Serializes a value to XML with the default configuration.
///
/// See [`SerializerConfig`].
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// An element or sequence that is being written.
enum Frame {
    Element(Element),
    /// A sequence whose values are elements with the name.
    Items {
        name: String,
        /// If the elements are written on a single line.
        compact: bool,
        /// If an element was written.
        started: bool,
    },
}

/// How the content of an element is laid out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lines {
    /// Not decided yet, no child element was written.
    Pending,
    /// Child elements are on lines of their own.
    Indented,
    /// Everything is on a single line from here on.
    Inline,
}

/// An element whose content is a map.
struct Element {
    /// The name as written.
    name: String,
    /// Where attributes are added to the start tag (in the document).
    attrs_at: usize,
    /// If the start tag was ended because content was written.
    content: bool,
    /// Attributes that came after content, they are added to the start
    /// tag once no more can come.
    late: String,
    /// Which attributes can still come.
    attrs: Attrs,
    /// The fields of the struct and the index after the last key among
    /// them, `None` if the keys are not known.
    fields: Option<(&'static [&'static str], usize)>,
    /// The number of local namespace bindings outside of the element.
    bindings: usize,
    /// How the content is laid out.
    lines: Lines,
    /// If the layout is not predicted from the fields.
    expanded: bool,
}

/// Which attributes an element can still get.
///
/// The output from the start tag of the first element that can still get
/// attributes on is not final and cannot be passed on.
enum Attrs {
    /// Any, the keys of the map are not known.
    Unknown,
    /// Those among the fields of the struct (see `Element::fields`), the
    /// field with the index is the last attribute.
    Until(usize),
    /// None.
    Done,
}

/// What a key of a map stands for.
enum Key {
    /// An attribute, `last` if no other attribute can follow.
    Attribute {
        name: String,
        last: bool,
    },
    Text,
    Element(String),
}

/// Writes the events of a document.
pub(crate) struct Writer {
    config: SerializerConfig,
    /// The output that was not passed on yet, it starts at `base` in the
    /// document.
    out: String,
    base: usize,
    /// The driver is paused once this much output is final (see
    /// `PausableSink`).
    limit: usize,
    /// An error of `pause`, which cannot fail.
    error: Option<Error>,
    stack: Vec<Frame>,
    /// The key of the next value of the element on top of the stack.
    key: Option<Key>,
    /// The prefixes of the namespaces declared on the root element, the
    /// first one is `xml` which is never declared.
    root_bindings: Vec<(String, String)>,
    /// If the declarations on the root element were written.  After that,
    /// new namespaces are declared on the elements that use them.
    root_declared: bool,
    /// Where the declarations of the root element go (after its name).
    root_declarations: Option<usize>,
    /// The prefixes declared on the elements on the stack.
    local_bindings: Vec<(String, String)>,
    /// The number of elements on the stack.
    depth: usize,
}

/// Finds the name of a type.
#[derive(Default)]
struct TypeName(Option<String>);

impl Describe for TypeName {
    fn structure(&mut self, name: &str) {
        self.0.get_or_insert_with(|| name.to_string());
    }

    fn newtype(&mut self, name: &str) {
        self.0.get_or_insert_with(|| name.to_string());
    }

    fn tuple_struct(&mut self, name: &str) {
        self.0.get_or_insert_with(|| name.to_string());
    }

    fn unit_struct(&mut self, name: &str) {
        self.0.get_or_insert_with(|| name.to_string());
    }
}

/// Finds the fields of a struct that is a map.
///
/// Values that are variants are not the struct they describe last (for
/// instance the content of an externally tagged variant is in a map with
/// the name of the variant).
#[derive(Default)]
struct Fields {
    names: Option<&'static [&'static str]>,
    variant: bool,
}

impl Describe for Fields {
    fn structure(&mut self, _name: &str) {
        self.names = None;
    }

    fn fields(&mut self, names: &'static [&'static str]) {
        self.names = Some(names);
    }

    fn variant(&mut self, _variant: &deser_core::ser::Variant<'_>) {
        self.variant = true;
    }
}

impl PausableSink for Writer {
    const DESCRIBED: bool = true;

    fn event(
        &mut self,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &mut State,
    ) -> Result<(), Error> {
        Writer::event(self, event, value, state)
    }

    fn pause(&mut self) -> bool {
        // the declarations of the root element are only written once the
        // output is passed on, as namespaces found afterwards are declared
        // on the elements that use them
        if self.final_until() - self.base < self.limit {
            return false;
        }
        if let Err(err) = self.final_len() {
            // the error is returned once the driver stopped
            self.error = Some(err);
        }
        true
    }
}

impl Writer {
    fn new(config: &SerializerConfig) -> Writer {
        let mut out = String::new();
        if config.declaration {
            out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            if config.indent != Indent::None {
                out.push('\n');
            }
        }
        Writer {
            config: config.clone(),
            out,
            base: 0,
            limit: usize::MAX,
            error: None,
            stack: Vec::new(),
            key: None,
            root_bindings: std::iter::once(("xml", XML_NAMESPACE))
                .chain(config.names.namespaces.iter().copied())
                .map(|(prefix, uri)| (prefix.to_string(), uri.to_string()))
                .collect(),
            root_declared: false,
            root_declarations: None,
            local_bindings: Vec::new(),
            depth: 0,
        }
    }

    fn event(
        &mut self,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &State,
    ) -> Result<(), Error> {
        match self.stack.last_mut() {
            None => self.root(event, value, state)?,
            Some(Frame::Items { .. }) => self.item(event, value, state)?,
            Some(Frame::Element(element)) => match self.key.take() {
                None => {
                    // flattened mixed content marks the key of its first
                    // entry
                    if matches!(event, Event::Atom(_)) && keeps_whitespace(state) {
                        element.lines = Lines::Inline;
                    }
                    self.key(event)?
                }
                Some(key) => self.entry(key, event, value, state)?,
            },
        }
        Ok(())
    }

    fn root(
        &mut self,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &State,
    ) -> Result<(), Error> {
        let name = match self.config.root {
            Some(name) => name.to_string(),
            None => {
                let mut name = TypeName::default();
                value.describe(&mut name);
                name.0.ok_or_else(|| {
                    Error::new(
                        ErrorKind::UnsupportedType,
                        "the name of the root element is unknown (see SerializerConfig::root)",
                    )
                })?
            }
        };
        check_element_name(&name)?;
        match event {
            Event::MapStart(_) => self.open_element(&name, value, state),
            Event::Atom(atom) => self.atom_element(&name, &atom, true),
            _ => Err(Error::new(
                ErrorKind::UnsupportedType,
                "the root element must be a map or a single value",
            )),
        }
    }

    fn key(&mut self, event: Event<'_>) -> Result<(), Error> {
        let key = match event {
            Event::MapEnd => return self.close_element(),
            Event::Atom(ref atom) => match self.text(atom)? {
                Some(key) => key.into_owned(),
                None => return Err(unsupported_key()),
            },
            _ => return Err(unsupported_key()),
        };

        // keep track of the attributes that can still come
        let Some(Frame::Element(element)) = self.stack.last_mut() else {
            unreachable!()
        };
        let mut is_last = false;
        if let Some((names, cursor)) = &mut element.fields {
            match names[*cursor..].iter().position(|name| *name == key) {
                Some(offset) => {
                    *cursor += offset + 1;
                    if let Attrs::Until(last) = element.attrs {
                        is_last = *cursor == last + 1;
                        if *cursor > last + 1 {
                            element.attrs = Attrs::Done;
                        }
                    }
                }
                // not a field, all bets are off
                None => {
                    element.fields = None;
                    if matches!(element.attrs, Attrs::Until(_)) {
                        element.attrs = Attrs::Unknown;
                    }
                }
            }
        }

        let names = &self.config.names;
        self.key = Some(if key == names.text_key {
            Key::Text
        } else if let Some(name) = attribute_name(names, &key) {
            check_element_name(name)?;
            Key::Attribute {
                name: name.to_string(),
                last: is_last,
            }
        } else {
            check_element_name(&key)?;
            Key::Element(key)
        });
        self.settle();
        Ok(())
    }

    fn entry(
        &mut self,
        key: Key,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &State,
    ) -> Result<(), Error> {
        match (key, event) {
            (Key::Attribute { name, last }, Event::Atom(atom)) => {
                if let Some(text) = self.text(&atom)? {
                    let text = text.into_owned();
                    self.attribute(&name, &text)?;
                }
                if last && let Some(Frame::Element(element)) = self.stack.last_mut() {
                    element.attrs = Attrs::Done;
                    self.settle();
                }
                Ok(())
            }
            (Key::Text, Event::Atom(atom)) => {
                if let Some(text) = self.text(&atom)? {
                    let text = text.into_owned();
                    self.close_start_tag();
                    if !text.is_empty() {
                        // whitespace next to text would be text
                        let Some(Frame::Element(element)) = self.stack.last_mut() else {
                            unreachable!()
                        };
                        element.lines = Lines::Inline;
                    }
                    escape(&text, false, &mut self.out)?;
                }
                Ok(())
            }
            (Key::Element(name), Event::Atom(atom)) => self.atom_element(&name, &atom, false),
            (Key::Element(name), Event::MapStart(_)) => {
                self.before_child();
                self.open_element(&name, value, state)
            }
            (Key::Element(name), Event::SeqStart(_)) => {
                self.stack.push(Frame::Items {
                    name,
                    compact: Layout::of(state) == Layout::Compact,
                    started: false,
                });
                Ok(())
            }
            (Key::Attribute { name, .. }, _) => Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("attribute `{name}` must be a single value"),
            )),
            (Key::Text, _) => Err(Error::new(
                ErrorKind::UnsupportedType,
                "the text of an element must be a single value",
            )),
            (Key::Element(_), _) => unreachable!("ends are handled by the frames"),
        }
    }

    fn item(
        &mut self,
        event: Event<'_>,
        value: &dyn Serialize,
        state: &State,
    ) -> Result<(), Error> {
        let Some(Frame::Items { name, .. }) = self.stack.last() else {
            unreachable!()
        };
        let name = name.clone();
        match event {
            Event::SeqEnd => {
                self.stack.pop();
                Ok(())
            }
            // nulls keep their position
            Event::Atom(Atom::Null) => {
                self.before_child();
                let bindings = self.local_bindings.len();
                self.start_tag(&name, false)?;
                self.out.push_str("/>");
                self.local_bindings.truncate(bindings);
                Ok(())
            }
            Event::Atom(atom) => self.atom_element(&name, &atom, false),
            Event::MapStart(_) => {
                self.before_child();
                self.open_element(&name, value, state)
            }
            _ => Err(Error::new(
                ErrorKind::UnsupportedType,
                "sequences in sequences are not supported",
            )),
        }
    }

    /// Writes an attribute of the element on top of the stack.
    fn attribute(&mut self, name: &str, value: &str) -> Result<(), Error> {
        let mut declarations = String::new();
        let name = self.qualify(name, true, &mut declarations)?;
        let Some(Frame::Element(element)) = self.stack.last_mut() else {
            unreachable!()
        };
        let out = if !element.content {
            &mut self.out
        } else if element.attrs_at >= self.base {
            &mut element.late
        } else {
            return Err(Error::new(
                ErrorKind::Unexpected,
                format!(
                    "attribute `{name}` comes after the start tag of the element was written \
                     (the fields the value described are not the keys it has)"
                ),
            ));
        };
        out.push_str(&declarations);
        out.push(' ');
        out.push_str(&name);
        out.push_str("=\"");
        escape(value, true, out)?;
        out.push('"');
        if !element.content {
            element.attrs_at = self.base + self.out.len();
        }
        self.settle();
        Ok(())
    }

    /// Adds the late attributes of the element on top of the stack to its
    /// start tag once no more attributes can come.
    fn settle(&mut self) {
        let Some(Frame::Element(element)) = self.stack.last_mut() else {
            return;
        };
        if !matches!(element.attrs, Attrs::Done) || element.late.is_empty() {
            return;
        }
        let late = std::mem::take(&mut element.late);
        let at = element.attrs_at;
        self.insert(at, &late);
    }

    /// Inserts text into the output which was not passed on yet.
    ///
    /// Positions of attributes at or after the text move behind it.
    fn insert(&mut self, at: usize, text: &str) {
        self.out.insert_str(at - self.base, text);
        for frame in &mut self.stack {
            if let Frame::Element(element) = frame
                && element.attrs_at >= at
            {
                element.attrs_at += text.len();
            }
        }
    }

    /// Writes an element whose content is a single value.
    ///
    /// Nulls are left out unless they are the root.
    fn atom_element(&mut self, name: &str, atom: &Atom<'_>, is_root: bool) -> Result<(), Error> {
        let text = match self.text(atom)? {
            Some(text) => text.into_owned(),
            None if is_root => String::new(),
            None => return Ok(()),
        };
        self.before_child();
        let bindings = self.local_bindings.len();
        let name = self.start_tag(name, is_root)?;
        if text.is_empty() {
            self.out.push_str("/>");
        } else {
            self.out.push('>');
            escape(&text, false, &mut self.out)?;
            write!(self.out, "</{name}>").unwrap();
        }
        self.local_bindings.truncate(bindings);
        Ok(())
    }

    /// Writes the start tag of an element whose content is a map.
    fn open_element(
        &mut self,
        name: &str,
        value: &dyn Serialize,
        state: &State,
    ) -> Result<(), Error> {
        let bindings = self.local_bindings.len();
        let name = self.start_tag(name, self.stack.is_empty())?;
        let (fields, attrs) = self.fields_of(value);
        let layout = Layout::of(state);
        let lines = if self.config.indent == Indent::None
            || self.in_line()
            || layout == Layout::Compact
            || keeps_whitespace(state)
        {
            Lines::Inline
        } else {
            Lines::Pending
        };
        self.stack.push(Frame::Element(Element {
            name,
            attrs_at: self.base + self.out.len(),
            content: false,
            late: String::new(),
            attrs,
            fields: fields.map(|names| (names, 0)),
            bindings,
            lines,
            expanded: layout == Layout::Expanded,
        }));
        self.depth += 1;
        Ok(())
    }

    /// Returns the fields of the map of a value and which attributes it
    /// can have.
    fn fields_of(&self, value: &dyn Serialize) -> (Option<&'static [&'static str]>, Attrs) {
        let mut fields = Fields::default();
        value.describe(&mut fields);
        let Some(names) = fields.names.filter(|_| !fields.variant) else {
            return (None, Attrs::Unknown);
        };
        let names_config = &self.config.names;
        let last = names.iter().rposition(|name| {
            *name != names_config.text_key && attribute_name(names_config, name).is_some()
        });
        let attrs = match last {
            Some(last) => Attrs::Until(last),
            None => Attrs::Done,
        };
        (Some(names), attrs)
    }

    /// Returns `true` if the next element is written on a single line with
    /// its parent.
    fn in_line(&self) -> bool {
        match self.stack.last() {
            None => false,
            Some(Frame::Items { compact: true, .. }) => true,
            Some(Frame::Items { .. }) => match self.stack.iter().rev().nth(1) {
                Some(Frame::Element(element)) => element.lines == Lines::Inline,
                _ => unreachable!("sequences are in elements"),
            },
            Some(Frame::Element(element)) => element.lines == Lines::Inline,
        }
    }

    /// Ends the start tag of the parent of the next element and starts a
    /// new line for it if the content of the parent is indented.
    fn before_child(&mut self) {
        self.close_start_tag();
        if self.config.indent == Indent::None {
            return;
        }
        let text_key = self.config.names.text_key;
        let mut frames = self.stack.iter_mut().rev();
        let element = match frames.next() {
            None => return,
            Some(Frame::Items {
                compact, started, ..
            }) => {
                // compact sequences are on the line of their first element
                if std::mem::replace(started, true) && *compact {
                    return;
                }
                match frames.next() {
                    Some(Frame::Element(element)) => element,
                    _ => unreachable!("sequences are in elements"),
                }
            }
            Some(Frame::Element(element)) => element,
        };
        if element.lines == Lines::Pending {
            // text that can still come is next to the child elements,
            // structs whose text comes after them are on a single line
            let text_ahead = !element.expanded
                && element
                    .fields
                    .is_some_and(|(names, cursor)| names[cursor..].contains(&text_key));
            element.lines = if text_ahead {
                Lines::Inline
            } else {
                Lines::Indented
            };
        }
        if element.lines == Lines::Indented {
            self.newline(self.depth);
        }
    }

    /// Starts a new line indented for the depth.
    fn newline(&mut self, depth: usize) {
        self.out.push('\n');
        let (unit, count) = match self.config.indent {
            Indent::None => return,
            Indent::Spaces(width) => (' ', width * depth),
            Indent::Tab => ('\t', depth),
        };
        self.out.extend(std::iter::repeat_n(unit, count));
    }

    /// Writes the start of a start tag and returns the name as written.
    fn start_tag(&mut self, name: &str, is_root: bool) -> Result<String, Error> {
        let mut declarations = String::new();
        let name = self.qualify(name, false, &mut declarations)?;
        self.out.push('<');
        self.out.push_str(&name);
        if is_root {
            self.root_declarations = Some(self.base + self.out.len());
        }
        self.out.push_str(&declarations);
        Ok(name)
    }

    /// Returns how a name is written, `{uri}local` names get the prefix
    /// of their namespace.
    ///
    /// Namespaces without prefix are declared on the root element as long
    /// as its declarations were not written, afterwards on the element
    /// that is being written (the declaration is added to `declarations`).
    fn qualify(
        &mut self,
        name: &str,
        is_attribute: bool,
        declarations: &mut String,
    ) -> Result<String, Error> {
        let Some((uri, local)) = split_name(name)? else {
            return Ok(name.to_string());
        };
        // the default namespace (the empty prefix) does not apply to
        // attributes
        let usable = |(prefix, bound): &&(String, String)| {
            bound == uri && !(is_attribute && prefix.is_empty())
        };
        let bound = self
            .local_bindings
            .iter()
            .rev()
            .find(usable)
            .or_else(|| self.root_bindings.iter().find(usable));
        let prefix = match bound {
            Some((prefix, _)) => prefix,
            None => {
                // prefixes are never shadowed
                let prefix = (0..)
                    .map(|n| format!("ns{n}"))
                    .find(|prefix| {
                        self.root_bindings
                            .iter()
                            .chain(&self.local_bindings)
                            .all(|(x, _)| x != prefix)
                    })
                    .unwrap();
                let bindings = if self.root_declared {
                    declare(&prefix, uri, declarations)?;
                    &mut self.local_bindings
                } else {
                    &mut self.root_bindings
                };
                bindings.push((prefix, uri.to_string()));
                &bindings.last().unwrap().0
            }
        };
        Ok(if prefix.is_empty() {
            local.to_string()
        } else {
            format!("{prefix}:{local}")
        })
    }

    /// Writes the namespace declarations of the root element.
    fn declare_root(&mut self) -> Result<(), Error> {
        if self.root_declared {
            return Ok(());
        }
        self.root_declared = true;
        let Some(at) = self.root_declarations else {
            return Ok(());
        };
        let mut declarations = String::new();
        for (prefix, uri) in &self.root_bindings[1..] {
            declare(prefix, uri, &mut declarations)?;
        }
        self.insert(at, &declarations);
        Ok(())
    }

    /// Returns where the output that is not final starts.
    fn final_until(&self) -> usize {
        for frame in &self.stack {
            if let Frame::Element(element) = frame
                && !matches!(element.attrs, Attrs::Done)
            {
                return element.attrs_at;
            }
        }
        self.base + self.out.len()
    }

    /// Returns how much of the output that was not passed on is final.
    ///
    /// Once the start tag of the root element is final, its namespace
    /// declarations are written.  Namespaces that are found afterwards are
    /// declared on the elements that use them.
    fn final_len(&mut self) -> Result<usize, Error> {
        let mut until = self.final_until();
        // the declarations of the root element are final with it
        if !self.root_declared && self.root_declarations.is_some_and(|at| at < until) {
            self.declare_root()?;
            until = self.final_until();
        }
        Ok(until - self.base)
    }

    /// Passes the output that is final on.
    #[cfg_attr(not(feature = "io"), allow(dead_code))]
    fn pass_on(&mut self, out: &mut Vec<u8>) {
        let len = self.final_until() - self.base;
        out.extend_from_slice(&self.out.as_bytes()[..len]);
        self.out.drain(..len);
        self.base += len;
    }

    /// Completes the output once the document was written, the output
    /// that was not passed on is final afterwards.
    fn finish(&mut self) -> Result<(), Error> {
        if !self.stack.is_empty() || self.depth > 0 {
            return Err(Error::new(ErrorKind::Unexpected, "incomplete document"));
        }
        self.declare_root()
    }

    /// Ends the start tag of the element that contains the next content.
    fn close_start_tag(&mut self) {
        let element = self.stack.iter_mut().rev().find_map(|frame| match frame {
            Frame::Element(element) => Some(element),
            Frame::Items { .. } => None,
        });
        if let Some(element) = element
            && !element.content
        {
            element.content = true;
            self.out.push('>');
        }
    }

    fn close_element(&mut self) -> Result<(), Error> {
        if let Some(Frame::Element(element)) = self.stack.last_mut() {
            element.attrs = Attrs::Done;
        }
        self.settle();
        let Some(Frame::Element(element)) = self.stack.pop() else {
            unreachable!()
        };
        self.depth -= 1;
        if element.lines == Lines::Indented {
            self.newline(self.depth);
        }
        if element.content {
            write!(self.out, "</{}>", element.name).unwrap();
        } else {
            self.out.push_str("/>");
        }
        self.local_bindings.truncate(element.bindings);
        Ok(())
    }

    /// Returns the text of an atom, `None` for null.
    fn text<'a>(&self, atom: &'a Atom<'_>) -> Result<Option<Cow<'a, str>>, Error> {
        Ok(Some(match *atom {
            Atom::Null => return Ok(None),
            // implicit values keep their text if it's the same value in
            // XML Schema, which is the case for all but `~`, `.inf`, ...
            Atom::Implicit(ref value) => {
                return Ok(self
                    .text(&value.value().to_atom())?
                    .map(|text| Cow::Owned(text.into_owned())));
            }
            Atom::Bool(value) => Cow::Borrowed(if value { "true" } else { "false" }),
            Atom::Str(ref value) | Atom::Lexical(ref value) => Cow::Borrowed(&**value),
            Atom::Char(value) => Cow::Owned(value.to_string()),
            Atom::U64(value) => Cow::Owned(value.to_string()),
            Atom::I64(value) => Cow::Owned(value.to_string()),
            Atom::F32(value) => Cow::Owned(float_text(value)),
            Atom::F64(value) => Cow::Owned(float_text(value)),
            Atom::Bytes(ref bytes) => {
                let format = bytes.fallback.copied().unwrap_or(self.config.bytes);
                Cow::Owned(
                    format
                        .encode(bytes)
                        .or_else(|| BytesFormat::BASE64.encode(bytes))
                        .unwrap_or_default(),
                )
            }
            Atom::Ext(ref ext) => {
                if let Some(number) = ext.downcast_value_ref::<Number>() {
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
                                format!("XML does not support {}", ext.name()),
                            ));
                        }
                        fallback => match self.text(&fallback)? {
                            Some(text) => Cow::Owned(text.into_owned()),
                            None => return Ok(None),
                        },
                    }
                }
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("XML does not support {}", atom.name()),
                ));
            }
        }))
    }
}

/// Returns `true` if the event starts content that keeps whitespace.
fn keeps_whitespace(state: &State) -> bool {
    state
        .event::<KeepsWhitespace>()
        .is_some_and(|keeps| keeps.0)
}

/// Writes a float like XML Schema (`INF`, `-INF` and `NaN`).
/// Returns the text of a float.
///
/// Finite floats have the shortest text that reads back as the same value
/// of their type, like in the other formats.  The others are written as in
/// XML Schema.
fn float_text<F: Float>(value: F) -> String {
    let wide = value.to_f64();
    if wide.is_nan() {
        "NaN".into()
    } else if wide.is_infinite() {
        if wide > 0.0 { "INF" } else { "-INF" }.into()
    } else {
        format_finite(value)
    }
}

/// Escapes text or the value of an attribute.
fn escape(text: &str, attribute: bool, out: &mut String) -> Result<(), Error> {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            // attribute values are normalized, whitespace is kept with
            // references
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' | '\n' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("the character {c:?} cannot be written in XML"),
                ));
            }
            c => out.push(c),
        }
    }
    Ok(())
}

/// Returns the name of the attribute a key stands for.
///
/// The text key is checked before.
fn attribute_name<'a>(names: &Names, key: &'a str) -> Option<&'a str> {
    key.strip_prefix(names.attribute_prefix)
        .filter(|_| !names.attribute_prefix.is_empty())
}

/// Writes the declaration of a namespace prefix.
fn declare(prefix: &str, uri: &str, out: &mut String) -> Result<(), Error> {
    out.push_str(" xmlns");
    if !prefix.is_empty() {
        out.push(':');
        out.push_str(prefix);
    }
    out.push_str("=\"");
    escape(uri, true, out)?;
    out.push('"');
    Ok(())
}

/// Splits a `{uri}local` name, other names are `None`.
fn split_name(name: &str) -> Result<Option<(&str, &str)>, Error> {
    let Some(rest) = name.strip_prefix('{') else {
        return Ok(None);
    };
    match rest.split_once('}') {
        Some((uri, local)) if !uri.is_empty() => Ok(Some((uri, local))),
        _ => Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("`{name}` is not a name in XML"),
        )),
    }
}

/// Checks that a name is a name in XML or a `{uri}local` name whose local
/// name has no prefix.
fn check_element_name(name: &str) -> Result<(), Error> {
    match split_name(name)? {
        Some((_, local)) if local.contains(':') => Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("`{name}` is not a name in XML"),
        )),
        Some((_, local)) => check_name(local),
        None => check_name(name),
    }
}

/// Checks that a name is a name in XML.
fn check_name(name: &str) -> Result<(), Error> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' || c == ':' => {
            chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.' | '\u{b7}'))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("`{name}` is not a name in XML"),
        ))
    }
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "the keys of elements must be names",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use deser::Serialize;

    use super::*;

    /// Serializes a value in pieces that are passed on as early as possible
    /// (the driver pauses between values).
    fn pieces(config: &SerializerConfig, value: &dyn Serialize) -> Result<Vec<String>, Error> {
        let mut pieces = Vec::new();
        let mut driver = SerializeDriver::new(value);
        let mut progress = None;
        loop {
            let mut out = Vec::new();
            let done = config.serialize_part(&mut progress, &mut driver, &mut out, 1)?;
            pieces.push(String::from_utf8(out).unwrap());
            if done {
                return Ok(pieces);
            }
        }
    }

    #[derive(Serialize)]
    struct Feed {
        #[deser(rename = "@id")]
        id: u32,
        title: &'static str,
        entry: Vec<Entry>,
    }

    #[derive(Serialize)]
    struct Entry {
        #[deser(rename = "$text")]
        text: &'static str,
        #[deser(rename = "@n")]
        n: u32,
        #[deser(skip_serializing_if = Option::is_none)]
        note: Option<&'static str>,
    }

    fn feed() -> Feed {
        Feed {
            id: 1,
            title: "t",
            entry: vec![
                Entry {
                    text: "a",
                    n: 1,
                    note: None,
                },
                Entry {
                    text: "b",
                    n: 2,
                    note: Some("x"),
                },
            ],
        }
    }

    #[test]
    fn test_structs_stream() {
        // output is final once no more attributes can come, late
        // attributes hold back the rest of the element until they come
        assert_eq!(
            pieces(&SerializerConfig::new(), &feed()).unwrap(),
            [
                "<Feed id=\"1\"",
                "><title>t</title>",
                "<entry",
                " n=\"1\">a",
                "</entry>",
                "<entry",
                " n=\"2\">b",
                "<note>x</note>",
                "</entry>",
                "</Feed>",
            ]
        );
    }

    #[test]
    fn test_maps_are_buffered() {
        // the keys of maps are not known, their elements are final at the end
        let config = SerializerConfig::new().root("m").declaration(true);
        let map = BTreeMap::from([
            ("a", BTreeMap::from([("$text", "1"), ("@x", "2")])),
            ("b", BTreeMap::from([("$text", "3"), ("@y", "4")])),
        ]);
        assert_eq!(
            pieces(&config, &map).unwrap(),
            [
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><m",
                "><a x=\"2\">1</a><b y=\"4\">3</b></m>",
            ]
        );

        let map = BTreeMap::from([("$text", "x"), ("@a", "1"), ("b", "2")]);
        assert_eq!(
            pieces(&config, &map).unwrap(),
            [
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><m",
                " a=\"1\">x<b>2</b></m>",
            ]
        );
    }

    #[test]
    fn test_same_output() {
        let map = BTreeMap::from([("$text", "x"), ("@a", "1"), ("b", "2")]);
        let nested = BTreeMap::from([("a", BTreeMap::from([("@x", "1"), ("b", "2")]))]);
        let values: [&dyn Serialize; 4] = [&feed(), &map, &nested, &Some(42)];
        for config in [
            SerializerConfig::new().root("r"),
            SerializerConfig::new()
                .root("r")
                .declaration(true)
                .indent(Indent::Spaces(2)),
        ] {
            for value in values {
                assert_eq!(
                    pieces(&config, value).unwrap().concat(),
                    config.to_string(value).unwrap()
                );
            }
        }
    }

    #[test]
    fn test_indent_streams() {
        // indentation does not hold back output
        let config = SerializerConfig::new().indent(Indent::Spaces(2));
        assert_eq!(
            pieces(&config, &feed()).unwrap(),
            [
                "<Feed id=\"1\"",
                ">\n  <title>t</title>",
                "\n  <entry",
                " n=\"1\">a",
                "</entry>",
                "\n  <entry",
                " n=\"2\">b",
                "<note>x</note>",
                "</entry>",
                "\n</Feed>",
            ]
        );
    }

    #[test]
    fn test_namespaces_stream() {
        #[derive(Serialize)]
        #[deser(rename = "{urn:root}root")]
        struct Root {
            #[deser(rename = "{urn:a}a")]
            a: Vec<Child>,
        }

        #[derive(Serialize)]
        struct Child {
            #[deser(rename = "@{urn:b}b")]
            b: u32,
            #[deser(rename = "{urn:a}c")]
            c: u32,
        }

        let root = Root {
            a: vec![Child { b: 1, c: 2 }, Child { b: 3, c: 4 }],
        };

        // written at once, all namespaces are declared on the root
        let config = SerializerConfig::new();
        assert_eq!(
            config.to_string(&root).unwrap(),
            "<ns0:root xmlns:ns0=\"urn:root\" xmlns:ns1=\"urn:a\" xmlns:ns2=\"urn:b\">\
             <ns1:a ns2:b=\"1\"><ns1:c>2</ns1:c></ns1:a>\
             <ns1:a ns2:b=\"3\"><ns1:c>4</ns1:c></ns1:a></ns0:root>"
        );

        // streamed, the ones that are found after the start tag of the root
        // was written are declared where they are used
        assert_eq!(
            pieces(&config, &root).unwrap().concat(),
            "<ns0:root xmlns:ns0=\"urn:root\" xmlns:ns1=\"urn:a\">\
             <ns1:a xmlns:ns2=\"urn:b\" ns2:b=\"1\"><ns1:c>2</ns1:c></ns1:a>\
             <ns1:a xmlns:ns2=\"urn:b\" ns2:b=\"3\"><ns1:c>4</ns1:c></ns1:a>\
             </ns0:root>"
        );

        // configured namespaces are always declared on the root
        let config = SerializerConfig::new().namespaces(&[("r", "urn:root"), ("a", "urn:a")]);
        assert_eq!(
            pieces(&config, &root).unwrap().concat(),
            "<r:root xmlns:r=\"urn:root\" xmlns:a=\"urn:a\">\
             <a:a xmlns:ns0=\"urn:b\" ns0:b=\"1\"><a:c>2</a:c></a:a>\
             <a:a xmlns:ns0=\"urn:b\" ns0:b=\"3\"><a:c>4</a:c></a:a></r:root>"
        );
    }

    #[test]
    fn test_wrong_fields() {
        // a value whose keys are not the fields it describes
        struct Wrong(BTreeMap<&'static str, &'static str>);

        impl Serialize for Wrong {
            fn describe(&self, d: &mut dyn Describe) {
                d.structure("Wrong");
                d.fields(&["$text"]);
            }

            fn serialize(
                &self,
                state: &mut deser_core::State,
            ) -> Result<deser_core::ser::Chunk<'_>, Error> {
                self.0.serialize(state)
            }
        }

        let wrong = Wrong(BTreeMap::from([("$text", "x"), ("@a", "1")]));
        let config = SerializerConfig::new();
        // written at once, the attribute can still go into the start tag
        assert_eq!(
            config.to_string(&wrong).unwrap(),
            r#"<Wrong a="1">x</Wrong>"#
        );
        let err = pieces(&config, &wrong).unwrap_err();
        assert!(
            err.message()
                .starts_with("attribute `a` comes after the start tag")
        );
    }
}
