//! Query strings and form data (`application/x-www-form-urlencoded`) for
//! deser.
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Debug, Deserialize, Serialize)]
//! struct Search {
//!     q: String,
//!     page: Option<u32>,
//!     #[deser(default)]
//!     tags: Vec<String>,
//! }
//!
//! let search: Search = deser_urlencoded::from_str("q=rust+serde&tags=a&tags=b").unwrap();
//! assert_eq!(search.q, "rust serde");
//! assert_eq!(search.page, None);
//! assert_eq!(search.tags, ["a", "b"]);
//!
//! let query = deser_urlencoded::to_string(&search).unwrap();
//! assert_eq!(query, "q=rust+serde&tags=a&tags=b");
//! ```
//!
//! # Data Model
//!
//! Everything in a query string is text, the type of a value is only known
//! to the type it's deserialized into.  Keys and values are therefore
//! passed on as [lexical atoms](deser::Atom::Lexical) which are parsed by
//! the types they are delivered to: numbers parse them, strings take them
//! as they are.  Lexical atoms are retained when values are buffered, so
//! flattened structs and internally tagged and untagged enums work as well:
//!
//! ```rust
//! use deser::Deserialize;
//!
//! #[derive(Debug, Deserialize, PartialEq)]
//! struct Paginate {
//!     limit: u32,
//!     offset: u32,
//! }
//!
//! #[derive(Debug, Deserialize, PartialEq)]
//! #[deser(tag = "type", rename_all = "lowercase")]
//! enum Query {
//!     Users {
//!         active: bool,
//!         #[deser(flatten)]
//!         paginate: Paginate,
//!     },
//! }
//!
//! let query: Query = deser_urlencoded::from_str("limit=10&offset=20&active=yes&type=users")
//!     .unwrap();
//! assert_eq!(
//!     query,
//!     Query::Users {
//!         active: true,
//!         paginate: Paginate { limit: 10, offset: 20 },
//!     }
//! );
//! ```
//!
//! The input is a map.  Keys can be nested (see [`Nesting`]):
//!
//! | query string               | deser                                               |
//! |----------------------------|-----------------------------------------------------|
//! | `a=1`                      | `{"a": "1"}`                                        |
//! | `a=1&a=2`                  | `{"a": ["1", "2"]}` (a [repeated key])              |
//! | `a[]=1&a[]=2`              | `{"a": ["1", "2"]}`                                 |
//! | `a[0]=1&a[1]=2`            | `{"a": ["1", "2"]}`                                 |
//! | `a[1]=1&a[3]=2`            | `{"a": {"1": "1", "3": "2"}}`                       |
//! | `a[b]=1&a[c][]=2`          | `{"a": {"b": "1", "c": ["2"]}}`                     |
//! | `a[0][b]=1&a[1][b]=2`      | `{"a": [{"b": "1"}, {"b": "2"}]}`                   |
//!
//! A key which is given once can stand for a single value or for a
//! sequence of one element: types that expect sequences (like `Vec<T>`)
//! accept a single value.  The values of a key which is given more than
//! once are a sequence which is marked as [repeated]: types that expect a
//! single value receive the last one (see
//! [`DeserializerConfig::duplicate_keys`]).  `a[]=1` is always a
//! sequence.  Indexes that start at `0` and have no gaps are sequences,
//! other indexes are map keys.
//!
//! The URL standard makes no difference between `a` and `a=`, both are the
//! empty value.  Empty values are `None` for optionals if the type does not
//! accept them (`page=` is `None` for an `Option<u32>` and `Some("")` for an
//! `Option<String>`).  Booleans accept `true`, `yes`, `on` and `1` and
//! `false`, `no`, `off` and `0` (HTML checkboxes send `on`).
//!
//! Keys and values are percent-decoded (`+` is a space) before the keys are
//! split, so `a%5B%5D=1` (as sent by browsers) is the same as `a[]=1`.
//! Values which are not UTF-8 after decoding are passed on as bytes.  A
//! leading `?` is ignored.  Keys which do not follow the syntax of the
//! nesting (like `a[b`) are taken as they are.  A key that has a value and
//! nested keys (`a=1&a[b]=2`) is an error.
//!
//! Serializing works the other way around (see [`SerializerConfig`]), how
//! sequences are written can be configured (see [`ArrayFormat`]).
//!
//! [repeated]: deser::ContainerShape::with_repeated
//!
//! # Streams
//!
//! Form data is read from a [`Read`](std::io::Read) with [`from_reader`]
//! and written to a [`Write`](std::io::Write) with [`to_writer`].  The
//! configurations can also be used with [`deser::io`] or an adapter for an
//! async runtime (such as `deser-tokio`).  The whole stream is read before
//! it's parsed.
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams, see
//!   [streams](#streams).
mod de;
mod encoding;
#[cfg(feature = "io")]
mod io;
mod ser;

pub use self::de::{Deserializer, DeserializerConfig};
#[cfg(feature = "io")]
pub use self::io::{from_reader, to_writer};
pub use self::ser::{ArrayFormat, Serializer, SerializerConfig, to_string};

use deser::Error;
use deser::de::Deserialize;

/// How keys are nested.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_urlencoded::{DeserializerConfig, Nesting};
///
/// type Nested = BTreeMap<String, BTreeMap<String, u32>>;
///
/// let value: Nested = deser_urlencoded::from_str("a[b]=1").unwrap();
/// assert_eq!(value["a"]["b"], 1);
///
/// let dots = DeserializerConfig::new().nesting(Nesting::Dots);
/// let value: Nested = dots.from_str("a.b=1").unwrap();
/// assert_eq!(value["a"]["b"], 1);
///
/// let flat = DeserializerConfig::new().nesting(Nesting::Flat);
/// let value: BTreeMap<String, u32> = flat.from_str("a[b]=1").unwrap();
/// assert_eq!(value["a[b]"], 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Nesting {
    /// Keys are not nested, they are taken as they are.
    Flat,
    /// Nested keys are in brackets: `a[b][0]=1` and `a[]=1`.
    ///
    /// This is how Rails, PHP and the `qs` library of Node write nested
    /// keys.
    #[default]
    Brackets,
    /// Nested keys are separated with dots: `a.b.0=1`.
    Dots,
}

/// Deserializes a value from a query string.
///
/// This uses the default [`DeserializerConfig`], see the [crate
/// documentation](crate) for how query strings map onto deser.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, Vec<u32>> = deser_urlencoded::from_str("a=1&a=2&b=3").unwrap();
/// assert_eq!(value["a"], [1, 2]);
/// assert_eq!(value["b"], [3]);
/// ```
#[allow(clippy::should_implement_trait)]
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    Deserializer::from_str(s).deserialize()
}

/// Deserializes a value from a query string in a byte slice.
///
/// The input has to be UTF-8.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, String> = deser_urlencoded::from_slice(b"a=%C3%A4").unwrap();
/// assert_eq!(value["a"], "\u{e4}");
/// ```
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    Deserializer::from_slice(bytes).deserialize()
}
