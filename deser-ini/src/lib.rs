//! INI files (and git's config files) for deser.
//!
//! ```rust
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Debug, Deserialize, Serialize)]
//! struct Config {
//!     name: String,
//!     server: Server,
//! }
//!
//! #[derive(Debug, Deserialize, Serialize)]
//! struct Server {
//!     host: String,
//!     port: u16,
//!     #[deser(default)]
//!     tags: Vec<String>,
//! }
//!
//! let config: Config = deser_ini::from_str("
//! name = shop
//!
//! [server]
//! host = localhost  ; the host to bind to
//! port = 8080
//! tags = a
//! tags = b
//! ").unwrap();
//! assert_eq!(config.name, "shop");
//! assert_eq!(config.server.port, 8080);
//! assert_eq!(config.server.tags, ["a", "b"]);
//!
//! assert_eq!(
//!     deser_ini::to_string(&config).unwrap(),
//!     "name = shop\n\n[server]\nhost = localhost\nport = 8080\ntags = a\ntags = b\n"
//! );
//! ```
//!
//! # Dialects
//!
//! INI files have no specification, every implementation reads them a
//! little differently.  The default ([`DeserializerConfig::new`]) reads
//! what is common today:
//!
//! * Lines that start with `;` or `#` are comments.  After values, a `;`
//!   that follows whitespace starts a comment and so does a `#` that follows
//!   whitespace after the value (`key = value ; comment`), so `1;2;3`,
//!   `A;B;` and `#ff0000` are values (see [`InlineComments`]).
//! * `=` and `:` separate keys and values, whitespace around keys and
//!   values is removed.
//! * Lines that are indented more than the line of their key continue its
//!   value (see [`Continuation`]), like in Python's `configparser`:
//!
//!   ```ini
//!   [options]
//!   install_requires =
//!       deser
//!       requests
//!   ```
//! * A value that is quoted as a whole (`"value"` or `'value'`) is read
//!   without the quotes (see [`Quotes`]).  This is how whitespace at the
//!   start or end of a value and comment characters are written.
//! * A key without value (`skip-name-resolve`) is a key with a null value.
//! * Keys can come before the first section.
//!
//! These choices are based on a survey of INI files on GitHub: values are
//! quoted for PHP, MySQL, Windows, Unreal and most other readers that are
//! not Python, values with indented continuation lines are common in Python
//! tools (`tox.ini`, `setup.cfg`, `pylintrc`) and PlatformIO, and keys are
//! rarely indented more than the key before them.
//! [`DeserializerConfig::python`] reads files of Python's `configparser`
//! (no inline comments and quotes) and [`DeserializerConfig::git`] reads
//! git's config files ([`Syntax::Git`]).
//!
//! # Data Model
//!
//! An INI file is a map of its sections and the keys before the first
//! section, sections are maps of their keys:
//!
//! | INI file                           | deser                               |
//! |------------------------------------|-------------------------------------|
//! | `a = 1`                            | `{"a": "1"}`                        |
//! | `[s]` `a = 1`                      | `{"s": {"a": "1"}}`                 |
//! | `[s]` `a = 1` `a = 2`              | `{"s": {"a": "1", "a": "2"}}` (a repeated key) |
//! | `[s]` `a = 1` `[s]` `b = 2`        | `{"s": {"a": "1", "b": "2"}}`       |
//! | `[s]` `a`                          | `{"s": {"a": null}}`                |
//! | `[s]`                              | `{"s": {}}`                         |
//! | `[s "x"]` `a = 1` (git)            | `{"s": {"x": {"a": "1"}}}`          |
//!
//! Sections that are given more than once are merged.  Keys can repeat
//! (the maps are [multimaps]): fields and map values that are collections
//! (like `Vec<T>`) collect the values of all occurrences of their key, also
//! if other keys are between them.  Types that expect a single value receive
//! the last one unless the [`Context`](deser_core::Context) has another
//! [`DuplicateKeys`](deser_core::de::DuplicateKeys) policy.  A name that is
//! a key and a section is an error.
//!
//! Everything in an INI file is text, the type of a value is only known to
//! the type it's deserialized into.  Keys and values are therefore passed
//! on as [lexical atoms](deser_core::Atom::Lexical) which are parsed by the
//! types they are delivered to: numbers parse them, strings take them as
//! they are.  Booleans accept `true`, `yes`, `on` and `1` and `false`, `no`,
//! `off` and `0`.  Empty values are `None` for optionals of types that do
//! not accept them (`port =` is `None` for an `Option<u16>` and `Some("")`
//! for an `Option<String>`).  Keys without value are null, the
//! [`Flag`](deser_core::adapters::Flag) adapter reads them as `true`:
//!
//! ```rust
//! use deser::adapters::Flag;
//!
//! #[derive(deser::Deserialize)]
//! struct Mysqld {
//!     #[deser(as = Flag)]
//!     skip_name_resolve: bool,
//!     port: Option<u16>,
//! }
//!
//! #[derive(deser::Deserialize)]
//! struct MyCnf {
//!     mysqld: Mysqld,
//! }
//!
//! let cnf: MyCnf =
//!     deser_ini::from_str("[mysqld]\nskip_name_resolve\nport =\n").unwrap();
//! assert!(cnf.mysqld.skip_name_resolve);
//! assert_eq!(cnf.mysqld.port, None);
//! ```
//!
//! Lists in a single value (`hosts = a, b`) use the
//! [`Separated`](deser_core::adapters::Separated) adapter, values on
//! continuation lines are separated by line breaks
//! (`Separated<'\n'>`).
//!
//! Interpolation (`%(name)s`, `${name}`) and the `DEFAULT` section of
//! `configparser` are not supported, they are text.
//!
//! [multimaps]: deser_core::ContainerShape::set_multimap
//!
//! # Errors
//!
//! Errors point at the line and column of the input, also errors of values
//! that do not parse (like `port = http` for a `u16`).  With `deser-path`
//! they also have the path of the value (`server.port`), see
//! [`Deserializer`].
//!
//! # Serialization
//!
//! Values are written as INI files (see [`SerializerConfig`]): the value
//! has to be a map (like a struct), its entries with maps as values are
//! sections and the others are written before the first section.
//! Sequences are written as repeated keys.
//!
//! # Streams
//!
//! INI files are read from a [`Read`](std::io::Read) with [`from_reader`]
//! and written to a [`Write`](std::io::Write) with [`to_writer`].  The
//! configurations also create readers and writers of
//! [`deser::io`](deser_core::io) ([`DeserializerConfig::reader`] and
//! [`SerializerConfig::writer`]).  An INI file holds a single value, the
//! whole stream is read before it's parsed.
//!
//! # Features
//!
//! * `io` (enabled by default): reading and writing streams of the
//!   standard library, see [streams](#streams).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

