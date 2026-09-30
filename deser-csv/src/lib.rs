//! CSV, TSV and other delimited text for deser.
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Debug, Deserialize, Serialize)]
//! struct City {
//!     name: String,
//!     country: String,
//!     population: Option<u64>,
//! }
//!
//! let input =
//!     "name,country,population\nVienna,Austria,1897000\nAtlantis,,\n";
//! let cities: Vec<City> = deser_csv::from_str(input).unwrap();
//! assert_eq!(cities[0].population, Some(1897000));
//! assert_eq!(cities[1].population, None);
//!
//! assert_eq!(deser_csv::to_string(&cities).unwrap(), input);
//! ```
//!
//! # Data Model
//!
//! A document is a sequence of records.  By default the first record holds
//! the names of the columns (see [`Headers`]) and the other records are
//! maps of the names to their fields, without names records are sequences
//! (for instance tuples):
//!
//! | input                  | deser                                     |
//! |------------------------|-------------------------------------------|
//! | `a,b\n1,2\n3,4\n`      | `[{"a": "1", "b": "2"}, {"a": "3", "b": "4"}]` |
//! | `1,2\n3,4\n` (no names)| `[["1", "2"], ["3", "4"]]`                |
//!
//! Everything in a CSV file is text, only the type a field is deserialized
//! into knows what it means.  Fields are therefore passed on as [lexical
//! atoms](deser_core::Atom::Lexical) which are parsed by the types they are
//! delivered to: numbers parse them, strings take them as they are.  They
//! are interpreted with the [lenient
//! rules](deser_core::de::LexicalRules::LENIENT): `yes`, `on` and `1` are
//! booleans too and empty fields are `None` for optional numbers.  This
//! also works when values are buffered, so flattened structs and
//! internally tagged and untagged enums work:
//!
//! ```rust
//! use deser::Deserialize;
//!
//! #[derive(Debug, Deserialize, PartialEq)]
//! #[deser(tag = "kind", rename_all = "lowercase")]
//! enum Shape {
//!     Circle { radius: f64 },
//!     Rect { width: f64, height: f64 },
//! }
//!
//! #[derive(Debug, Deserialize, PartialEq)]
//! struct Row {
//!     id: u32,
//!     #[deser(flatten)]
//!     shape: Shape,
//! }
//!
//! let input = "id,kind,radius,width,height\n1,circle,2,,\n2,rect,,3,4\n";
//! let config =
//!     deser_csv::DeserializerConfig::new().nulls(deser_csv::Nulls::Empty);
//! let rows: Vec<Row> = config.from_str(input).unwrap();
//! assert_eq!(rows[1].shape, Shape::Rect { width: 3.0, height: 4.0 });
//! ```
//!
//! Empty fields are `None` for optionals if the type does not accept them
//! (an empty field is `None` for an `Option<u32>` and `Some("")` for an
//! `Option<String>`).  Which fields are null can be configured (see
//! [`Nulls`]).  Fields cannot hold maps or sequences, but a list in a
//! field can be read with the [`Separated`](deser_core::adapters::Separated)
//! adapter (`#[deser(as = Separated<';'>)]` reads `a;b;c`).
//!
//! Serializing works the other way around (see [`SerializerConfig`]): the
//! value is a sequence of records which are maps (the keys of the first
//! record are the names of the columns) or sequences.
//!
//! # Dialects
//!
//! There is no single CSV format, the configurations can be adjusted to
//! what the other side writes and reads:
//!
//! * The delimiter (`,`, `;`, `\t`, `|`, the ASCII unit separator, ...),
//!   the quote character (or no quotes at all) and if quotes are doubled
//!   or escaped (see [`Escape`]).
//! * The line ending (see [`Terminator`]), by default `\n`, `\r\n` and
//!   `\r` all end records and `\n` is written.
//! * Comment lines, blank lines, whitespace around fields (see [`Trim`]),
//!   records with a different number of fields (`flexible`) and fields
//!   with quotes that do not follow the rules (`lenient_quotes`).
//! * Tab separated values as written by databases (with backslash escapes
//!   and `\N` for null), see [`DeserializerConfig::tsv`].
//!
//! Input is UTF-8, a byte order mark at the start is skipped (and UTF-16
//! input is reported as such).  Fields which are not UTF-8 are passed on as
//! bytes.  The `sep=;` line Excel writes can be enabled with
//! [`DeserializerConfig::sep_line`].
//!
//! # Errors
//!
//! Errors point at the position in the input and (with `deser-path`) at
//! the path of the value, for instance `[3].price`.  By default quotes
//! that do not follow the rules and records with the wrong number of fields
//! are errors.
//!
//! # Streams
//!
//! A stream of records is read with a reader of `deser::io` which the
//! configuration creates ([`DeserializerConfig::reader`]): every read
//! returns the next record.  Errors of a record (like a field that does
//! not fit the type) only discard the record.  The names of the columns
//! are known to the stream deserializer (see
//! [`StreamDeserializer::headers`]).
//!
//! ```rust
//! # #[cfg(feature = "io")] {
//! use deser_csv::DeserializerConfig;
//!
//! #[derive(deser::Deserialize)]
//! struct Row {
//!     name: String,
//!     age: u32,
//! }
//!
//! let input = &b"name,age\njane,42\njohn,x\nmax,7\n"[..];
//! let mut reader = DeserializerConfig::new().reader(input);
//! let mut ages = Vec::new();
//! let mut errors = Vec::new();
//! loop {
//!     match reader.read::<Row>() {
//!         Ok(Some(row)) => ages.push(row.age),
//!         Ok(None) => break,
//!         Err(err) => errors.push(err.line()),
//!     }
//! }
//! assert_eq!(ages, [42, 7]);
//! assert_eq!(errors, [Some(3)]);
//! assert_eq!(reader.deserializer().headers().unwrap(), ["name", "age"]);
//! # }
//! ```
//!
//! Likewise every value written with a writer created by
//! [`SerializerConfig::writer`] is a record, the names are written before
//! the first one.  The functions that read and write a single value
//! ([`from_reader`] and [`to_writer`]) read and write all records as a
//! sequence, like [`from_str`] and [`to_string`].
//!
//! The stream serializer ([`Serializer`]) and the stream deserializer
//! ([`StreamDeserializer`]) do not do IO themselves (see
//! [`deser::stream`](deser_core::stream)), they also work with other kinds
//! of IO and without the standard library.
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library, see [streams](#streams).  Requires `std`.
//! * `std` (enabled by default): uses the standard library.  Without it
//!   this crate only needs `alloc` (see [`no_std`](https://docs.rs/deser/latest/deser/#no_std)).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

mod de;
mod num;
mod parser;
mod ser;
mod stream;

pub use self::de::{Deserializer, DeserializerConfig, Records};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Serializer, SerializerConfig, to_string};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;

