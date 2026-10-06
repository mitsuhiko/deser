use std::borrow::Cow;
use std::collections::HashMap;

use deser_core::ext::Number;
use deser_core::ser::SerializeDriver;
use deser_core::{Atom, BytesFormat, Error, ErrorKind, Event, Serialize, State};

use crate::Case;

/// Configures how values are serialized into environment variables.
///
/// The value has to serialize to a map (for instance a struct or a map
/// type).  The names are the keys with the prefix in front, nested keys are
/// joined with the separator and uppercased (see [`Case`]).  Keys whose
/// names would not read back as the same keys are an error: keys that
/// contain the separator (or end with a part of it), and different keys
/// with the same name (like `a` and `A`).  Sequences are
/// written with indexes (`APP_HOSTS__0`), use the
/// [`Separated`](deser_core::adapters::Separated) adapter to write them into
/// a single variable.  Null values (like `None`) of map entries are skipped,
/// null values in sequences are written as empty values.  Maps and sequences
/// that are empty are not written as variables cannot represent them.
///
/// Numbers are written with the shortest text that reads back as the same
/// value, booleans as `true` and `false` and bytes as base64 (or the
/// [`BytesFormat`](deser_core::BytesFormat) of the context).  Keys that are empty or contain the separator
/// are an error as they would not read back.
///
/// ```
/// use deser_env::SerializerConfig;
///
/// #[derive(deser::Serialize)]
/// struct Config {
///     name: &'static str,
///     server: Server,
///     hosts: Vec<&'static str>,
/// }
///
/// #[derive(deser::Serialize)]
/// struct Server {
///     port: u16,
///     timeout: Option<u32>,
/// }
///
/// let config = Config {
///     name: "shop",
///     server: Server { port: 80, timeout: None },
///     hosts: vec!["a", "b"],
/// };
/// let vars = SerializerConfig::new().to_vars("APP_", &config).unwrap();
/// let vars: Vec<_> =
///     vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
/// assert_eq!(
///     vars,
///     [
///         ("APP_NAME", "shop"),
///         ("APP_SERVER__PORT", "80"),
///         ("APP_HOSTS__0", "a"),
///         ("APP_HOSTS__1", "b"),
///     ]
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    separator: &'static str,
    case: Case,
    context: deser_core::Context,
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
            separator: "__",
            case: Case::Upper,
            context: deser_core::Context::new(),
        }
    }

    /// Returns a builder for the configuration (see [`SerializerConfigBuilder`]).
    pub const fn builder() -> SerializerConfigBuilder {
        SerializerConfigBuilder::new()
    }

    /// Returns a builder that starts with this configuration.
    pub const fn into_builder(self) -> SerializerConfigBuilder {
        SerializerConfigBuilder { value: self }
    }

    /// Sets the context the values are serialized in.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)), for instance
    /// the [`BytesFormat`](deser_core::BytesFormat).  A context set on the
    /// driver (for instance in the setup callback of
    /// [`to_vars_with`](Self::to_vars_with)) takes precedence.
    pub fn set_context(&mut self, context: deser_core::Context) {
        self.context = context;
    }

    /// Returns the context the values are serialized in.
    pub fn context(&self) -> &deser_core::Context {
        &self.context
    }

    /// Gives the context to a driver which has none.
    #[inline]
    fn apply_context(&self, driver: &mut SerializeDriver<'_>) {
        if !self.context.is_empty() {
            driver.set_default_context(self.context.clone());
        }
    }

    /// Sets the separator of nested keys.
    ///
    /// The default is `__`, see
    /// [`DeserializerConfig::set_separator`](crate::DeserializerConfig::set_separator).
    /// With the empty string nested maps and sequences are an error.
    pub const fn set_separator(&mut self, separator: &'static str) {
        self.separator = separator;
    }

    /// Sets how keys map onto names.
    ///
    /// The default is [`Case::Upper`] which uppercases keys.
    pub const fn set_case(&mut self, case: Case) {
        self.case = case;
    }

    /// Serializes a value into variables with a prefix.
    ///
    /// See [`to_vars`].
    pub fn to_vars<T: Serialize + ?Sized>(
        &self,
        prefix: &str,
        value: &T,
    ) -> Result<Vec<(String, String)>, Error> {
        self.to_vars_with(prefix, value, |_| {})
    }

    /// Serializes a value into variables with a prefix and a configured
    /// driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_vars_with<F, T: Serialize + ?Sized>(
        &self,
        prefix: &str,
        value: &T,
        setup: F,
    ) -> Result<Vec<(String, String)>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(&value);
        setup(&mut driver);
        self.apply_context(&mut driver);
        let mut writer = Writer {
            config: self,
            bytes: BytesFormat::of(driver.state()),
            out: Vec::new(),
            name: prefix.to_string(),
            stack: Vec::new(),
        };
        driver.drive(|event, state| writer.event(event, state))?;
        writer.check_names(prefix.len())?;
        Ok(writer.out)
    }
}