mod de;
mod num;
mod parser;
mod ser;
mod stream;

pub use self::de::{Deserializer, DeserializerConfig, DeserializerConfigBuilder};
#[cfg(feature = "io")]
pub use self::ser::to_writer;
pub use self::ser::{Serializer, SerializerConfig, SerializerConfigBuilder, to_string};
pub use self::stream::StreamDeserializer;
#[cfg(feature = "io")]
pub use self::stream::from_reader;

use deser_core::Error;
use deser_core::de::Deserialize;

/// The syntax of the files.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::{DeserializerConfig, Syntax};
///
/// type Config = BTreeMap<String, BTreeMap<String, String>>;
///
/// let git = DeserializerConfig::builder().syntax(Syntax::Git).build();
/// let config: Config = git
///     .from_str("[Alias]\n\tLG = \"log --graph\" # short\n")
///     .unwrap();
/// assert_eq!(config["alias"]["lg"], "log --graph");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Syntax {
    /// INI files, see the [crate documentation](crate#dialects) and the
    /// options of [`DeserializerConfig`].
    #[default]
    Ini,
    /// git's config files (`.gitconfig`, `.git/config`, `.gitmodules`), read
    /// like git reads them (see `git help config`).
    ///
    /// The names of sections and keys are case insensitive and lowercased,
    /// keys are letters, digits and `-`.  `[section "subsection"]` (and the
    /// deprecated `[section.subsection]`) are nested maps.  `;` and `#` start
    /// comments anywhere outside of quotes, values can be quoted in parts
    /// (`a" b "c`), `\n`, `\t`, `\b`, `\"` and `\\` are escapes and a
    /// backslash at the end of a line continues the value on the next
    /// line.  Whitespace at the start and the end of values is removed
    /// unless it's quoted.  Includes (`[include]`) are not followed.
    Git,
}

