//! XML support for deser.
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Debug, Deserialize, Serialize, PartialEq)]
//! struct Link {
//!     #[deser(rename = "@href")]
//!     href: String,
//!     #[deser(rename = "@rel")]
//!     rel: Option<String>,
//! }
//!
//! #[derive(Debug, Deserialize, Serialize, PartialEq)]
//! #[deser(rename = "feed")]
//! struct Feed {
//!     title: String,
//!     link: Vec<Link>,
//!     count: u32,
//! }
//!
//! let feed: Feed = deser_xml::from_str(r#"
//!     <feed>
//!       <title>Example</title>
//!       <link href="/a"/>
//!       <count>3</count>
//!       <link href="/b" rel="self"/>
//!     </feed>
//! "#).unwrap();
//! assert_eq!(feed.title, "Example");
//! assert_eq!(feed.link.len(), 2);
//! assert_eq!(feed.link[1].rel.as_deref(), Some("self"));
//!
//! assert_eq!(
//!     deser_xml::to_string(&feed).unwrap(),
//!     "<feed><title>Example</title><link href=\"/a\"/>\
//!      <link href=\"/b\" rel=\"self\"/><count>3</count></feed>"
//! );
//! ```
//!
//! # Data Model
//!
//! The document is the value of its root element (the name of the root
//! element is not checked, see [`Root`]).  An element is:
//!
//! * **Its text** if it has neither attributes nor child elements:
//!   `<count>3</count>` is `"3"` and `<empty/>` is `""`.  Like the values of
//!   query strings, text is passed on as
//!   [lexical atom](deser_core::Atom::Lexical) that the type it's
//!   deserialized into parses.  An empty element is `None` for optionals of
//!   types that do not accept the empty text.
//! * **A map** otherwise.  Attributes are entries whose key has the
//!   [attribute prefix](DeserializerConfig::set_attribute_prefix) (`@href`),
//!   child elements are entries with their name and text is an entry with
//!   the [text key](DeserializerConfig::set_text_key) (`$text`).  Whitespace
//!   between child elements is not text.
//!
//! | XML                                   | deser                                   |
//! |---------------------------------------|-----------------------------------------|
//! | `<a>1</a>`                            | `"1"`                                   |
//! | `<a/>`                                | `""`                                    |
//! | `<a href="x"/>`                       | `{"@href": "x"}`                        |
//! | `<a href="x">y</a>`                   | `{"@href": "x", "$text": "y"}`          |
//! | `<a><b>1</b><c>2</c></a>`             | `{"b": "1", "c": "2"}`                  |
//! | `<a><b>1</b><c/><b>2</b></a>`         | `{"b": "1", "c": "", "b": "2"}`         |
//! | `<p>x <b>y</b> z</p>`                 | `{"$text": "x ", "b": "y", "$text": " z"}` |
//!
//! Maps are [multimaps](deser_core::ContainerShape::set_multimap) whose
//! order is [significant](deser_core::Order::Significant): an element can
//! have more than one child with the same name.  Fields and map values
//! that are collections (like `Vec<T>`) collect all of them, also if other
//! elements are between them.  A single child element is a collection of
//! one value and a missing one an empty collection.  For other types
//! the [`DuplicateKeys`](deser_core::de::DuplicateKeys) policy of the
//! [`Context`](deser_core::Context) decides (repeated elements are an error
//! by default).
//!
//! Whether an element is text or a map depends on the document, the type
//! it's deserialized into decides what it wants (the text key is the
//! [key of the content](deser_core::de::ContentKey)):
//!
//! * An element with attributes for a type that expects text (like a
//!   `u32`) is its text, the attributes are skipped:
//!   `<count unit="m">3</count>` is `3` for a `u32`.
//! * An element that is only text for a type that expects a map (like a
//!   struct) is a map with the text under the text key: `<price>3</price>`
//!   is `{"$text": "3"}` for a struct and an empty element an empty map.
//!
//! ```rust
//! #[derive(deser::Deserialize)]
//! struct Price {
//!     #[deser(rename = "@currency")]
//!     currency: Option<String>,
//!     #[deser(rename = "$text")]
//!     amount: f64,
//! }
//!
//! #[derive(deser::Deserialize)]
//! struct Item {
//!     price: Vec<Price>,
//!     weight: f64,
//! }
//!
//! let item: Item = deser_xml::from_str(r#"
//!     <item>
//!       <price currency="EUR">3</price>
//!       <price>4</price>
//!       <weight unit="kg">1.5</weight>
//!     </item>
//! "#).unwrap();
//! assert_eq!(item.price[0].currency.as_deref(), Some("EUR"));
//! assert_eq!(item.price[1].amount, 4.0);
//! assert_eq!(item.weight, 1.5);
//! ```
//!
//! The order of text and child elements is kept by [`Mixed`], which is
//! for mixed content (`<p>x <b>y</b> z</p>`): every text and child
//! element is a value, typically of an enum whose variants are named after
//! the elements:
//!
//! ```rust
//! use deser::Deserialize;
//! use deser_xml::Mixed;
//!
//! #[derive(Debug, Deserialize, PartialEq)]
//! enum Inline {
//!     #[deser(rename = "$text")]
//!     Text(String),
//!     #[deser(rename = "b")]
//!     Bold(String),
//! }
//!
//! #[derive(Deserialize)]
//! struct Paragraph {
//!     #[deser(rename = "@class")]
//!     class: Option<String>,
//!     #[deser(flatten)]
//!     content: Mixed<Inline>,
//! }
//!
//! let p: Paragraph =
//!     deser_xml::from_str(r#"<p class="x">x <b>y</b> z</p>"#).unwrap();
//! assert_eq!(p.content.0, [
//!     Inline::Text("x ".into()),
//!     Inline::Bold("y".into()),
//!     Inline::Text(" z".into()),
//! ]);
//! ```
//!
//! Enums work like elsewhere: an externally tagged enum is an element with
//! one child (`<shape><circle r="1"/></shape>`), unit variants are text,
//! internally tagged enums can use an attribute as tag
//! (`#[deser(tag = "@type")]`).
//!
//! Names are passed on as written (`atom:link`), namespace declarations
//! (`xmlns` attributes) are not data.  The name of the root element and
//! the namespaces declared on it are not part of the value either, they are
//! captured by [`Root`].  Values that capture event data (such as
//! [`Recording`](deser_core::de::Recording)) keep the root element and the
//! namespace declarations of all elements, so they are written again where
//! they were.  Namespaces can be given prefixes
//! that are used regardless of the prefixes of the document (see
//! [`DeserializerConfig::namespaces`]) or be
//! [resolved](DeserializerConfig::set_resolve_namespaces) into names like
//! `{http://www.w3.org/2005/Atom}title` (see [`qname!`] and
//! [`namespace!`]), which the serializer writes with prefixes.
//! CDATA sections are text, the
//! predefined entities (`&amp;`, ...) and character references are
//! resolved.  Entities of document types are never expanded.
//!
//! Serializing works the other way around (see [`SerializerConfig`]).
//!
//! # Pretty Printing
//!
//! By default the output is a single line.  [`SerializerConfig::set_pretty`]
//! (or [`set_indent`](SerializerConfig::set_indent)) writes child elements on
//! lines of their own, but only where the whitespace is not text: the text
//! of elements is kept as it is and mixed content stays on a single line.
//!
//! ```rust
//! use deser_xml::{Indent, SerializerConfig};
//!
//! #[derive(deser::Serialize)]
//! #[deser(rename = "item")]
//! struct Item {
//!     name: &'static str,
//!     tag: Vec<&'static str>,
//! }
//!
//! const PRETTY: SerializerConfig =
//!     SerializerConfig::builder().pretty(Indent::Spaces(2)).build();
//! let item = Item { name: "x", tag: vec!["a", "b"] };
//! assert_eq!(
//!     PRETTY.to_string(&item).unwrap(),
//!     "<item>\n  <name>x</name>\n  <tag>a</tag>\n  <tag>b</tag>\n</item>"
//! );
//! ```
//!
//! # Streams
//!
//! Documents are read from a [`Read`](std::io::Read) with
//! [`from_reader`] and written to a [`Write`](std::io::Write) with
//! [`to_writer`].  The configurations also create readers and writers of
//! [`deser::io`](deser_core::io) ([`DeserializerConfig::reader`] and
//! [`SerializerConfig::writer`]), the stream serializer ([`Serializer`])
//! and deserializer ([`StreamDeserializer`]) work with other kinds of IO
//! too (for instance async runtimes with `deser-tokio`).  A stream holds a
//! single document.  The reader is read to
//! the end before the document is parsed.  The output is written while the
//! value is serialized: an element is only held back until no more
//! attributes can come for it, which for structs is known from their fields
//! and for maps is their end.
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! #[derive(deser::Serialize, deser::Deserialize)]
//! #[deser(rename = "feed")]
//! struct Feed {
//!     entry: Vec<String>,
//! }
//!
//! let feed = Feed { entry: (0..100).map(|x| x.to_string()).collect() };
//! let mut out = Vec::new();
//! deser_xml::to_writer(&mut out, &feed).unwrap();
//! let read: Feed = deser_xml::from_reader(&out[..]).unwrap();
//! assert_eq!(read.entry.len(), 100);
//! # }
//! ```
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library, see [streams](#streams).
//! * `speedups` (enabled by default): formats floats with
//!   [`zmij`](https://docs.rs/zmij), which is faster and makes binaries
//!   smaller.  Without it floats are formatted with the same text by a
//!   fallback on top of the float formatting of `core`.
//!
//! # Limitations
//!
//! This crate is an early version.  The input has to be UTF-8.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![deny(missing_docs)]

mod de;
mod mixed;
mod num;
mod root;
mod ser;
mod stream;

pub use self::de::{
    Deserializer, DeserializerConfig, DeserializerConfigBuilder, from_slice, from_str,
};
pub use self::mixed::{KeepWhitespace, Mixed, SkipWhitespace, Whitespace};
pub use self::root::Root;
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Indent, Serializer, SerializerConfig, SerializerConfigBuilder, to_string};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;

