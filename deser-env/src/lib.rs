//! Environment variables for deser.
//!
//! ```rust
//! use deser::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! struct Config {
//!     name: String,
//!     debug: bool,
//!     server: Server,
//! }
//!
//! #[derive(Debug, Deserialize)]
//! struct Server {
//!     port: u16,
//!     max_connections: Option<u32>,
//! }
//!
//! // `deser_env::from_env::<Config>("APP_")` reads the environment of the
//! // process, `from_vars` reads the given variables
//! let config: Config = deser_env::from_vars("APP_", [
//!     ("APP_NAME", "shop"),
//!     ("APP_DEBUG", "yes"),
//!     ("APP_SERVER__PORT", "8080"),
//!     ("PATH", "/usr/bin"),
//! ])
//! .unwrap();
//! assert_eq!(config.name, "shop");
//! assert!(config.debug);
//! assert_eq!(config.server.port, 8080);
//! assert_eq!(config.server.max_connections, None);
//! ```
//!
//! # Data Model
//!
//! The variables with names that start with a prefix (like `APP_`) are a
//! map.  The prefix is removed and the rest of the name is split at the
//! separator (`__` by default, see [`DeserializerConfig::set_separator`]) into
//! nested keys which are lowercased (see [`Case`]):
//!
//! | variables                               | deser                                     |
//! |-----------------------------------------|-------------------------------------------|
//! | `APP_PORT=80`                           | `{"port": "80"}`                          |
//! | `APP_MAX_CONNECTIONS=5`                 | `{"max_connections": "5"}`                |
//! | `APP_SERVER__PORT=80`                   | `{"server": {"port": "80"}}`              |
//! | `APP_HOSTS__0=a` `APP_HOSTS__1=b`       | `{"hosts": ["a", "b"]}`                   |
//! | `APP_HOSTS__1=a` `APP_HOSTS__3=b`       | `{"hosts": {"1": "a", "3": "b"}}`         |
//! | `APP_FEATURES__NEW_UI=on`               | `{"features": {"new_ui": "on"}}`          |
//!
//! A single `_` separates words in names, which is why nested keys are
//! separated with two.  Indexes that start at `0` and have no gaps are
//! sequences, other indexes are map keys.  Names that start or end with the
//! separator or contain it twice in a row are taken as they are.  A name
//! that has a value and nested names (`APP_DB=x` and `APP_DB__POOL=4`) is an
//! error.  The order of the environment is arbitrary, the variables are
//! sorted by name.
//!
//! Everything in the environment is text, the type of a value is only known
//! to the type it's deserialized into.  Keys and values are therefore passed
//! on as [lexical atoms](deser_core::Atom::Lexical) which are parsed by the
//! types they are delivered to: numbers parse them, strings take them as
//! they are.  Booleans accept `true`, `yes`, `on` and `1` and `false`, `no`,
//! `off` and `0`.  Lexical atoms are retained when values are buffered, so
//! flattened structs and internally tagged and untagged enums work as well.
//!
//! ## Empty Values
//!
//! A variable that is set to the empty string is the empty value, like
//! `?page=` in a query string: it's `None` for optionals of types that do
//! not accept it (`APP_PORT=` is `None` for an `Option<u16>`) and
//! `Some("")` for an `Option<String>`.  Variables that switch something on
//! by being set (like `APP_VERBOSE=`) use the
//! [`Flag`](deser_core::adapters::Flag) adapter which treats the empty
//! value as `true`:
//!
//! ```rust
//! use deser::adapters::Flag;
//!
//! #[derive(deser::Deserialize)]
//! struct Options {
//!     #[deser(as = Flag)]
//!     verbose: bool,
//!     port: Option<u16>,
//! }
//!
//! let options: Options =
//!     deser_env::from_vars("APP_", [("APP_VERBOSE", ""), ("APP_PORT", "")])
//!         .unwrap();
//! assert!(options.verbose);
//! assert_eq!(options.port, None);
//! ```
//!
//! ## Lists
//!
//! Sequences can be given with indexes (`APP_HOSTS__0`) without changes to
//! the types.  A single variable (`APP_HOSTS=a`) is a list of one value and
//! a list without variables is empty (the maps of the environment are
//! [multimaps](deser_core::ContainerShape::set_multimap), like query
//! strings).  Lists in a single variable (`APP_HOSTS=a,b,c`) use the
//! [`Separated`](deser_core::adapters::Separated) adapter, with
//! [`TrimWhitespace`](deser_core::adapters::TrimWhitespace) to allow spaces
//! (`a, b, c`).  Both work with all formats, a configuration file can still
//! give an array:
//!
//! ```rust
//! use deser::adapters::{Separated, TrimWhitespace};
//!
//! #[derive(deser::Deserialize)]
//! struct Config {
//!     #[deser(as = Separated<',', TrimWhitespace>)]
//!     hosts: Vec<String>,
//!     ports: Vec<u16>,
//!     tags: Vec<String>,
//! }
//!
//! let config: Config = deser_env::from_vars("APP_", [
//!     ("APP_HOSTS", "a, b"),
//!     ("APP_PORTS__0", "80"),
//!     ("APP_PORTS__1", "443"),
//! ])
//! .unwrap();
//! assert_eq!(config.hosts, ["a", "b"]);
//! assert_eq!(config.ports, [80, 443]);
//! assert!(config.tags.is_empty());
//! ```
//!
//! # Errors
//!
//! Errors carry the name of the variable they refer to (see [`EnvVar`]).
//! This is also the case for unknown fields that are collected as warnings
//! (see [`UnknownFields`](deser_core::de::UnknownFields)) and for values
//! which are buffered:
//!
//! ```rust
//! use deser_env::EnvVar;
//!
//! #[derive(Debug, deser::Deserialize)]
//! struct Config {
//!     port: u16,
//! }
//!
//! let vars = [("APP_PORT", "http")];
//! let err = deser_env::from_vars::<Config, _, _, _>("APP_", vars)
//!     .unwrap_err();
//! assert_eq!(err.attachment::<EnvVar>().unwrap().name(), "APP_PORT");
//! assert_eq!(
//!     err.to_string(),
//!     "InvalidValue: invalid value \"http\", expected u16 \
//!      (environment variable APP_PORT)"
//! );
//! ```
//!
//! # Layering
//!
//! Environment variables typically override a configuration file.  With
//! [`update`](deser_core::de::Deserializer::update) the variables that are
//! set are applied to an existing value, nested structs are merged:
//!
//! ```rust
//! use deser::de::Deserializer as _;
//!
//! #[derive(Debug, deser::Deserialize)]
//! struct Config {
//!     server: Server,
//! }
//!
//! #[derive(Debug, deser::Deserialize)]
//! struct Server {
//!     host: String,
//!     port: u16,
//! }
//!
//! let mut config = Config {
//!     server: Server { host: "localhost".into(), port: 80 },
//! };
//! deser_env::Deserializer::from_vars("APP_", [("APP_SERVER__PORT", "8080")])
//!     .update(&mut config)
//!     .unwrap();
//! assert_eq!(config.server.host, "localhost");
//! assert_eq!(config.server.port, 8080);
//! ```
//!
//! # Platforms
//!
//! On Unix values which are not valid unicode are passed on as
//! [bytes](deser_core::adapters#bytes) (which `Vec<u8>` and `PathBuf`
//! accept), on other platforms they are an error.  Names that are not valid
//! unicode are skipped unless they start with the prefix.  On Windows names
//! are not case sensitive, the prefix is matched ignoring ASCII case.
//!
//! Setting environment variables of the running process is `unsafe` (see
//! [`std::env::set_var`]), [`from_vars`] is a better fit for tests.
//!
//! # Serialization
//!
//! Values are serialized into name-value pairs (see [`to_vars`] and
//! [`SerializerConfig`]), for instance to pass a configuration to a child
//! process with [`Command::envs`](std::process::Command::envs).
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