/// Where comments start after values.
///
/// Lines that start with `;` or `#` (after whitespace) are always comments.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::{DeserializerConfig, InlineComments};
///
/// let input = "a = 1;2 ; one\nb = x #y\nc = #fff";
/// let with = |comments| -> BTreeMap<String, String> {
///     DeserializerConfig::builder()
///         .inline_comments(comments)
///         .build()
///         .from_str(input)
///         .unwrap()
/// };
/// let values = with(InlineComments::AfterWhitespace);
/// assert_eq!((&*values["a"], &*values["b"], &*values["c"]), ("1;2", "x", "#fff"));
/// let values = with(InlineComments::None);
/// assert_eq!((&*values["a"], &*values["b"], &*values["c"]), ("1;2 ; one", "x #y", "#fff"));
/// let values = with(InlineComments::Anywhere);
/// assert_eq!((&*values["a"], &*values["b"], &*values["c"]), ("1", "x", ""));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InlineComments {
    /// There are no comments after values (like Python's `configparser`).
    None,
    /// A `;` that follows whitespace starts a comment and so does a `#` that
    /// follows whitespace after the value.
    ///
    /// Values can contain `;` and `#` that do not follow whitespace (`1;2`)
    /// and start with `#` (`#ff0000`).
    #[default]
    AfterWhitespace,
    /// `;` and `#` start comments anywhere (outside of quoted values).
    Anywhere,
}

/// How values continue on the next lines.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::{Continuation, DeserializerConfig};
///
/// let config = DeserializerConfig::new();
/// let value: BTreeMap<String, String> =
///     config.from_str("deps =\n    a\n    b\nnext = 1").unwrap();
/// assert_eq!(value["deps"], "a\nb");
///
/// let config =
///     DeserializerConfig::builder().continuation(Continuation::Backslash).build();
/// let value: BTreeMap<String, String> =
///     config.from_str("cmd = a \\\n  b").unwrap();
/// assert_eq!(value["cmd"], "a   b");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Continuation {
    /// Values are a single line, indented lines are lines of their own.
    None,
    /// Lines that are indented more than the line of the key continue its
    /// value, like in Python's `configparser`.
    ///
    /// The lines are joined with line breaks (without the indentation),
    /// empty lines between them are kept.  If the line of the key has no
    /// value (`key =`), the value starts on the next line.  Comment lines
    /// are skipped.
    #[default]
    Indented,
    /// A backslash at the end of a line continues the value on the next
    /// line (the backslash and the line break are removed), like in Windows
    /// INF files and systemd units.
    Backslash,
}

/// How quoted values are read.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_ini::{DeserializerConfig, Quotes};
///
/// let input = "a = \" x ; y \" ; comment\nb = \"say \\\"hi\\\"\"\nc = \"a\" \"b\"";
/// let values: BTreeMap<String, String> = deser_ini::from_str(input).unwrap();
/// assert_eq!(values["a"], " x ; y ");
/// assert_eq!(values["b"], "say \"hi\"");
/// assert_eq!(values["c"], "\"a\" \"b\"");
///
/// let config = DeserializerConfig::builder().quotes(Quotes::None).build();
/// let values: BTreeMap<String, String> = config.from_str("a = \"x\"").unwrap();
/// assert_eq!(values["a"], "\"x\"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Quotes {
    /// Quotes are text.
    None,
    /// A value that is quoted as a whole (with `"` or `'`, optionally
    /// followed by a comment) is read without the quotes.  In double quotes
    /// `\"` and `\\` are escapes, other backslashes are text.  Values with
    /// quotes in other places are text.
    #[default]
    Value,
}

/// Deserializes a value from an INI file.
///
/// This uses the default [`DeserializerConfig`], see the [crate
/// documentation](crate) for how INI files map onto deser.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, BTreeMap<String, u32>> =
///     deser_ini::from_str("[a]\nb = 1\nc = 2").unwrap();
/// assert_eq!(value["a"]["c"], 2);
/// ```
#[allow(clippy::should_implement_trait)]
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    DeserializerConfig::new().from_str(s)
}

/// Deserializes a value from an INI file in a byte slice.
///
/// The input has to be UTF-8, a byte order mark is skipped.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, String> =
///     deser_ini::from_slice(b"\xef\xbb\xbfa = \xc3\xa4").unwrap();
/// assert_eq!(value["a"], "\u{e4}");
/// ```
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(bytes)
}

// the examples of the readme are tested
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