/// Builds a [`SerializerConfig`].
///
/// The methods have the names of the setters of [`SerializerConfig`] (without `set_`).
#[derive(Debug, Clone)]
#[must_use]
pub struct SerializerConfigBuilder {
    value: SerializerConfig,
}

impl SerializerConfigBuilder {
    /// Creates a builder that starts with the default.
    pub const fn new() -> SerializerConfigBuilder {
        SerializerConfigBuilder {
            value: SerializerConfig::new(),
        }
    }

    /// Sets the separator of nested keys.
    ///
    /// See [`SerializerConfig::set_separator`].
    pub const fn separator(mut self, separator: &'static str) -> SerializerConfigBuilder {
        self.value.set_separator(separator);
        self
    }

    /// Sets how keys map onto names.
    ///
    /// See [`SerializerConfig::set_case`].
    pub const fn case(mut self, case: Case) -> SerializerConfigBuilder {
        self.value.set_case(case);
        self
    }

    /// Sets the context the values are serialized in.
    ///
    /// See [`SerializerConfig::set_context`].
    pub fn context(mut self, context: deser_core::Context) -> SerializerConfigBuilder {
        self.value.set_context(context);
        self
    }

    /// Returns the built [`SerializerConfig`].
    pub const fn build(self) -> SerializerConfig {
        // the value cannot be moved out of the builder in a const fn as the
        // builder needs dropping (the context has a destructor)
        // SAFETY: the value is read once and the builder is forgotten
        let value = unsafe { core::ptr::read(&self.value) };
        core::mem::forget(self);
        value
    }
}

impl Default for SerializerConfigBuilder {
    fn default() -> SerializerConfigBuilder {
        SerializerConfigBuilder::new()
    }
}

/// Serializes a value into environment variables with a prefix.
///
/// This uses the default [`SerializerConfig`], see there for more
/// information.  The variables are name-value pairs which can for instance
/// be passed to a child process:
///
/// ```no_run
/// use std::process::Command;
///
/// #[derive(deser::Serialize)]
/// struct Config {
///     port: u16,
/// }
///
/// let vars = deser_env::to_vars("APP_", &Config { port: 80 }).unwrap();
/// Command::new("server").envs(vars).spawn().unwrap();
/// ```
pub fn to_vars<T: Serialize + ?Sized>(
    prefix: &str,
    value: &T,
) -> Result<Vec<(String, String)>, Error> {
    SerializerConfig::new().to_vars(prefix, value)
}

/// A container that is being written.
enum Frame {
    /// A map, with the length of its name.
    Map { prefix: usize },
    /// A sequence, with the length of its name and the index of the next
    /// element.
    Seq { prefix: usize, index: usize },
}

impl Frame {
    fn prefix(&self) -> usize {
        match *self {
            Frame::Map { prefix } | Frame::Seq { prefix, .. } => prefix,
        }
    }
}