use deser_core::Error;
use deser_core::de::Deserialize;

/// Where the names of the columns come from.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_csv::{DeserializerConfig, Headers};
///
/// let rows: Vec<BTreeMap<String, u32>> =
///     deser_csv::from_str("a,b\n1,2\n").unwrap();
/// assert_eq!(rows[0]["b"], 2);
///
/// let config = DeserializerConfig::new().headers(Headers::None);
/// let rows: Vec<Vec<u32>> = config.from_str("1,2\n3,4\n").unwrap();
/// assert_eq!(rows, [[1, 2], [3, 4]]);
///
/// let config = DeserializerConfig::new().headers(Headers::Skip);
/// let rows: Vec<(String, u32)> =
///     config.from_str("name,age\njane,42\n").unwrap();
/// assert_eq!(rows, [("jane".to_string(), 42)]);
///
/// let config =
///     DeserializerConfig::new().headers(Headers::Given(&["a", "b"]));
/// let rows: Vec<BTreeMap<String, u32>> = config.from_str("1,2\n").unwrap();
/// assert_eq!(rows[0]["a"], 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Headers {
    /// The first record holds the names, records are maps.
    #[default]
    First,
    /// The first record holds the names, but records are sequences (for
    /// instance to read them into tuples).
    ///
    /// The names are still read (see [`StreamDeserializer::headers`]).
    Skip,
    /// There are no names, records are sequences.
    None,
    /// The names are given (the first record is a regular record), records
    /// are maps.
    Given(&'static [&'static str]),
}

/// What ends records.
///
/// Line breaks in quoted fields do not end records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Terminator {
    /// Records end with `\n`, `\r\n` or `\r` (also mixed).  `\n` is
    /// written.
    #[default]
    Newline,
    /// Like [`Newline`](Self::Newline) but `\r\n` is written (as described
    /// by RFC 4180).
    CrLf,
    /// Records end with the given character (for instance the ASCII record
    /// separator `0x1e`).
    Byte(u8),
}

/// How characters are escaped.
///
/// ```
/// use deser_csv::{DeserializerConfig, Escape, Headers};
///
/// let config = DeserializerConfig::new()
///     .headers(Headers::None)
///     .escape(Escape::Backslash)
///     .double_quote(false);
/// let rows: Vec<Vec<String>> = config.from_str(r#""a\"b",c\,d"#).unwrap();
/// assert_eq!(rows, [["a\"b", "c,d"]]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Escape {
    /// Nothing is escaped (quotes in quoted fields are doubled).
    #[default]
    None,
    /// The character after this one is taken as it is (in quoted and
    /// unquoted fields), like `escapechar` of Python's `csv` module.
    Char(u8),
    /// Backslash escapes: `\t`, `\n`, `\r` and `\0` are a tab, line feed,
    /// carriage return and NUL, a backslash followed by another character
    /// is that character (`\\`, `\"`, `\,`).  This is how databases (like
    /// PostgreSQL and MySQL) write delimited text.
    Backslash,
}