/// Writes a name in a namespace as `{uri}local`.
///
/// This is the notation for names in namespaces (see
/// [`DeserializerConfig::set_resolve_namespaces`]).  With `@` in front it's
/// the name of an attribute with the default
/// [attribute prefix](DeserializerConfig::set_attribute_prefix).  The name is
/// a literal, so it can be used for `rename`:
///
/// ```
/// use deser_xml::qname;
///
/// #[derive(deser::Deserialize)]
/// struct Link {
///     #[deser(rename = qname!(@ "http://www.w3.org/1999/xlink", "href"))]
///     href: String,
/// }
///
/// assert_eq!(qname!("urn:x", "a"), "{urn:x}a");
/// assert_eq!(qname!(@ "urn:x", "a"), "@{urn:x}a");
/// ```
#[macro_export]
macro_rules! qname {
    (@ $uri:literal, $local:literal) => {
        concat!("@{", $uri, "}", $local)
    };
    ($uri:literal, $local:literal) => {
        concat!("{", $uri, "}", $local)
    };
}

/// Defines a macro that writes names in a namespace.
///
/// `namespace!(atom = "http://www.w3.org/2005/Atom")` defines `atom!` so
/// that `atom!("title")` is [`qname!("http://www.w3.org/2005/Atom",
/// "title")`](qname), `atom!(@ "href")` the attribute and `atom!()` the
/// URI.  The names of the macros are the prefixes of the namespaces in
/// [`prefixes!`].  Like all macros defined by macros, they can be used
/// after the invocation in the same module and its children.
///
/// ```
/// deser_xml::namespace!(atom = "http://www.w3.org/2005/Atom");
///
/// #[derive(deser::Deserialize)]
/// struct Entry {
///     #[deser(rename = atom!("title"))]
///     title: String,
///     #[deser(rename = atom!(@ "lang"))]
///     lang: Option<String>,
/// }
///
/// assert_eq!(atom!("title"), "{http://www.w3.org/2005/Atom}title");
/// assert_eq!(atom!(), "http://www.w3.org/2005/Atom");
/// ```
#[macro_export]
macro_rules! namespace {
    ($($name:ident = $uri:literal),+ $(,)?) => {
        $($crate::__namespace!($name, $uri, $);)+
    };
}

