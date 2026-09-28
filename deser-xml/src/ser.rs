use std::borrow::Cow;
use std::fmt::Write as _;

use deser_core::adapters::BytesFormat;
use deser_core::ext::Number;
use deser_core::ser::{Describe, SerializeDriver};
use deser_core::{Atom, Error, ErrorKind, Event, Serialize};

use crate::Names;
use crate::de::XML_NAMESPACE;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    names: Names,
    root: Option<&'static str>,
    declaration: bool,
    bytes: BytesFormat,
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
    /// const CONFIG: SerializerConfig =
    ///     SerializerConfig::new().namespaces(deser_xml::prefixes![atom as "", dc]);
    /// let feed = Feed {
    ///     title: "x".into(),
    ///     creator: vec!["y".into(), "z".into()],
    ///     thumbnail: "t.png".into(),
    /// };
    /// assert_eq!(
    ///     CONFIG.to_string(&feed).unwrap(),
    ///     "<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
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
        let mut writer = Writer::new(self, None);
        driver.drive_described(|event, value, _state| writer.event(event, value))?;
        writer.finish()
    }

    /// Serializes a value and passes the output on in pieces.
    ///
    /// A piece is passed on as soon as it's final and at least `threshold`
    /// bytes long, the rest at the end.  This is how output is streamed
    /// (there is no public API for it yet).
    #[allow(dead_code)]
    pub(crate) fn to_pieces(
        &self,
        value: &dyn Serialize,
        threshold: usize,
        write: &mut dyn FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut driver = SerializeDriver::new(value);
        let mut writer = Writer::new(self, Some(Sink { write, threshold }));
        driver.drive_described(|event, value, _state| writer.event(event, value))?;
        let rest = writer.finish()?;
        if !rest.is_empty() {
            write(&rest)?;
        }
        Ok(())
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
    },
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
    /// The number of local namespace bindings outside of the element.
    bindings: usize,
}

/// Which attributes an element can still get.
///
/// The output from the start tag of the first element that can still get
/// attributes on is not final and cannot be passed on.
enum Attrs {
    /// Any, the keys of the map are not known.
    Unknown,
    /// Those among the fields of a struct: `names[last]` is the last
    /// attribute and the next key is one of `names[cursor..]`.
    Until {
        names: &'static [&'static str],
        cursor: usize,
        last: usize,
    },
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

/// Where final output goes.
struct Sink<'o> {
    write: &'o mut dyn FnMut(&str) -> Result<(), Error>,
    threshold: usize,
}

struct Writer<'c, 'o> {
    config: &'c SerializerConfig,
    /// The output that was not passed on yet, it starts at `base` in the
    /// document.
    out: String,
    base: usize,
    sink: Option<Sink<'o>>,
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

impl<'c, 'o> Writer<'c, 'o> {
    fn new(config: &'c SerializerConfig, sink: Option<Sink<'o>>) -> Writer<'c, 'o> {
        let mut out = String::new();
        if config.declaration {
            out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        }
        Writer {
            config,
            out,
            base: 0,
            sink,
            stack: Vec::new(),
            key: None,
            root_bindings: std::iter::once(("xml", XML_NAMESPACE))
                .chain(config.names.namespaces.iter().copied())
                .map(|(prefix, uri)| (prefix.to_string(), uri.to_string()))
                .collect(),
            root_declared: false,
            root_declarations: None,
            local_bindings: Vec::new(),
        }
    }

    fn event(&mut self, event: Event<'_>, value: &dyn Serialize) -> Result<(), Error> {
        match self.stack.last() {
            None => self.root(event, value)?,
            Some(Frame::Items { .. }) => self.item(event, value)?,
            Some(Frame::Element(_)) => match self.key.take() {
                None => self.key(event)?,
                Some(key) => self.entry(key, event, value)?,
            },
        }
        self.flush()
    }

