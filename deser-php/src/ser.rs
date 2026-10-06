use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use deser_core::State;
use deser_core::ext::{BigInt, ExtValue, Number};
use deser_core::ser::{self, EventSink, SerializeDriver, SerializeRef};
use deser_core::{Atom, Error, ErrorKind, Event, ImplicitValue, Serialize};

use crate::float::{write_f32, write_f64};
use crate::object::{ClassName, PropertyVisibility, Visibility};
use crate::parser::{int_key, is_class_name, is_name};
use crate::reference::{Reference, ReferenceKind};

/// Configures how values are serialized.
///
/// The output is what PHP's `serialize` writes for the same values (see
/// the [crate documentation](crate#serialization)).  The only option is
/// the [`Context`](deser_core::Context) (see
/// [`set_context`](Self::set_context)).
///
/// ```
/// use deser_php::SerializerConfig;
///
/// const CONFIG: SerializerConfig = SerializerConfig::new();
/// assert_eq!(CONFIG.to_vec(&vec![1, 2]).unwrap(), b"a:2:{i:0;i:1;i:1;i:2;}");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerializerConfig {
    context: deser_core::Context,
}

impl SerializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> SerializerConfig {
        SerializerConfig {
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
    /// of the state (see [`Context`](deser_core::Context)).  The
    /// serializers and writers created with the configuration use this
    /// context.  A context set on the driver takes precedence.
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

    /// Serializes the given value.
    pub fn to_vec<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, Error> {
        self.to_vec_ref(SerializeRef::new(&value))
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_vec_with<F, T: Serialize + ?Sized>(
        &self,
        value: &T,
        setup: F,
    ) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(&value);
        setup(&mut driver);
        self.apply_context(&mut driver);
        serialize_driver(&mut driver)
    }

    /// Serializes a value whose type is erased (see
    /// [`to_vec`](Self::to_vec)).
    ///
    /// This is not generic: the code that exists for every type only
    /// erases it.
    fn to_vec_ref(&self, value: SerializeRef<'_>) -> Result<Vec<u8>, Error> {
        let mut driver = SerializeDriver::from_ref(value);
        self.apply_context(&mut driver);
        serialize_driver(&mut driver)
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

/// Serializes values into PHP's serialization format.
///
/// The values are appended to the output one after another (they can be
/// read back with [`Deserializer::iter`](crate::Deserializer::iter)).
///
/// ```
/// use deser_php::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&true).unwrap();
/// serializer.serialize(&"hi").unwrap();
/// assert_eq!(serializer.output(), b"b:1;s:2:\"hi\";");
/// ```
///
/// The serializer is also the stream serializer of the format (see
/// [`StreamSerializer`](ser::StreamSerializer)).  Values are written once
/// they are complete as the lengths of arrays are written in front of
/// them.  To write to a [`Write`](std::io::Write) use
/// [`SerializerConfig::writer`].
#[derive(Debug, Clone, Default)]
pub struct Serializer {
    config: SerializerConfig,
    out: Vec<u8>,
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: SerializerConfig) -> Serializer {
        Serializer {
            config,
            out: Vec::new(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SerializerConfig {
        &self.config
    }

    /// Serializes a value and appends it to the output.
    pub fn serialize<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn serialize_with<F, T: Serialize + ?Sized>(
        &mut self,
        value: &T,
        setup: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the output written so far (that was not cleared).
    pub fn output(&self) -> &[u8] {
        &self.out
    }

    /// Returns the output.
    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        self.config.apply_context(driver);
        let bytes = serialize_driver(driver)?;
        self.out.extend_from_slice(&bytes);
        Ok(())
    }
}

impl ser::StreamSerializer for Serializer {
    fn output(&self) -> &[u8] {
        &self.out
    }

    fn clear_output(&mut self) {
        self.out.clear();
    }
}

#[cfg(feature = "io")]
impl SerializerConfig {
    /// Creates a writer of values (see
    /// [`deser::io::Writer`](deser_core::io::Writer)).
    ///
    /// The values are written one after another, each once it's complete.
    pub fn writer<W: std::io::Write>(&self, writer: W) -> deser_core::io::Writer<W, Serializer> {
        deser_core::io::Writer::new(writer, Serializer::with_config(self.clone()))
    }

    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`].
    pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
        &self,
        writer: W,
        value: &T,
    ) -> Result<(), Error> {
        self.writer(writer).write(value)
    }
}

/// Serializes a value to a writer.
///
/// ```
/// let mut out = Vec::new();
/// deser_php::to_writer(&mut out, &vec!["a", "b"]).unwrap();
/// assert_eq!(out, br#"a:2:{i:0;s:1:"a";i:1;s:1:"b";}"#);
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// Serializes a value into PHP's serialization format.
///
/// This uses the default [`SerializerConfig`].
///
/// ```
/// #[derive(deser::Serialize)]
/// struct Package {
///     name: String,
///     tags: Vec<String>,
/// }
/// let package = Package { name: "deser".into(), tags: vec!["a".into()] };
/// assert_eq!(
///     deser_php::to_vec(&package).unwrap(),
///     br#"a:2:{s:4:"name";s:5:"deser";s:4:"tags";a:1:{i:0;s:1:"a";}}"#
/// );
/// ```
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    SerializerConfig::new().to_vec(value)
}

/// Serializes the value of a driver.
fn serialize_driver(driver: &mut SerializeDriver<'_>) -> Result<Vec<u8>, Error> {
    let mut writer = Writer::default();
    driver.drive_sink(&mut writer)?;
    writer.finish()
}

/// The kind of an open container.
enum Container {
    /// A sequence, written as array with the keys `0`, `1`, ...
    List(u64),
    Array,
    Object(String),
}

/// An open container.
struct Open {
    container: Container,
    /// The index of the header in `Writer::headers`.
    header: usize,
    /// The number of entries.
    count: usize,
    /// `true` if a key comes next (maps only).
    expects_key: bool,
}

/// Writes the events of a value.
///
/// The number of entries is written in front of the entries of arrays and
/// objects.  The headers are inserted once the value is complete.
#[derive(Default)]
struct Writer {
    out: Vec<u8>,
    stack: Vec<Open>,
    /// The headers of the arrays and objects and where they go, in the
    /// order of the output.
    headers: Vec<(usize, Vec<u8>)>,
    /// `true` once the top-level value started.
    started: bool,
    /// For every value that has a number (see [`Reference`]), `true` if
    /// it's an object, references can only refer to earlier values.
    objects: Vec<bool>,
}

impl EventSink for Writer {
    fn event(
        &mut self,
        event: Event<'_>,
        _value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error> {
        let Some(open) = self.stack.last_mut() else {
            if self.started {
                return Err(Error::new(ErrorKind::InvalidState, "unexpected event"));
            }
            self.started = true;
            return self.value(event, state);
        };
        match open.container {
            Container::List(ref mut index) => {
                if event == Event::SeqEnd {
                    return self.close();
                }
                let key = *index;
                *index += 1;
                open.count += 1;
                self.out.extend_from_slice(b"i:");
                push_int(&mut self.out, key as i128);
                self.out.push(b';');
                self.value(event, state)
            }
            _ if open.expects_key => {
                if event == Event::MapEnd {
                    return self.close();
                }
                open.expects_key = false;
                open.count += 1;
                let is_object = matches!(open.container, Container::Object(_));
                let Event::Atom(atom) = event else {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "keys must be integers or strings",
                    ));
                };
                let visibility = state
                    .event::<PropertyVisibility>()
                    .and_then(|visibility| visibility.0.clone());
                self.key(atom, visibility, is_object)
            }
            _ => {
                open.expects_key = true;
                self.value(event, state)
            }
        }
    }
}

impl Writer {
    /// Returns the output with the headers inserted.
    fn finish(self) -> Result<Vec<u8>, Error> {
        if !self.started || !self.stack.is_empty() {
            return Err(Error::new(ErrorKind::InvalidState, "incomplete value"));
        }
        if self.headers.is_empty() {
            return Ok(self.out);
        }
        let len = self
            .headers
            .iter()
            .map(|(_, header)| header.len())
            .sum::<usize>();
        let mut out = Vec::with_capacity(self.out.len() + len);
        let mut pos = 0;
        for (offset, header) in &self.headers {
            out.extend_from_slice(&self.out[pos..*offset]);
            out.extend_from_slice(header);
            pos = *offset;
        }
        out.extend_from_slice(&self.out[pos..]);
        Ok(out)
    }

    /// Writes the first event of a value.
    fn value(&mut self, event: Event<'_>, state: &mut State) -> Result<(), Error> {
        let class = state
            .event::<ClassName>()
            .and_then(|class| class.0.as_ref());
        match event {
            Event::Atom(atom) => match class {
                Some(class) => {
                    let class = class.clone();
                    self.objects.push(true);
                    self.classed_atom(atom, &class)
                }
                None => {
                    // references are numbered when they are written
                    if !matches!(atom, Atom::Ext(ref ext) if ext.is::<Reference>()) {
                        self.objects.push(false);
                    }
                    self.atom(atom)
                }
            },
            Event::MapStart(_) => {
                let container = match class {
                    Some(class) => {
                        check_class(class)?;
                        Container::Object(class.clone())
                    }
                    None => Container::Array,
                };
                self.objects.push(matches!(container, Container::Object(_)));
                self.open(container);
                Ok(())
            }
            Event::SeqStart(_) => {
                if class.is_some() {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "only maps, strings and bytes can have a class",
                    ));
                }
                self.objects.push(false);
                self.open(Container::List(0));
                Ok(())
            }
            Event::MapEnd | Event::SeqEnd => {
                Err(Error::new(ErrorKind::InvalidState, "unexpected end event"))
            }
        }
    }

    fn open(&mut self, container: Container) {
        self.stack.push(Open {
            container,
            header: self.headers.len(),
            count: 0,
            expects_key: true,
        });
        self.headers.push((self.out.len(), Vec::new()));
    }

    /// Closes the innermost container and writes its header.
    fn close(&mut self) -> Result<(), Error> {
        let open = self.stack.pop().unwrap();
        if !open.expects_key {
            return Err(Error::new(ErrorKind::InvalidState, "map without value"));
        }
        let header = &mut self.headers[open.header].1;
        match open.container {
            Container::Object(class) => {
                header.extend_from_slice(b"O:");
                push_int(header, class.len() as i128);
                header.extend_from_slice(b":\"");
                header.extend_from_slice(class.as_bytes());
                header.extend_from_slice(b"\":");
            }
            _ => header.extend_from_slice(b"a:"),
        }
        push_int(header, open.count as i128);
        header.extend_from_slice(b":{");
        self.out.push(b'}');
        Ok(())
    }

    fn atom(&mut self, atom: Atom<'_>) -> Result<(), Error> {
        match atom {
            Atom::Null => self.out.extend_from_slice(b"N;"),
            Atom::Bool(value) => self
                .out
                .extend_from_slice(if value { b"b:1;" } else { b"b:0;" }),
            Atom::U64(value) => self.int(value.into())?,
            Atom::I64(value) => self.int(value.into())?,
            Atom::F32(value) => {
                let mut text = String::new();
                write_f32(value, &mut text);
                self.float(&text);
            }
            Atom::F64(value) => {
                let mut text = String::new();
                write_f64(value, &mut text);
                self.float(&text);
            }
            Atom::Str(value) | Atom::Lexical(value) => self.string(value.as_bytes()),
            Atom::Char(value) => self.string(value.encode_utf8(&mut [0; 4]).as_bytes()),
            Atom::Bytes(value) => self.string(value.data()),
            // values whose type was inferred from text are written as value
            Atom::Implicit(value) => return self.atom(value.value().to_atom()),
            Atom::Ext(ref ext) => return self.ext(ext),
            _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        }
        Ok(())
    }

    #[cold]
    fn ext(&mut self, ext: &ExtValue<'_>) -> Result<(), Error> {
        if let Some(reference) = ext.downcast_ref::<Reference>() {
            // like the deserializer, references refer to an earlier value,
            // `r:` to an object.  `r:` has a number itself, `R:` does not.
            let target = usize::try_from(reference.number())
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|index| self.objects.get(index));
            match (reference.kind(), target) {
                (ReferenceKind::Object, Some(true)) => self.objects.push(true),
                (ReferenceKind::Value, Some(_)) => {}
                _ => {
                    return Err(Error::new(
                        ErrorKind::InvalidValue,
                        "the reference does not refer to an earlier value",
                    ));
                }
            }
            self.out.extend_from_slice(match reference.kind() {
                ReferenceKind::Object => b"r:",
                ReferenceKind::Value => b"R:",
            });
            push_int(&mut self.out, reference.number().into());
            self.out.push(b';');
            return Ok(());
        }
        if let Some(value) = ext_int(ext) {
            return self.int(value);
        }
        // numbers from text formats are integers if their text is one
        if let Some(value) = ext.downcast_value_ref::<Number>() {
            return match value.as_str().parse::<i64>() {
                Ok(value) => self.int(value.into()),
                Err(_) => self.atom(Atom::F64(value.value())),
            };
        }
        match ext.fallback() {
            Atom::Ext(_) => Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("PHP's serialization format does not support {}", ext.name()),
            )),
            fallback => self.atom(fallback),
        }
    }

    /// Writes an atom with a class: an enum case or a custom serialized
    /// object.
    fn classed_atom(&mut self, atom: Atom<'_>, class: &str) -> Result<(), Error> {
        check_class(class)?;
        match atom {
            Atom::Str(ref case) | Atom::Lexical(ref case) => {
                if !is_name(case.as_bytes()) {
                    return Err(Error::new(ErrorKind::InvalidValue, "invalid enum case"));
                }
                self.out.extend_from_slice(b"E:");
                push_int(&mut self.out, (class.len() + 1 + case.len()) as i128);
                self.out.extend_from_slice(b":\"");
                self.out.extend_from_slice(class.as_bytes());
                self.out.push(b':');
                self.out.extend_from_slice(case.as_bytes());
                self.out.extend_from_slice(b"\";");
            }
            Atom::Bytes(ref payload) => {
                self.out.extend_from_slice(b"C:");
                push_int(&mut self.out, class.len() as i128);
                self.out.extend_from_slice(b":\"");
                self.out.extend_from_slice(class.as_bytes());
                self.out.extend_from_slice(b"\":");
                push_int(&mut self.out, payload.data().len() as i128);
                self.out.extend_from_slice(b":{");
                self.out.extend_from_slice(payload.data());
                self.out.push(b'}');
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "only maps, strings and bytes can have a class",
                ));
            }
        }
        Ok(())
    }

    fn int(&mut self, value: i128) -> Result<(), Error> {
        if i64::try_from(value).is_err() {
            return Err(out_of_range());
        }
        self.out.extend_from_slice(b"i:");
        push_int(&mut self.out, value);
        self.out.push(b';');
        Ok(())
    }

    fn float(&mut self, text: &str) {
        self.out.extend_from_slice(b"d:");
        self.out.extend_from_slice(text.as_bytes());
        self.out.push(b';');
    }

    fn string(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(b"s:");
        push_int(&mut self.out, bytes.len() as i128);
        self.out.extend_from_slice(b":\"");
        self.out.extend_from_slice(bytes);
        self.out.extend_from_slice(b"\";");
    }

    /// Writes a key.
    ///
    /// Arrays have integer and string keys, strings which are the text of
    /// an integer are integers (like in PHP).  The keys of objects are the
    /// names of properties, which are strings (with the prefix of their
    /// visibility).
    fn key(
        &mut self,
        atom: Atom<'_>,
        visibility: Option<Visibility>,
        is_object: bool,
    ) -> Result<(), Error> {
        let mut key = key_of(atom)?;
        if let Some(visibility) = visibility.filter(|x| *x != Visibility::Public) {
            let name = match key {
                Key::Int(value) => Cow::Owned(value.to_string().into_bytes()),
                Key::Str(name) => name,
            };
            let mut mangled = Vec::with_capacity(name.len() + 3);
            mangled.push(0);
            match visibility {
                Visibility::Private(class) => mangled.extend_from_slice(class.as_bytes()),
                _ => mangled.push(b'*'),
            }
            mangled.push(0);
            mangled.extend_from_slice(&name);
            key = Key::Str(Cow::Owned(mangled));
        }
        match key {
            Key::Int(value) if !is_object => self.int(value.into()),
            Key::Int(value) => {
                self.string(value.to_string().as_bytes());
                Ok(())
            }
            Key::Str(name) => {
                match int_key(&name).filter(|_| !is_object) {
                    Some(value) => self.int(value.into())?,
                    None => self.string(&name),
                }
                Ok(())
            }
        }
    }
}