mod de;
mod num;
mod ser;

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use deser_core::Text;
use deser_core::de::{
    Deserialize, DeserializeDriver, DeserializeOwned, LexicalRules, missing_multimap_value,
};
use deser_core::{Atom, Error, ErrorAttachment, ErrorKind};

pub use self::de::{Deserializer, DeserializerConfig, DeserializerConfigBuilder};
pub use self::ser::{SerializerConfig, SerializerConfigBuilder, to_vars};

/// How the case of names maps onto keys.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_env::{Case, DeserializerConfig, SerializerConfig};
///
/// let vars = [("APP_Server__Port", "80")];
/// let value: BTreeMap<String, BTreeMap<String, u16>> =
///     deser_env::from_vars("APP_", vars).unwrap();
/// assert_eq!(value["server"]["port"], 80);
///
/// let preserve = DeserializerConfig::builder().case(Case::Preserve).build();
/// let value: BTreeMap<String, BTreeMap<String, u16>> =
///     preserve.from_vars("APP_", vars).unwrap();
/// assert_eq!(value["Server"]["Port"], 80);
///
/// let vars = deser_env::to_vars("APP_", &value).unwrap();
/// assert_eq!(vars, [("APP_SERVER__PORT".to_string(), "80".to_string())]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Case {
    /// Names are upper case and keys lower case.
    ///
    /// When deserializing names are lowercased (ASCII only), so
    /// `APP_SERVER__PORT` (and `APP_server__port`) is `server.port`.  When
    /// serializing keys are uppercased.  Keys of maps that are not lower
    /// case do not read back the same.
    #[default]
    Upper,
    /// Names and keys are the same.
    ///
    /// This is useful if the types have names in upper case, for instance
    /// with `#[deser(alias_all = "SCREAMING_SNAKE_CASE")]`.
    Preserve,
}