    fn root(&mut self, event: Event<'_>, value: &dyn Serialize) -> Result<(), Error> {
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
            Event::MapStart(_) => self.open_element(&name, value),
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
        if let Attrs::Until {
            names,
            cursor,
            last,
        } = &mut element.attrs
        {
            match names[*cursor..].iter().position(|name| *name == key) {
                Some(offset) => {
                    *cursor += offset + 1;
                    is_last = *cursor == *last + 1;
                    if *cursor > *last + 1 {
                        element.attrs = Attrs::Done;
                    }
                }
                // not a field, all bets are off
                None => element.attrs = Attrs::Unknown,
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

    fn entry(&mut self, key: Key, event: Event<'_>, value: &dyn Serialize) -> Result<(), Error> {
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
                    escape(&text, false, &mut self.out)?;
                }
                Ok(())
            }
            (Key::Element(name), Event::Atom(atom)) => self.atom_element(&name, &atom, false),
            (Key::Element(name), Event::MapStart(_)) => {
                self.close_start_tag();
                self.open_element(&name, value)
            }
            (Key::Element(name), Event::SeqStart(_)) => {
                self.stack.push(Frame::Items { name });
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

    fn item(&mut self, event: Event<'_>, value: &dyn Serialize) -> Result<(), Error> {
        let Some(Frame::Items { name }) = self.stack.last() else {
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
                self.close_start_tag();
                let bindings = self.local_bindings.len();
                self.start_tag(&name, false)?;
                self.out.push_str("/>");
                self.local_bindings.truncate(bindings);
                Ok(())
            }
            Event::Atom(atom) => self.atom_element(&name, &atom, false),
            Event::MapStart(_) => {
                self.close_start_tag();
                self.open_element(&name, value)
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
        self.close_start_tag();
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
    fn open_element(&mut self, name: &str, value: &dyn Serialize) -> Result<(), Error> {
        let bindings = self.local_bindings.len();
        let name = self.start_tag(name, self.stack.is_empty())?;
        let attrs = self.attrs_of(value);
        self.stack.push(Frame::Element(Element {
            name,
            attrs_at: self.base + self.out.len(),
            content: false,
            late: String::new(),
            attrs,
            bindings,
        }));
        Ok(())
    }

    /// Returns which attributes the map of a value can have.
    fn attrs_of(&self, value: &dyn Serialize) -> Attrs {
        let mut fields = Fields::default();
        value.describe(&mut fields);
        let Some(names) = fields.names.filter(|_| !fields.variant) else {
            return Attrs::Unknown;
        };
        let names_config = &self.config.names;
        let last = names.iter().rposition(|name| {
            *name != names_config.text_key && attribute_name(names_config, name).is_some()
        });
        match last {
            Some(last) => Attrs::Until {
                names,
                cursor: 0,
                last,
            },
            None => Attrs::Done,
        }
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

    /// Passes the output that is final on to the sink.
    fn flush(&mut self) -> Result<(), Error> {
        let Some(threshold) = self.sink.as_ref().map(|sink| sink.threshold) else {
            return Ok(());
        };
        let mut until = self.final_until();
        if until - self.base < threshold.max(1) {
            return Ok(());
        }
        // the declarations of the root element are final with it
        if !self.root_declared && self.root_declarations.is_some_and(|at| at < until) {
            self.declare_root()?;
            until = self.final_until();
        }
        let len = until - self.base;
        (self.sink.as_mut().unwrap().write)(&self.out[..len])?;
        self.out.drain(..len);
        self.base = until;
        Ok(())
    }

    /// Returns the output that was not passed on.
    fn finish(mut self) -> Result<String, Error> {
        self.declare_root()?;
        Ok(self.out)
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
            Atom::F32(value) => Cow::Owned(float_text(value as f64)),
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

/// Writes a float like XML Schema (`INF`, `-INF` and `NaN`).
fn float_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value.is_infinite() {
        if value > 0.0 { "INF" } else { "-INF" }.into()
    } else {
        value.to_string()
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

    /// Serializes a value in pieces that are passed on as early as possible.
    fn pieces(config: &SerializerConfig, value: &dyn Serialize) -> Result<Vec<String>, Error> {
        let mut pieces = Vec::new();
        config.to_pieces(value, 0, &mut |piece| {
            pieces.push(piece.to_string());
            Ok(())
        })?;
        Ok(pieces)
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
                "<Feed",
                " id=\"1\"",
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
        let config = SerializerConfig::new().root("r");
        let map = BTreeMap::from([("$text", "x"), ("@a", "1"), ("b", "2")]);
        let values: [&dyn Serialize; 3] = [&feed(), &map, &Some(42)];
        for value in values {
            assert_eq!(
                pieces(&config, value).unwrap().concat(),
                config.to_string(value).unwrap()
            );
        }
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