/// A key of an array or object.
enum Key<'a> {
    Int(i64),
    Str(Cow<'a, [u8]>),
}

/// Converts the atom of a key.
fn key_of(atom: Atom<'_>) -> Result<Key<'_>, Error> {
    Ok(match atom {
        Atom::Str(value) | Atom::Lexical(value) => Key::Str(match value.into_cow() {
            Cow::Borrowed(value) => Cow::Borrowed(value.as_bytes()),
            Cow::Owned(value) => Cow::Owned(value.into_bytes()),
        }),
        Atom::Char(value) => Key::Str(Cow::Owned(value.to_string().into_bytes())),
        Atom::Bytes(value) => Key::Str(value.into_data()),
        Atom::U64(value) => Key::Int(i64::try_from(value).map_err(|_| out_of_range())?),
        Atom::I64(value) => Key::Int(value),
        // like PHP: `true` is the key `1`
        Atom::Bool(value) => Key::Int(value.into()),
        Atom::Implicit(value) => {
            let (text, value) = value.into_parts();
            match value {
                ImplicitValue::U64(value) => {
                    Key::Int(i64::try_from(value).map_err(|_| out_of_range())?)
                }
                ImplicitValue::I64(value) => Key::Int(value),
                _ => Key::Str(Cow::Owned(text.into_owned().into_bytes())),
            }
        }
        Atom::Ext(ref ext) => {
            if let Some(value) = ext_int(ext) {
                Key::Int(i64::try_from(value).map_err(|_| out_of_range())?)
            } else if ext.is::<Reference>() {
                return Err(unsupported_key());
            } else {
                match ext.fallback() {
                    Atom::Ext(_) => return Err(unsupported_key()),
                    fallback => match key_of(fallback)? {
                        Key::Int(value) => Key::Int(value),
                        Key::Str(name) => Key::Str(Cow::Owned(name.into_owned())),
                    },
                }
            }
        }
        _ => return Err(unsupported_key()),
    })
}

/// Returns the value of the extension types for integers.
fn ext_int(ext: &ExtValue<'_>) -> Option<i128> {
    if let Some(&value) = ext.downcast_ref::<u128>() {
        // out of range values fail
        return Some(i128::try_from(value).unwrap_or(i128::MAX));
    }
    if let Some(&value) = ext.downcast_ref::<i128>() {
        return Some(value);
    }
    ext.downcast_ref::<BigInt>().map(|value| {
        value.to_i128().unwrap_or(if value.is_negative() {
            i128::MIN
        } else {
            i128::MAX
        })
    })
}

fn check_class(class: &str) -> Result<(), Error> {
    if !is_class_name(class.as_bytes()) {
        return Err(Error::new(
            ErrorKind::InvalidValue,
            format!("invalid class name {:?}", class),
        ));
    }
    Ok(())
}

/// Appends the decimal text of an integer.
fn push_int(out: &mut Vec<u8>, value: i128) {
    let mut buf = [0u8; 40];
    let mut pos = buf.len();
    let negative = value < 0;
    let mut rest = value.unsigned_abs();
    loop {
        pos -= 1;
        buf[pos] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if negative {
        pos -= 1;
        buf[pos] = b'-';
    }
    out.extend_from_slice(&buf[pos..]);
}

#[cold]
fn out_of_range() -> Error {
    Error::new(
        ErrorKind::OutOfRange,
        "integer out of range for PHP's serialization format",
    )
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "keys must be integers or strings",
    )
}