/// Writes the events of a value.
struct Writer<'c> {
    config: &'c SerializerConfig,
    /// How bytes are written (from the state).
    bytes: BytesFormat,
    out: Vec<(String, String)>,
    /// The name of the current value.
    name: String,
    stack: Vec<Frame>,
}

impl Writer<'_> {
    fn event(&mut self, event: Event, state: &State) -> Result<(), Error> {
        match (self.stack.last_mut(), event) {
            (None, Event::MapStart(_)) => self.stack.push(Frame::Map {
                prefix: self.name.len(),
            }),
            (None, Event::Atom(Atom::Null)) => {}
            (None, _) => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "environment variables hold maps (like structs)",
                ));
            }

            (Some(Frame::Map { .. }), Event::MapEnd) => {
                self.stack.pop();
            }
            (Some(&mut Frame::Map { prefix }), event) => {
                if state.is_map_key() {
                    let key = match event {
                        Event::Atom(ref atom) => key_text(atom)?,
                        _ => return Err(unsupported_key()),
                    };
                    if key.is_empty() {
                        return Err(Error::new(
                            ErrorKind::UnsupportedType,
                            "keys of environment variables must not be empty",
                        ));
                    }
                    if !self.config.separator.is_empty() && key.contains(self.config.separator) {
                        return Err(Error::new(
                            ErrorKind::UnsupportedType,
                            format!("key {:?} contains the separator", key),
                        ));
                    }
                    self.name.truncate(prefix);
                    // the keys of the top level map follow the prefix
                    if self.stack.len() > 1 {
                        self.name.push_str(self.config.separator);
                    }
                    self.push_key(&key);
                } else {
                    self.value(event, false)?;
                }
            }

            (Some(Frame::Seq { .. }), Event::SeqEnd) => {
                self.stack.pop();
            }
            (
                Some(&mut Frame::Seq {
                    prefix,
                    ref mut index,
                }),
                event,
            ) => {
                let element = *index;
                *index += 1;
                self.name.truncate(prefix);
                self.name.push_str(self.config.separator);
                self.name.push_str(&element.to_string());
                self.value(event, true)?;
            }
        }
        Ok(())
    }

    /// Appends a key to the name.
    fn push_key(&mut self, key: &str) {
        match self.config.case {
            Case::Upper => self
                .name
                .extend(key.chars().map(|c| c.to_ascii_uppercase())),
            Case::Preserve => self.name.push_str(key),
        }
    }

    /// Checks that no name is written twice and that no name is nested
    /// in another one.
    ///
    /// Different keys can have the same name, for instance `a` and `A`
    /// (names are uppercased) or `1` and `"1"`.
    fn check_names(&self, prefix: usize) -> Result<(), Error> {
        let normalize = |name: &str| match self.config.case {
            Case::Upper => name.to_ascii_lowercase(),
            Case::Preserve => name.to_string(),
        };
        let names: HashMap<String, &str> = self
            .out
            .iter()
            .map(|(name, _)| (normalize(name), name.as_str()))
            .collect();
        if names.len() != self.out.len() {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "different keys have the same name",
            ));
        }
        let mut segments = Vec::new();
        for name in names.keys() {
            segments.clear();
            crate::de::split_name(&name[prefix..], self.config.separator, &mut segments);
            for &(_, end) in &segments[..segments.len() - 1] {
                if let Some(parent) = names.get(&name[..prefix + end]) {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        format!("the variable {parent:?} has a value and nested variables"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Checks that the current name splits into its keys again.
    ///
    /// Keys do not contain the separator, but the end of a key and the
    /// separator after it can (`A_` and `__` are `A___`, which splits into
    /// `A` and `_`).
    fn check_name(&self) -> Result<(), Error> {
        let Some(base) = self.stack.first().map(Frame::prefix) else {
            return Ok(());
        };
        let separator = self.config.separator;
        let mut segments = Vec::new();
        crate::de::split_name(&self.name[base..], separator, &mut segments);
        let expected = self.stack.iter().enumerate().map(|(index, frame)| {
            let start = match index {
                0 => frame.prefix(),
                _ => frame.prefix() + separator.len(),
            };
            let end = match self.stack.get(index + 1) {
                Some(next) => next.prefix(),
                None => self.name.len(),
            };
            (start - base, end - base)
        });
        if segments.iter().copied().eq(expected) {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::UnsupportedType,
                format!(
                    "the name {:?} does not split into its keys at the separator",
                    self.name
                ),
            ))
        }
    }

    /// Writes a value for the current name.
    fn value(&mut self, event: Event, in_seq: bool) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => {
                let value = match value_text(&atom, self.bytes)? {
                    Some(value) => value,
                    // nulls in sequences are empty values to keep the
                    // positions of the other values
                    None if in_seq => Cow::Borrowed(""),
                    None => return Ok(()),
                };
                self.check_name()?;
                self.out.push((self.name.clone(), value.into_owned()));
            }
            Event::MapStart(_) | Event::SeqStart(_) if self.config.separator.is_empty() => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "nested maps and sequences require a separator",
                ));
            }
            Event::MapStart(_) => self.stack.push(Frame::Map {
                prefix: self.name.len(),
            }),
            Event::SeqStart(_) => self.stack.push(Frame::Seq {
                prefix: self.name.len(),
                index: 0,
            }),
            Event::MapEnd | Event::SeqEnd => unreachable!("ends are handled by the frames"),
        }
        Ok(())
    }
}

