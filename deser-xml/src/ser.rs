use std::borrow::Cow;
use std::fmt::Write as _;

use deser_core::adapters::BytesFormat;
use deser_core::ext::Number;
use deser_core::ser::{Describe, SerializeDriver};
use deser_core::{Atom, Error, ErrorKind, Event, Serialize};

use crate::Names;

/// Configures how values are serialized to XML.
///
/// The value becomes the root element.  Its name is the configured
/// [`root`](Self::root) or the name of the struct (or enum) that is
/// serialized.  Maps are elements: keys with the
/// [attribute prefix](Self::attribute_prefix) are attributes, the
/// [text key](Self::text_key) is text and all other keys are child
/// elements.  Sequences are elements with the same name, one per value.
/// Null values are left out.
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
    /// The empty prefix declares the default namespace.
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
        let mut writer = Writer {
            config: self,
            out: String::new(),
            stack: Vec::new(),
            key: None,
        };
        if self.declaration {
            writer
                .out
                .push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        }
        driver.drive_described(|event, value, _state| writer.event(event, value))?;
        Ok(writer.out)
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
    /// An element whose content is a map.  `open` is `true` while
    /// attributes can still be added to the start tag.
    Element { name: String, open: bool },
    /// A sequence whose values are elements with the name.
    Items { name: String },
}

/// What a key of a map stands for.
enum Key {
    Attribute(String),
    Text,
    Element(String),
}

struct Writer<'c> {
    config: &'c SerializerConfig,
    out: String,
    stack: Vec<Frame>,
    /// The key of the next value of the element on top of the stack.
    key: Option<Key>,
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

impl Writer<'_> {
    fn event(&mut self, event: Event<'_>, value: &dyn Serialize) -> Result<(), Error> {
        match self.stack.last() {
            None => self.root(event, value),
            Some(Frame::Items { .. }) => self.item(event),
            Some(Frame::Element { .. }) => match self.key.take() {
                None => self.key(event),
                Some(key) => self.entry(key, event),
            },
        }
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
        check_name(&name)?;
        match event {
            Event::MapStart(_) => {
                self.out.push('<');
                self.out.push_str(&name);
                for (prefix, uri) in self.config.names.namespaces {
                    self.out.push_str(" xmlns");
                    if !prefix.is_empty() {
                        self.out.push(':');
                        self.out.push_str(prefix);
                    }
                    self.out.push_str("=\"");
                    escape(uri, true, &mut self.out)?;
                    self.out.push('"');
                }
                self.stack.push(Frame::Element { name, open: true });
                Ok(())
            }
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
        let names = &self.config.names;
        self.key = Some(if key == names.text_key {
            Key::Text
        } else if let Some(name) = key
            .strip_prefix(names.attribute_prefix)
            .filter(|_| !names.attribute_prefix.is_empty())
        {
            check_name(name)?;
            Key::Attribute(name.to_string())
        } else {
            check_name(&key)?;
            Key::Element(key)
        });
        Ok(())
    }

    fn entry(&mut self, key: Key, event: Event<'_>) -> Result<(), Error> {
        match (key, event) {
            (Key::Attribute(name), Event::Atom(atom)) => {
                let Some(text) = self.text(&atom)? else {
                    return Ok(());
                };
                match self.stack.last() {
                    Some(Frame::Element { open: true, .. }) => {}
                    _ => {
                        return Err(Error::new(
                            ErrorKind::Unexpected,
                            format!("attribute `{name}` comes after the content of the element"),
                        ));
                    }
                }
                let text = text.into_owned();
                self.out.push(' ');
                self.out.push_str(&name);
                self.out.push_str("=\"");
                escape(&text, true, &mut self.out)?;
                self.out.push('"');
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
                self.out.push('<');
                self.out.push_str(&name);
                self.stack.push(Frame::Element { name, open: true });
                Ok(())
            }
            (Key::Element(name), Event::SeqStart(_)) => {
                self.stack.push(Frame::Items { name });
                Ok(())
            }
            (Key::Attribute(name), _) => Err(Error::new(
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

    fn item(&mut self, event: Event<'_>) -> Result<(), Error> {
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
                write!(self.out, "<{name}/>").unwrap();
                Ok(())
            }
            Event::Atom(atom) => self.atom_element(&name, &atom, false),
            Event::MapStart(_) => {
                self.close_start_tag();
                self.out.push('<');
                self.out.push_str(&name);
                self.stack.push(Frame::Element { name, open: true });
                Ok(())
            }
            _ => Err(Error::new(
                ErrorKind::UnsupportedType,
                "sequences in sequences are not supported",
            )),
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
        self.out.push('<');
        self.out.push_str(name);
        if text.is_empty() {
            self.out.push_str("/>");
        } else {
            self.out.push('>');
            escape(&text, false, &mut self.out)?;
            write!(self.out, "</{name}>").unwrap();
        }
        Ok(())
    }

    /// Ends the start tag of the element that contains the next content.
    fn close_start_tag(&mut self) {
        let element = self.stack.iter_mut().rev().find_map(|frame| match frame {
            Frame::Element { open, .. } => Some(open),
            Frame::Items { .. } => None,
        });
        if let Some(open @ true) = element {
            *open = false;
            self.out.push('>');
        }
    }

    fn close_element(&mut self) -> Result<(), Error> {
        let Some(Frame::Element { name, open }) = self.stack.pop() else {
            unreachable!()
        };
        if open {
            self.out.push_str("/>");
        } else {
            write!(self.out, "</{name}>").unwrap();
        }
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
