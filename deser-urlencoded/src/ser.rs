use std::borrow::Cow;

use deser_core::__format::{Float, format_finite};
use deser_core::adapters::BytesFormat;
use deser_core::ext::Number;
use deser_core::ser::{self, PausableSink, SerializeDriver};
use deser_core::{Atom, Error, ErrorKind, Event, Serialize, State};

use crate::Nesting;
use crate::encoding::encode;

/// How sequences are written.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_urlencoded::{ArrayFormat, SerializerConfig};
///
/// let value = BTreeMap::from([("a", vec![1, 2])]);
/// let with = |arrays| {
///     SerializerConfig::new().arrays(arrays).to_string(&value).unwrap()
/// };
/// assert_eq!(with(ArrayFormat::Repeat), "a=1&a=2");
/// assert_eq!(with(ArrayFormat::Brackets), "a%5B%5D=1&a%5B%5D=2");
/// assert_eq!(with(ArrayFormat::Indices), "a%5B0%5D=1&a%5B1%5D=2");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ArrayFormat {
    /// The key is repeated for every element (`a=1&a=2`).
    ///
    /// This is what HTML forms send for `<select multiple>` and what
    /// `URLSearchParams` produces.  The elements have to be atoms.
    #[default]
    Repeat,
    /// The key is repeated with empty brackets (`a[]=1&a[]=2`).
    ///
    /// The elements have to be atoms.
    Brackets,
    /// The key is repeated with the index of the element (`a[0]=1&a[1]=2`,
    /// with [`Nesting::Dots`] `a.0=1&a.1=2`).
    ///
    /// This is the only format that supports sequences of maps and
    /// sequences.
    Indices,
}

/// Configures how values are serialized to query strings.
///
/// The value has to serialize to a map (for instance a struct or a map
/// type), or to a sequence of key-value pairs (like `Vec<(&str, &str)>`).
/// Null values (like `None`) of map entries are skipped, null values in
/// sequences are written as empty values.  Maps and sequences that are
/// empty are not written as query strings cannot represent them.
///
/// Keys and values are percent-encoded like `application/x-www-form-urlencoded`
/// (ASCII alphanumerics and `*-._` are kept, space is written as `+`).
/// Numbers are written with the shortest text that reads back as the same
/// value, booleans as `true` and `false` and bytes as base64 (see
/// [`bytes`](Self::bytes)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    arrays: ArrayFormat,
    nesting: Nesting,
    space_as_plus: bool,
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
            arrays: ArrayFormat::Repeat,
            nesting: Nesting::Brackets,
            space_as_plus: true,
            bytes: BytesFormat::BASE64,
        }
    }

    /// Sets how sequences are written.
    ///
    /// The default is [`ArrayFormat::Repeat`].
    pub const fn arrays(mut self, format: ArrayFormat) -> SerializerConfig {
        self.arrays = format;
        self
    }

    /// Sets how the keys of nested maps are written.
    ///
    /// The default is [`Nesting::Brackets`] (`a[b]=1`), with
    /// [`Nesting::Dots`] they are written as `a.b=1`.  With
    /// [`Nesting::Flat`] nested maps are an error.
    pub const fn nesting(mut self, nesting: Nesting) -> SerializerConfig {
        self.nesting = nesting;
        self
    }

    /// Sets if spaces are written as `+` (the default) or as `%20`.
    pub const fn space_as_plus(mut self, yes: bool) -> SerializerConfig {
        self.space_as_plus = yes;
        self
    }

    /// Sets how bytes are represented.
    ///
    /// By default bytes are written as base64 ([`BytesFormat::BASE64`]).
    /// Values can request a different format (see [bytes](deser_core::adapters#bytes))
    /// which takes precedence.
    pub const fn bytes(mut self, format: BytesFormat) -> SerializerConfig {
        self.bytes = format;
        self
    }

    /// Serializes the given value.
    pub fn to_string(&self, value: &dyn Serialize) -> Result<String, Error> {
        self.to_string_with(value, |_| {})
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_string_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<String, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        let mut out = String::new();
        self.serialize_driver(&mut driver, &mut out)?;
        Ok(out)
    }

    /// Creates the writer of a value which writes into the output.
    pub(crate) fn writer(&self, out: String) -> Writer {
        Writer {
            config: self.clone(),
            separate: false,
            out,
            key: String::new(),
            stack: Vec::new(),
            limit: usize::MAX,
        }
    }

    /// Serializes (a part of) the value of a driver and appends it to the
    /// output.
    ///
    /// The progress of the value is kept in `value` (see
    /// `Encoder::encode_incremental`), `true` is returned once the value is
    /// complete.
    #[cfg_attr(not(feature = "io"), allow(dead_code))]
    pub(crate) fn serialize_part(
        &self,
        value: &mut Option<Box<Writer>>,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<bool, Error> {
        let mut writer = value
            .take()
            .unwrap_or_else(|| Box::new(self.writer(String::new())));
        // the writer writes into an empty output directly, otherwise its
        // output is appended
        let adopt = out.is_empty();
        if adopt {
            writer.out = String::from_utf8(std::mem::take(out)).unwrap();
        }
        // after an error the value is abandoned, its writer is dropped
        let done = if limit == usize::MAX {
            driver.drive(|event, state| writer.event(event, state))?;
            true
        } else {
            writer.limit = limit;
            driver.drive_until(&mut *writer)?
        };
        let output = std::mem::take(&mut writer.out).into_bytes();
        if adopt {
            *out = output;
        } else {
            out.extend_from_slice(&output);
        }
        if !done {
            *value = Some(writer);
        }
        Ok(done)
    }

    /// Serializes the value of a driver and appends it to the output.
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
        out: &mut String,
    ) -> Result<(), Error> {
        let mut writer = self.writer(String::new());
        driver.drive(|event, state| writer.event(event, state))?;
        // the parameters of more than one value are joined
        if !out.is_empty() && !writer.out.is_empty() {
            out.push('&');
        }
        out.push_str(&writer.out);
        Ok(())
    }
}