/// The environment variable an error refers to.
///
/// Errors of values and keys that come from environment variables carry
/// this [attachment](deser_core::ErrorAttachment).  For keys that more than
/// one variable share (like `server` in `APP_SERVER__HOST` and
/// `APP_SERVER__PORT`) it's the name up to the key (`APP_SERVER`).  Errors
/// that do not come from a single variable (like missing fields) have none.
///
/// ```
/// use deser_env::EnvVar;
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Config {
///     server: Server,
/// }
///
/// #[derive(Debug, deser::Deserialize)]
/// #[deser(deny_unknown_fields)]
/// struct Server {
///     port: u16,
/// }
///
/// let err = deser_env::from_vars::<Config, _, _, _>(
///     "APP_",
///     [("APP_SERVER__PROT", "80")],
/// )
/// .unwrap_err();
/// assert_eq!(err.message(), "unknown field `prot`, expected `port`");
/// assert_eq!(
///     err.attachment::<EnvVar>().unwrap().name(),
///     "APP_SERVER__PROT"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    name: Arc<str>,
}

impl EnvVar {
    pub(crate) fn new(name: Arc<str>) -> EnvVar {
        EnvVar { name }
    }

    /// Returns the name of the variable.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl ErrorAttachment for EnvVar {
    fn fmt_context(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, " (environment variable {})", self.name)
    }
}

/// Deserializes a value from the environment variables with a prefix.
///
/// This reads the environment of the process with the default
/// [`DeserializerConfig`], see the [crate documentation](crate) for how
/// variables map onto deser.  Without a prefix (`""`) all variables are
/// read.  Then `PATH`, `HOME` and the like are keys too which fails for
/// types that deny unknown fields.
///
/// ```no_run
/// #[derive(deser::Deserialize)]
/// struct Config {
///     port: u16,
/// }
///
/// // reads `APP_PORT`
/// let config: Config = deser_env::from_env("APP_").unwrap();
/// ```
pub fn from_env<T: DeserializeOwned>(prefix: &str) -> Result<T, Error> {
    Deserializer::from_env(prefix).deserialize()
}

/// Deserializes a value from the given variables with a prefix.
///
/// The variables are name-value pairs, for instance from
/// [`std::env::vars`] or a file, which are deserialized as if they were the
/// environment (see [`from_env`]).  Values that are given borrowed are
/// passed on borrowed.
///
/// ```
/// use std::collections::BTreeMap;
///
/// let value: BTreeMap<String, Vec<u32>> =
///     deser_env::from_vars("", [("A__0", "1"), ("A__1", "2"), ("B", "3")])
///         .unwrap();
/// assert_eq!(value["a"], [1, 2]);
/// assert_eq!(value["b"], [3]);
/// ```
pub fn from_vars<'a, T, I, K, V>(prefix: &str, vars: I) -> Result<T, Error>
where
    T: Deserialize<'a>,
    I: IntoIterator<Item = (K, V)>,
    K: Into<Cow<'a, str>>,
    V: Into<Cow<'a, str>>,
{
    Deserializer::from_vars(prefix, vars).deserialize()
}

/// Deserializes the value of a single environment variable.
///
/// The value is parsed like the values of [`from_env`] (numbers and
/// booleans parse, the empty value is `None` for optionals of types that do
/// not accept it).  Collections (like `Vec<T>`) are a collection of the
/// value, like the fields of structs.  If the variable is not set, types
/// that can be missing (like `Option<T>`) use their missing value and
/// collections are empty, other types fail.  Errors carry the name of the
/// variable (see [`EnvVar`]).
///
/// ```no_run
/// let port: u16 = deser_env::var("PORT").unwrap();
/// let workers: Option<usize> = deser_env::var("WORKERS").unwrap();
/// ```
pub fn var<T: DeserializeOwned>(name: &str) -> Result<T, Error> {
    let attach = |mut err: Error| {
        err.set_attachment(EnvVar::new(name.into()));
        err
    };
    let value = match std::env::var_os(name) {
        Some(value) => value,
        None => {
            // collections are empty (like the collections of missing
            // keys in `from_env`)
            return missing_multimap_value::<T>().ok_or_else(|| {
                attach(Error::new(
                    ErrorKind::MissingField,
                    "environment variable is not set",
                ))
            });
        }
    };
    let mut out = None;
    {
        // the variable stands for a key given once: collections (like
        // `Vec<T>`) are one value
        let mut driver = DeserializeDriver::multimap_value(&mut out);
        LexicalRules::LENIENT.set(driver.state_mut());
        match value.into_string() {
            Ok(text) => driver.emit(Atom::Lexical(Text::borrowed(&text))),
            Err(value) => match de::os_bytes(value) {
                Some(bytes) => driver.emit(Atom::Bytes(deser_core::Bytes::borrowed(&bytes))),
                None => Err(Error::new(ErrorKind::Syntax, "value is not valid unicode")),
            },
        }
        .map_err(attach)?;
    }
    out.ok_or_else(|| attach(Error::new(ErrorKind::InvalidState, "no value")))
}