impl Escape {
    /// Returns the escape character.
    pub(crate) const fn byte(self) -> Option<u8> {
        match self {
            Escape::None => None,
            Escape::Char(byte) => Some(byte),
            Escape::Backslash => Some(b'\\'),
        }
    }
}

/// Which whitespace is removed.
///
/// Spaces and tabs are removed from the start and end of unquoted fields
/// and around the quotes of quoted fields (`a, "b" ,c`), whitespace in
/// quotes is kept.
///
/// ```
/// use deser_csv::{DeserializerConfig, Trim};
///
/// #[derive(deser::Deserialize)]
/// struct Row {
///     name: String,
///     age: u32,
/// }
///
/// let config = DeserializerConfig::new().trim(Trim::All);
/// let rows: Vec<Row> =
///     config.from_str("name , age\n \"jane \" , 42\n").unwrap();
/// assert_eq!((rows[0].name.as_str(), rows[0].age), ("jane ", 42));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Trim {
    /// Whitespace is kept.
    #[default]
    None,
    /// Whitespace is removed from the names of the columns.
    Headers,
    /// Whitespace is removed from the fields of records.
    Fields,
    /// Whitespace is removed from names and fields.
    All,
}

/// Which fields are null.
///
/// Only unquoted fields are null, which allows writing the empty string
/// (or the text of null) in quotes.
///
/// ```
/// use deser_csv::{DeserializerConfig, Headers, Nulls};
///
/// let config = DeserializerConfig::new().headers(Headers::None);
/// let rows: Vec<Vec<Option<String>>> = config.from_str(",\"\"\n").unwrap();
/// assert_eq!(rows, [[Some("".to_string()), Some("".to_string())]]);
///
/// let config = config.nulls(Nulls::Empty);
/// let rows: Vec<Vec<Option<String>>> = config.from_str(",\"\"\n").unwrap();
/// assert_eq!(rows, [[None, Some("".to_string())]]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Nulls {
    /// No field is null.  Empty fields are still `None` for optionals of
    /// types that do not accept the empty string.
    #[default]
    None,
    /// Empty fields are null (like PostgreSQL's CSV format).
    Empty,
    /// Fields with this text are null (like `\N` or `NULL`).
    Text(&'static str),
}

/// When fields are quoted.
///
/// ```
/// use deser_csv::{QuoteStyle, SerializerConfig};
///
/// let rows = vec![("a b", 1), ("c,d", 2)];
/// let with = |style| {
///     SerializerConfig::new().quote_style(style).to_string(&rows).unwrap()
/// };
/// assert_eq!(with(QuoteStyle::Necessary), "a b,1\n\"c,d\",2\n");
/// assert_eq!(with(QuoteStyle::Always), "\"a b\",\"1\"\n\"c,d\",\"2\"\n");
/// assert_eq!(with(QuoteStyle::NonNumeric), "\"a b\",1\n\"c,d\",2\n");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum QuoteStyle {
    /// Fields are quoted if they have to be (if they contain special
    /// characters or would otherwise be read differently).
    #[default]
    Necessary,
    /// All fields are quoted.
    Always,
    /// All fields except for numbers are quoted.
    NonNumeric,
    /// Fields are never quoted, fields that need quotes are an error
    /// (unless they can be escaped, see [`Escape`]).
    Never,
}

/// Deserializes the records of a string.
///
/// This uses the default [`DeserializerConfig`] (CSV with a header), see
/// the [crate documentation](crate) for how records map onto deser.
///
/// ```
/// #[derive(deser::Deserialize)]
/// struct Row {
///     name: String,
///     score: f64,
/// }
///
/// let rows: Vec<Row> =
///     deser_csv::from_str("name,score\na,1.5\nb,2\n").unwrap();
/// assert_eq!(rows[1].score, 2.0);
/// ```
#[allow(clippy::should_implement_trait)]
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    DeserializerConfig::new().from_str(s)
}

/// Deserializes the records of a byte slice.
///
/// Fields which are not UTF-8 are passed on as bytes.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let rows: Vec<BTreeMap<String, u32>> =
///     deser_csv::from_slice(b"a,b\n1,2\n").unwrap();
/// assert_eq!(rows[0]["b"], 2);
/// ```
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(bytes)
}