/// Serializes values into query strings.
///
/// More than one value can be serialized, their parameters are joined.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_urlencoded::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&BTreeMap::from([("a", 1)])).unwrap();
/// serializer.serialize(&BTreeMap::from([("b", 2)])).unwrap();
/// assert_eq!(serializer.finish(), "a=1&b=2");
/// ```
#[derive(Debug, Clone)]
pub struct Serializer {
    config: SerializerConfig,
    out: String,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(&SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: &SerializerConfig) -> Serializer {
        Serializer {
            config: config.clone(),
            out: String::new(),
        }
    }

    /// Serializes a value.
    ///
    /// If the value fails to serialize, nothing is written.
    pub fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn serialize_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the output written so far.
    pub fn output(&self) -> &str {
        &self.out
    }

    /// Returns the output.
    pub fn finish(self) -> String {
        self.out
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        self.config.serialize_driver(driver, &mut self.out)
    }
}

/// Serializes a value to a query string.
///
/// This uses the default [`SerializerConfig`], see there for more
/// information.
///
/// ```
/// #[derive(deser::Serialize)]
/// struct Params {
///     cursor: Option<usize>,
///     per_page: Option<usize>,
///     username: String,
///     filter: Vec<&'static str>,
/// }
///
/// let params = Params {
///     cursor: Some(42),
///     per_page: None,
///     username: "boxdot".into(),
///     filter: vec!["new", "blocked"],
/// };
/// assert_eq!(
///     deser_urlencoded::to_string(&params).unwrap(),
///     "cursor=42&username=boxdot&filter=new&filter=blocked"
/// );
/// ```
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// A container that is being written.
enum Frame {
    /// A map, with the length of the key of the map.
    Map { prefix: usize },
    /// A sequence, with the length of its key and the index of the next
    /// element.
    Seq { prefix: usize, index: usize },
    /// A sequence of key-value pairs at the top level.
    Pairs,
    /// A key-value pair: 0 if the key is expected next, 1 for the value, 2
    /// for the end.
    Pair(u8),
}

/// Writes the events of a value.
pub(crate) struct Writer {
    config: SerializerConfig,
    /// `true` if the next parameter needs a separator.
    separate: bool,
    out: String,
    /// The key of the current value (not encoded).
    key: String,
    stack: Vec<Frame>,
    /// The driver is paused once the output is this long.
    limit: usize,
}

impl PausableSink for Writer {
    fn event(
        &mut self,
        event: Event<'_>,
        _value: &dyn Serialize,
        state: &mut State,
    ) -> Result<(), Error> {
        Writer::event(self, event, state)
    }

    fn pause(&mut self) -> bool {
        // the output is only appended to
        self.out.len() >= self.limit
    }
}