/// Returns the text of a map key.
fn key_text<'a>(atom: &'a Atom<'_>) -> Result<Cow<'a, str>, Error> {
    match atom {
        Atom::Null | Atom::Bytes(_) => Err(unsupported_key()),
        atom => value_text(atom, BytesFormat::BASE64)?.ok_or_else(unsupported_key),
    }
}

/// Returns the text of a value, `None` for null.
fn value_text<'a>(atom: &'a Atom<'_>, bytes: BytesFormat) -> Result<Option<Cow<'a, str>>, Error> {
    Ok(Some(match *atom {
        Atom::Null => return Ok(None),
        // values whose type was inferred from text are written as value
        Atom::Implicit(ref value) => {
            return Ok(value_text(&value.value().to_atom(), bytes)?
                .map(|text| Cow::Owned(text.into_owned())));
        }
        Atom::Bool(value) => Cow::Borrowed(if value { "true" } else { "false" }),
        Atom::Str(ref value) | Atom::Lexical(ref value) => Cow::Borrowed(&**value),
        Atom::Char(value) => Cow::Owned(value.to_string()),
        Atom::U64(value) => Cow::Owned(value.to_string()),
        Atom::I64(value) => Cow::Owned(value.to_string()),
        Atom::F32(value) => Cow::Owned(zmij::Buffer::new().format(value).into()),
        Atom::F64(value) => Cow::Owned(zmij::Buffer::new().format(value).into()),
        Atom::Bytes(ref value) => {
            let format = value.fallback.copied().unwrap_or(bytes);
            Cow::Owned(
                format
                    .encode(value)
                    .or_else(|| BytesFormat::BASE64.encode(value))
                    .unwrap_or_default(),
            )
        }
        Atom::Ext(ref ext) => {
            if let Some(number) = ext.downcast_value_ref::<Number>() {
                // numbers keep their text
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
                            format!("environment variables do not support {}", ext.name()),
                        ));
                    }
                    fallback => match value_text(&fallback, bytes)? {
                        Some(text) => Cow::Owned(text.into_owned()),
                        None => return Ok(None),
                    },
                }
            }
        }
        _ => {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("environment variables do not support {}", atom.name()),
            ));
        }
    }))
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "keys of environment variables must be strings, numbers or booleans",
    )
}