#[doc(hidden)]
#[macro_export]
// rustfmt indents the inner macro further on every run
#[rustfmt::skip]
macro_rules! __namespace {
    ($name:ident, $uri:literal, $d:tt) => {
        #[allow(unused_macros)]
        macro_rules! $name {
            () => {
                $uri
            };
            (@ $d local:literal) => {
                $crate::qname!(@ $uri, $d local)
            };
            ($d local:literal) => {
                $crate::qname!($uri, $d local)
            };
        }
    };
}

/// Writes the prefixes of namespaces defined by [`namespace!`].
///
/// Every namespace has the name of its macro as prefix unless another one
/// is given with `as`.  The result is the table for
/// [`SerializerConfig::namespaces`] and
/// [`DeserializerConfig::namespaces`]:
///
/// ```
/// use deser_xml::{DeserializerConfig, SerializerConfig, prefixes};
///
/// deser_xml::namespace!(
///     atom = "http://www.w3.org/2005/Atom",
///     dc = "http://purl.org/dc/elements/1.1/",
///     xlink = "http://www.w3.org/1999/xlink",
/// );
///
/// const PREFIXES: &[(&str, &str)] =
///     prefixes![atom as "", dc, xlink as "xl"];
/// assert_eq!(PREFIXES, [
///     ("", "http://www.w3.org/2005/Atom"),
///     ("dc", "http://purl.org/dc/elements/1.1/"),
///     ("xl", "http://www.w3.org/1999/xlink"),
/// ]);
///
/// const WRITE: SerializerConfig =
///     SerializerConfig::new().namespaces(PREFIXES);
/// const READ: DeserializerConfig =
///     DeserializerConfig::new().namespaces(PREFIXES);
/// ```
#[macro_export]
macro_rules! prefixes {
    ($($name:ident $(as $prefix:literal)?),* $(,)?) => {
        &[$(($crate::__prefix!($name $(, $prefix)?), $name!())),*]
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prefix {
    ($name:ident) => {
        stringify!($name)
    };
    ($name:ident, $prefix:literal) => {
        $prefix
    };
}

/// The names of the special keys and the prefixes of namespaces.
///
/// The deserializer publishes them in the state for [`Mixed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Names {
    pub(crate) attribute_prefix: &'static str,
    pub(crate) text_key: &'static str,
    pub(crate) namespaces: &'static [(&'static str, &'static str)],
}

impl Names {
    pub(crate) const fn new() -> Names {
        Names {
            attribute_prefix: "@",
            text_key: "$text",
            namespaces: &[],
        }
    }
}

impl Default for Names {
    fn default() -> Names {
        Names::new()
    }
}