impl Writer {
    fn event(&mut self, event: Event, state: &State) -> Result<(), Error> {
        match (self.stack.last_mut(), event) {
            (None, Event::MapStart(_)) => self.stack.push(Frame::Map { prefix: 0 }),
            (None, Event::SeqStart(_)) => self.stack.push(Frame::Pairs),
            (None, Event::Atom(Atom::Null)) => {}
            (None, _) => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "query strings hold maps or sequences of key-value pairs",
                ));
            }

            (Some(Frame::Map { .. }), Event::MapEnd) => {
                self.stack.pop();
            }
            (Some(&mut Frame::Map { prefix }), event) => {
                if state.is_map_key() {
                    let key = match event {
                        Event::Atom(ref atom) => self.key_text(atom)?,
                        _ => return Err(unsupported_key()),
                    };
                    self.key.truncate(prefix);
                    if prefix > 0 {
                        self.push_nested(&key);
                    } else {
                        self.key.push_str(&key);
                    }
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
                self.key.truncate(prefix);
                match self.config.arrays {
                    ArrayFormat::Repeat => {}
                    ArrayFormat::Brackets => self.key.push_str("[]"),
                    ArrayFormat::Indices => self.push_nested(&element.to_string()),
                }
                if self.config.arrays != ArrayFormat::Indices
                    && matches!(event, Event::MapStart(_) | Event::SeqStart(_))
                {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "sequences of maps or sequences require ArrayFormat::Indices",
                    ));
                }
                self.value(event, true)?;
            }

            (Some(Frame::Pairs), Event::SeqStart(_)) => self.stack.push(Frame::Pair(0)),
            (Some(Frame::Pairs), Event::SeqEnd) => {
                self.stack.pop();
            }
            (Some(Frame::Pair(state @ 0)), Event::Atom(ref atom)) => {
                *state = 1;
                let key = self.key_text(atom)?;
                self.key.clear();
                self.key.push_str(&key);
            }
            (Some(Frame::Pair(state @ 1)), event) => {
                *state = 2;
                self.value(event, false)?;
            }
            (Some(Frame::Pair(2)), Event::SeqEnd) => {
                self.stack.pop();
            }
            (Some(Frame::Pairs | Frame::Pair(_)), _) => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "sequences at the top level must hold key-value pairs",
                ));
            }
        }
        Ok(())
    }

    /// Writes a value for the current key.
    fn value(&mut self, event: Event, in_seq: bool) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => {
                let value = match self.value_text(&atom)? {
                    Some(value) => value,
                    // nulls in sequences are empty values to keep the
                    // positions of the other values
                    None if in_seq => Cow::Borrowed(""),
                    None => return Ok(()),
                };
                if self.separate {
                    self.out.push('&');
                }
                self.separate = true;
                encode(
                    self.key.as_bytes(),
                    self.config.space_as_plus,
                    &mut self.out,
                );
                self.out.push('=');
                encode(value.as_bytes(), self.config.space_as_plus, &mut self.out);
            }
            Event::MapStart(_) => {
                if self.config.nesting == Nesting::Flat {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "nested maps are not supported with Nesting::Flat",
                    ));
                }
                self.stack.push(Frame::Map {
                    prefix: self.key.len(),
                });
            }
            Event::SeqStart(_) => self.stack.push(Frame::Seq {
                prefix: self.key.len(),
                index: 0,
            }),
            Event::MapEnd | Event::SeqEnd => unreachable!("ends are handled by the frames"),
        }
        Ok(())
    }

    /// Appends a nested key (or index) to the current key.
    fn push_nested(&mut self, key: &str) {
        match self.config.nesting {
            Nesting::Dots => {
                self.key.push('.');
                self.key.push_str(key);
            }
            Nesting::Brackets | Nesting::Flat => {
                self.key.push('[');
                self.key.push_str(key);
                self.key.push(']');
            }
        }
    }

    /// Returns the text of a map key.
    fn key_text<'a>(&self, atom: &'a Atom<'_>) -> Result<Cow<'a, str>, Error> {
        match atom {
            Atom::Null | Atom::Bytes(_) => Err(unsupported_key()),
            atom => self.value_text(atom)?.ok_or_else(unsupported_key),
        }
    }

    /// Returns the text of a value, `None` for null.
    fn value_text<'a>(&self, atom: &'a Atom<'_>) -> Result<Option<Cow<'a, str>>, Error> {
        Ok(Some(match *atom {
            Atom::Null => return Ok(None),
            // values whose type was inferred from text are written as value
            Atom::Implicit(ref value) => {
                return Ok(self
                    .value_text(&value.value().to_atom())?
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
                                format!("query strings do not support {}", ext.name()),
                            ));
                        }
                        fallback => match self.value_text(&fallback)? {
                            Some(text) => Cow::Owned(text.into_owned()),
                            None => return Ok(None),
                        },
                    }
                }
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("query strings do not support {}", atom.name()),
                ));
            }
        }))
    }
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "keys of query strings must be strings, numbers or booleans",
    )
}

/// Returns the text of a float.
///
/// Finite floats have the shortest text that reads back as the same value
/// of their type, like in the other formats.  The others are `NaN`, `inf`
/// and `-inf`.
fn float_text<F: Float>(value: F) -> String {
    if value.is_finite() {
        format_finite(value)
    } else {
        value.to_f64().to_string()
    }
}
