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
//!     "<feed><title>Example</title><link href=\"/a\"/><link href=\"/b\" rel=\"self\"/>\
//!      <count>3</count></feed>"
//! );
//! ```
//!
//! # Data Model
//!
//! The document is the value of its root element (the name of the root
//! element is not checked).  An element is:
//!
//! * **Its text** if it has neither attributes nor child elements:
//!   `<count>3</count>` is `"3"` and `<empty/>` is `""`.  Like the values of
//!   query strings, text is passed on as
//!   [lexical atom](deser_core::Atom::Lexical) that the type it's
//!   deserialized into parses.  An empty element is `None` for optionals of
//!   types that do not accept the empty text.
//! * **A map** otherwise.  Attributes are entries whose key has the
//!   [attribute prefix](DeserializerConfig::attribute_prefix) (`@href`),
//!   child elements are entries with their name and text is an entry with
//!   the [text key](DeserializerConfig::text_key) (`$text`).  Whitespace
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
//! Maps are [multimaps](deser_core::ContainerShape::with_multimap) whose
//! order is [significant](deser_core::Order::Significant): an element can
//! have more than one child with the same name.  Fields and map values
//! that are collections (like `Vec<T>`) collect all of them, also if other
//! elements are between them.  A single child element is a collection of
//! one value and a missing one an empty collection.  For other types
//! [`DeserializerConfig::duplicate_keys`] decides.
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
//! let p: Paragraph = deser_xml::from_str(r#"<p class="x">x <b>y</b> z</p>"#).unwrap();
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
//! (`xmlns` attributes) are not data.  Namespaces can be given prefixes
//! that are used regardless of the prefixes of the document (see
//! [`DeserializerConfig::namespaces`]).  CDATA sections are text, the
//! predefined entities (`&amp;`, ...) and character references are
//! resolved.  Entities of document types are never expanded.
//!
//! Serializing works the other way around (see [`SerializerConfig`]).
//!
//! # Limitations
//!
//! This crate is an early version.  The serializer writes attributes before the content of an
//! element, a map whose attributes come after other keys is an error.  The
//! input has to be UTF-8.
#![deny(missing_docs)]

mod de;
mod mixed;
mod ser;

pub use self::de::{Deserializer, DeserializerConfig, from_slice, from_str};
pub use self::mixed::Mixed;
pub use self::ser::{SerializerConfig, to_string};

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
