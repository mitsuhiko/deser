use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use deser_core::State;
use deser_core::ext::{BigInt, ExtValue, Number};
use deser_core::ser::{self, EventSink, SerializeDriver, SerializeRef};
use deser_core::{Atom, Error, ErrorKind, Event, Serialize};

use crate::compat;
use crate::types::{ClassData, Form, FormData, Global, Kind, KindData, Reference, SharedIdData};
use crate::vm::HIGHEST_PROTOCOL;

/// The protocol written by default.
const DEFAULT_PROTOCOL: u8 = 4;

/// Configures how values are serialized.
///
/// The output can be read with Python's `pickle.loads` (see the
/// [crate documentation](crate#serialization)).  The options are the
/// [protocol](Self::set_protocol) and the [`Context`](deser_core::Context).
///
/// ```
/// use deser_pickle::SerializerConfig;
///
/// const CONFIG: SerializerConfig = SerializerConfig::builder().protocol(2).build();
/// assert_eq!(CONFIG.to_vec(&vec![1, 2]).unwrap(), b"\x80\x02](K\x01K\x02e.");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    context: deser_core::Context,
    protocol: u8,
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
            context: deser_core::Context::new(),
            protocol: DEFAULT_PROTOCOL,
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

    /// Sets the protocol that is written.
    ///
    /// Protocols 2 to 5 are supported, the default is 4 (which Python
    /// reads since 3.4).  Protocol 2 can be read by Python 2.  Serializing
    /// fails with other protocols.
    pub fn set_protocol(&mut self, protocol: u8) {
        self.protocol = protocol;
    }

    /// Returns the protocol that is written.
    pub fn protocol(&self) -> u8 {
        self.protocol
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
        serialize_driver(&mut driver, self.protocol)
    }

    /// Serializes a value whose type is erased (see
    /// [`to_vec`](Self::to_vec)).
    fn to_vec_ref(&self, value: SerializeRef<'_>) -> Result<Vec<u8>, Error> {
        let mut driver = SerializeDriver::from_ref(value);
        self.apply_context(&mut driver);
        serialize_driver(&mut driver, self.protocol)
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

    /// Sets the protocol that is written.
    ///
    /// See [`SerializerConfig::set_protocol`].
    pub const fn protocol(mut self, protocol: u8) -> SerializerConfigBuilder {
        self.value.protocol = protocol;
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

/// Serializes values into pickles.
///
/// Every value is a pickle of its own, they are appended to the output one
/// after another (they can be read back with
/// [`Deserializer::iter`](crate::Deserializer::iter) or by calling
/// Python's `pickle.load` repeatedly).
///
/// ```
/// use deser_pickle::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&true).unwrap();
/// serializer.serialize(&"hi").unwrap();
/// assert_eq!(serializer.output(), b"\x80\x04\x88.\x80\x04\x8c\x02hi.");
/// ```
///
/// The serializer is also the stream serializer of the format (see
/// [`StreamSerializer`](ser::StreamSerializer)).  To write to a
/// [`Write`](std::io::Write) use [`SerializerConfig::writer`].
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
        let bytes = serialize_driver(driver, self.config.protocol)?;
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
    /// Every value is written as a pickle once it's complete.
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
/// deser_pickle::to_writer(&mut out, &vec!["a", "b"]).unwrap();
/// assert_eq!(out, b"\x80\x04](\x8c\x01a\x8c\x01be.");
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// Serializes a value into a pickle.
///
/// This uses the default [`SerializerConfig`] (protocol 4).
///
/// ```
/// #[derive(deser::Serialize)]
/// struct Package {
///     name: String,
///     tags: Vec<String>,
/// }
/// let package = Package { name: "deser".into(), tags: vec!["a".into()] };
/// assert_eq!(
///     deser_pickle::to_vec(&package).unwrap(),
///     b"\x80\x04}(\x8c\x04name\x8c\x05deser\x8c\x04tags](\x8c\x01aeu."
/// );
/// ```
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    SerializerConfig::new().to_vec(value)
}

/// Serializes the value of a driver.
fn serialize_driver(driver: &mut SerializeDriver<'_>, protocol: u8) -> Result<Vec<u8>, Error> {
    if !(2..=HIGHEST_PROTOCOL).contains(&protocol) {
        return Err(Error::new(
            ErrorKind::Configuration,
            format!("unsupported pickle protocol {}", protocol),
        ));
    }
    let mut writer = Writer {
        out: alloc::vec![0x80, protocol],
        proto: protocol,
        stack: Vec::new(),
        memo: BTreeMap::new(),
        memo_len: 0,
        skip: 0,
        started: false,
    };
    driver.drive_sink(&mut writer)?;
    writer.finish()
}

/// What is written when a container ends.
#[derive(Clone, Copy, PartialEq)]
enum End {
    /// A list or the items of a list-like object (`APPENDS`).
    Appends,
    /// A dict or the items of a dict-like object (`SETITEMS`).
    SetItems,
    /// A set (`ADDITEMS`).
    AddItems,
    /// A tuple (`TUPLE`).
    Tuple,
    /// A frozenset (`FROZENSET`).
    FrozenSet,
    /// The list of a set or frozenset of protocols 2 and 3 (`LIST`).
    List,
    /// A map that must be empty (keyword arguments before protocol 4).
    Empty,
}

/// The opcodes written after the end of a container.
#[derive(Clone, Copy, Default)]
struct Suffix {
    buf: [u8; 4],
    len: u8,
}

impl Suffix {
    fn new(bytes: &[u8]) -> Suffix {
        Suffix::default().with(bytes)
    }

    fn with(mut self, bytes: &[u8]) -> Suffix {
        let len = usize::from(self.len);
        self.buf[len..len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len() as u8;
        self
    }

    fn as_slice(&self) -> &[u8] {
        &self.buf[..usize::from(self.len)]
    }
}

/// An open container.
struct Open {
    end: End,
    suffix: Suffix,
    /// The id to memoize the value with after the suffix (for the values
    /// that are only complete at their end).
    memoize: Option<u64>,
    is_map: bool,
    /// `true` if a key comes next (maps only).
    expects_key: bool,
    /// `true` if the value is (in) a key, which must be hashable.
    in_key: bool,
}

/// Writes the events of a value.
struct Writer {
    out: Vec<u8>,
    proto: u8,
    stack: Vec<Open>,
    /// The memo index of the values with a shared id.
    memo: BTreeMap<u64, u32>,
    memo_len: u32,
    /// The depth of a value that is skipped as it was written before.
    skip: usize,
    /// `true` once the top-level value started.
    started: bool,
}

impl EventSink for Writer {
    fn event(
        &mut self,
        event: Event<'_>,
        _value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error> {
        if self.skip > 0 {
            match event {
                Event::MapStart(_) | Event::SeqStart(_) => self.skip += 1,
                Event::MapEnd | Event::SeqEnd => {
                    self.skip -= 1;
                    if self.skip == 0 {
                        self.value_done();
                    }
                }
                Event::Atom(_) => {}
            }
            return Ok(());
        }
        let Some(open) = self.stack.last() else {
            if self.started {
                return Err(Error::new(ErrorKind::InvalidState, "unexpected event"));
            }
            self.started = true;
            return self.value(event, state, false);
        };
        let in_key = open.in_key || open.is_map && open.expects_key;
        match event {
            Event::MapEnd if open.is_map && open.expects_key => self.close(),
            Event::SeqEnd if !open.is_map => self.close(),
            Event::MapEnd | Event::SeqEnd => {
                Err(Error::new(ErrorKind::InvalidState, "unexpected end event"))
            }
            _ if open.end == End::Empty => Err(Error::new(
                ErrorKind::UnsupportedType,
                "keyword arguments need protocol 4",
            )),
            event => self.value(event, state, in_key),
        }
    }
}

impl Writer {
    fn finish(mut self) -> Result<Vec<u8>, Error> {
        if !self.started || !self.stack.is_empty() || self.skip > 0 {
            return Err(Error::new(ErrorKind::InvalidState, "incomplete value"));
        }
        self.out.push(b'.');
        Ok(self.out)
    }

    /// Called when a value is complete.
    fn value_done(&mut self) {
        if let Some(open) = self.stack.last_mut()
            && open.is_map
        {
            open.expects_key = !open.expects_key;
        }
    }

    /// Writes the first event of a value.
    fn value(&mut self, event: Event<'_>, state: &mut State, in_key: bool) -> Result<(), Error> {
        let class = state.event::<ClassData>().and_then(|x| x.0.clone());
        let form = state.event::<FormData>().and_then(|x| x.0);
        let kind = state.event::<KindData>().and_then(|x| x.0);
        let shared = state.event::<SharedIdData>().and_then(|x| x.0);
        // values with an id are written once and referred to after that
        if let Some(idx) = shared.and_then(|id| self.memo.get(&id).copied()) {
            self.get(idx);
            match event {
                Event::MapStart(_) | Event::SeqStart(_) => self.skip = 1,
                _ => self.value_done(),
            }
            return Ok(());
        }
        let Some(class) = class else {
            return match event {
                Event::Atom(atom) => {
                    self.atom(atom, kind)?;
                    self.value_done();
                    Ok(())
                }
                Event::MapStart(_) => {
                    if in_key {
                        return Err(Error::new(
                            ErrorKind::UnsupportedType,
                            "maps cannot be keys of dicts",
                        ));
                    }
                    self.out.push(b'}');
                    self.memoize(shared);
                    self.out.push(b'(');
                    self.open(End::SetItems, Suffix::default(), None, true, false);
                    Ok(())
                }
                Event::SeqStart(_) => self.open_seq(kind, in_key, shared, Suffix::default(), None),
                Event::MapEnd | Event::SeqEnd => {
                    Err(Error::new(ErrorKind::InvalidState, "unexpected end event"))
                }
            };
        };
        // objects are hashable (by their identity), their values do not
        // need to be
        self.global(&class)?;
        let form = form.unwrap_or(match event {
            Event::MapStart(_) => Form::State,
            Event::SeqStart(_) => match kind {
                None => Form::Items,
                Some(Kind::Tuple) => Form::Arguments,
                Some(_) => Form::Argument,
            },
            _ => Form::Argument,
        });
        match event {
            Event::Atom(atom) => {
                match form {
                    Form::State | Form::Slots => {
                        self.out.extend_from_slice(b")\x81");
                        self.memoize(shared);
                        self.atom(atom, kind)?;
                        self.out.push(b'b');
                    }
                    _ => {
                        self.atom(atom, kind)?;
                        self.out.extend_from_slice(b"\x85R");
                        self.memoize(shared);
                    }
                }
                self.value_done();
                Ok(())
            }
            Event::MapStart(_) => {
                let (end, suffix, late) = match form {
                    Form::State => {
                        self.out.extend_from_slice(b")\x81");
                        self.memoize(shared);
                        self.out.push(b'}');
                        (End::SetItems, Suffix::new(b"b"), None)
                    }
                    Form::Slots => {
                        // the state is `(None, attributes)`
                        self.out.extend_from_slice(b")\x81");
                        self.memoize(shared);
                        self.out.extend_from_slice(b"N}");
                        (End::SetItems, Suffix::new(b"\x86b"), None)
                    }
                    Form::Items => {
                        self.out.extend_from_slice(b")\x81");
                        self.memoize(shared);
                        (End::SetItems, Suffix::default(), None)
                    }
                    Form::Arguments if self.proto >= 4 => {
                        self.out.extend_from_slice(b")}");
                        (End::SetItems, Suffix::new(b"\x92"), shared)
                    }
                    Form::Arguments => {
                        self.out.extend_from_slice(b")\x81");
                        self.memoize(shared);
                        (End::Empty, Suffix::default(), None)
                    }
                    Form::Argument => {
                        self.out.push(b'}');
                        (End::SetItems, Suffix::new(b"\x85R"), shared)
                    }
                };
                if end != End::Empty {
                    self.out.push(b'(');
                }
                self.open(end, suffix, late, true, false);
                Ok(())
            }
            Event::SeqStart(_) => match form {
                Form::State | Form::Slots => {
                    self.out.extend_from_slice(b")\x81");
                    self.memoize(shared);
                    self.open_seq(kind, false, None, Suffix::new(b"b"), None)
                }
                Form::Items => {
                    self.out.extend_from_slice(b")\x81");
                    self.memoize(shared);
                    self.out.push(b'(');
                    let end = match kind {
                        Some(Kind::Set | Kind::FrozenSet) => End::AddItems,
                        _ => End::Appends,
                    };
                    self.open(end, Suffix::default(), None, false, false);
                    Ok(())
                }
                Form::Arguments => {
                    self.out.push(b'(');
                    self.open(End::Tuple, Suffix::new(b"R"), shared, false, false);
                    Ok(())
                }
                Form::Argument => self.open_seq(kind, false, None, Suffix::new(b"\x85R"), shared),
            },
            Event::MapEnd | Event::SeqEnd => {
                Err(Error::new(ErrorKind::InvalidState, "unexpected end event"))
            }
        }
    }

    /// Opens a sequence of a kind.
    ///
    /// `memo` is the id of the sequence, `suffix` is written after it and
    /// `late` is the id to memoize the value with after the suffix.
    fn open_seq(
        &mut self,
        kind: Option<Kind>,
        in_key: bool,
        memo: Option<u64>,
        suffix: Suffix,
        late: Option<u64>,
    ) -> Result<(), Error> {
        let (end, suffix, late) = match (kind, in_key) {
            (Some(Kind::Tuple), _) | (None, true) => (End::Tuple, suffix, memo.or(late)),
            (Some(Kind::FrozenSet), _) | (Some(Kind::Set), true) => {
                if self.proto >= 4 {
                    (End::FrozenSet, suffix, memo.or(late))
                } else {
                    self.global(&Global::new(self.builtins(), "frozenset"))?;
                    (
                        End::List,
                        Suffix::new(b"\x85R").with(suffix.as_slice()),
                        memo.or(late),
                    )
                }
            }
            (Some(Kind::Set), false) => {
                if self.proto >= 4 {
                    self.out.push(0x8f);
                    self.memoize(memo);
                    (End::AddItems, suffix, late)
                } else {
                    self.global(&Global::new(self.builtins(), "set"))?;
                    (
                        End::List,
                        Suffix::new(b"\x85R").with(suffix.as_slice()),
                        memo.or(late),
                    )
                }
            }
            _ => {
                self.out.push(b']');
                self.memoize(memo);
                (End::Appends, suffix, late)
            }
        };
        self.out.push(b'(');
        self.open(end, suffix, late, false, in_key);
        Ok(())
    }

    fn open(&mut self, end: End, suffix: Suffix, memoize: Option<u64>, is_map: bool, in_key: bool) {
        self.stack.push(Open {
            end,
            suffix,
            memoize,
            is_map,
            expects_key: true,
            in_key,
        });
    }

    fn close(&mut self) -> Result<(), Error> {
        let open = self.stack.pop().unwrap();
        self.out.extend_from_slice(match open.end {
            End::Appends => b"e",
            End::SetItems => b"u",
            End::AddItems => b"\x90",
            End::Tuple => b"t",
            End::FrozenSet => b"\x91",
            End::List => b"l",
            End::Empty => b"",
        });
        self.out.extend_from_slice(open.suffix.as_slice());
        self.memoize(open.memoize);
        self.value_done();
        Ok(())
    }

    /// The module of the builtins for the protocol.
    fn builtins(&self) -> &'static str {
        match self.proto {
            2 => "__builtin__",
            _ => "builtins",
        }
    }

    fn memoize(&mut self, id: Option<u64>) {
        let Some(id) = id else {
            return;
        };
        let idx = self.memo_len;
        self.memo_len += 1;
        self.memo.insert(id, idx);
        if self.proto >= 4 {
            self.out.push(0x94);
        } else if idx < 256 {
            self.out.push(b'q');
            self.out.push(idx as u8);
        } else {
            self.out.push(b'r');
            self.out.extend_from_slice(&idx.to_le_bytes());
        }
    }

    fn get(&mut self, idx: u32) {
        if idx < 256 {
            self.out.push(b'h');
            self.out.push(idx as u8);
        } else {
            self.out.push(b'j');
            self.out.extend_from_slice(&idx.to_le_bytes());
        }
    }

    fn global(&mut self, global: &Global) -> Result<(), Error> {
        let (mut module, mut name) = (global.module(), global.name());
        // like Python, the globals of Python 3 are written with the names
        // of Python 2 before protocol 3
        if self.proto < 3
            && let Some((new_module, new_name)) = compat::reverse_fix_import(module, name)
        {
            module = new_module;
            name = new_name.unwrap_or(name);
        }
        if self.proto >= 4 {
            self.string(module)?;
            self.string(name)?;
            self.out.push(0x93);
        } else {
            let valid = |x: &str| !x.is_empty() && !x.contains('\n');
            if !valid(module) || !valid(name) {
                return Err(Error::new(
                    ErrorKind::InvalidValue,
                    "names of globals must not be empty or contain newlines before protocol 4",
                ));
            }
            self.out.push(b'c');
            self.out.extend_from_slice(module.as_bytes());
            self.out.push(b'\n');
            self.out.extend_from_slice(name.as_bytes());
            self.out.push(b'\n');
        }
        Ok(())
    }

    fn atom(&mut self, atom: Atom<'_>, kind: Option<Kind>) -> Result<(), Error> {
        match atom {
            Atom::Null => self.out.push(b'N'),
            Atom::Bool(value) => self.out.push(if value { 0x88 } else { 0x89 }),
            Atom::U64(value) => self.int(value.into()),
            Atom::I64(value) => self.int(value.into()),
            Atom::F32(value) => self.float(value.into()),
            Atom::F64(value) => self.float(value),
            Atom::Str(value) | Atom::Lexical(value) => self.string(value.as_str())?,
            Atom::Char(value) => self.string(value.encode_utf8(&mut [0; 4]))?,
            Atom::Bytes(value) => match kind {
                Some(Kind::ByteArray) => self.bytearray(value.data())?,
                _ => self.bytes(value.data())?,
            },
            // values whose type was inferred from text are written as value
            Atom::Implicit(value) => return self.atom(value.value().to_atom(), kind),
            Atom::Ext(ref ext) => return self.ext(ext, kind),
            _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        }
        Ok(())
    }

    #[cold]
    fn ext(&mut self, ext: &ExtValue<'_>, kind: Option<Kind>) -> Result<(), Error> {
        if let Some(reference) = ext.downcast_ref::<Reference>() {
            return match self.memo.get(&reference.id()) {
                Some(&idx) => {
                    self.get(idx);
                    Ok(())
                }
                None => Err(Error::new(
                    ErrorKind::InvalidValue,
                    "reference to a value that was not written or a tuple that contains itself",
                )),
            };
        }
        if let Some(global) = ext.downcast_ref::<Global>() {
            return self.global(global);
        }
        if let Some(&value) = ext.downcast_ref::<u128>() {
            self.long(&BigInt {
                negative: false,
                magnitude: value.to_be_bytes().to_vec(),
            });
            return Ok(());
        }
        if let Some(&value) = ext.downcast_ref::<i128>() {
            self.long(&BigInt {
                negative: value < 0,
                magnitude: value.unsigned_abs().to_be_bytes().to_vec(),
            });
            return Ok(());
        }
        if let Some(value) = ext.downcast_ref::<BigInt>() {
            self.long(value);
            return Ok(());
        }
        // numbers from text formats are integers if their text is one
        if let Some(value) = ext.downcast_value_ref::<Number>() {
            return match value.as_str().parse::<i64>() {
                Ok(value) => {
                    self.int(value.into());
                    Ok(())
                }
                Err(_) => match value.as_str().parse::<BigInt>() {
                    Ok(value) => {
                        self.long(&value);
                        Ok(())
                    }
                    Err(_) => {
                        self.float(value.value());
                        Ok(())
                    }
                },
            };
        }
        match ext.fallback() {
            Atom::Ext(_) => Err(Error::new(
                ErrorKind::UnsupportedType,
                format!("pickle does not support {}", ext.name()),
            )),
            fallback => self.atom(fallback, kind),
        }
    }

    fn int(&mut self, value: i128) {
        if let Ok(value) = u8::try_from(value) {
            self.out.push(b'K');
            self.out.push(value);
        } else if let Ok(value) = u16::try_from(value) {
            self.out.push(b'M');
            self.out.extend_from_slice(&value.to_le_bytes());
        } else if let Ok(value) = i32::try_from(value) {
            self.out.push(b'J');
            self.out.extend_from_slice(&value.to_le_bytes());
        } else {
            self.long(&BigInt {
                negative: value < 0,
                magnitude: value.unsigned_abs().to_be_bytes().to_vec(),
            });
        }
    }

    /// Writes an integer with `LONG1` or `LONG4` (little-endian two's
    /// complement).
    fn long(&mut self, value: &BigInt) {
        let magnitude = value.significant_magnitude();
        // one more byte than the magnitude has room for the sign
        let mut bytes: Vec<u8> = magnitude.iter().rev().copied().collect();
        bytes.push(0);
        if value.is_negative() {
            for byte in bytes.iter_mut() {
                *byte = !*byte;
            }
            for byte in bytes.iter_mut() {
                let (sum, overflow) = byte.overflowing_add(1);
                *byte = sum;
                if !overflow {
                    break;
                }
            }
        }
        // drop the bytes that only repeat the sign
        while bytes.len() > 1 {
            let last = bytes[bytes.len() - 1];
            let before = bytes[bytes.len() - 2];
            if (last == 0 && before & 0x80 == 0) || (last == 0xff && before & 0x80 != 0) {
                bytes.pop();
            } else {
                break;
            }
        }
        if value.is_zero() {
            bytes.clear();
        }
        if bytes.len() < 256 {
            self.out.push(0x8a);
            self.out.push(bytes.len() as u8);
        } else {
            self.out.push(0x8b);
            self.out
                .extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        }
        self.out.extend_from_slice(&bytes);
    }

    fn float(&mut self, value: f64) {
        self.out.push(b'G');
        self.out.extend_from_slice(&value.to_be_bytes());
    }

    fn string(&mut self, value: &str) -> Result<(), Error> {
        let len = value.len();
        if self.proto >= 4 && len < 256 {
            self.out.push(0x8c);
            self.out.push(len as u8);
        } else if let Ok(len) = u32::try_from(len) {
            self.out.push(b'X');
            self.out.extend_from_slice(&len.to_le_bytes());
        } else if self.proto >= 4 {
            self.out.push(0x8d);
            self.out.extend_from_slice(&(len as u64).to_le_bytes());
        } else {
            return Err(too_large());
        }
        self.out.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), Error> {
        if self.proto < 3 {
            // Python 3 writes `_codecs.encode(text, "latin1")`
            if value.is_empty() {
                self.out.extend_from_slice(b"c__builtin__\nbytes\n)R");
            } else {
                self.out.extend_from_slice(b"c_codecs\nencode\n");
                self.string(&latin1(value))?;
                self.string("latin1")?;
                self.out.extend_from_slice(b"\x86R");
            }
            return Ok(());
        }
        let len = value.len();
        if len < 256 {
            self.out.push(b'C');
            self.out.push(len as u8);
        } else if let Ok(len) = u32::try_from(len) {
            self.out.push(b'B');
            self.out.extend_from_slice(&len.to_le_bytes());
        } else if self.proto >= 4 {
            self.out.push(0x8e);
            self.out.extend_from_slice(&(len as u64).to_le_bytes());
        } else {
            return Err(too_large());
        }
        self.out.extend_from_slice(value);
        Ok(())
    }

    fn bytearray(&mut self, value: &[u8]) -> Result<(), Error> {
        match self.proto {
            5.. => {
                self.out.push(0x96);
                self.out
                    .extend_from_slice(&(value.len() as u64).to_le_bytes());
                self.out.extend_from_slice(value);
            }
            3 | 4 => {
                self.global(&Global::new("builtins", "bytearray"))?;
                self.bytes(value)?;
                self.out.extend_from_slice(b"\x85R");
            }
            _ => {
                self.out.extend_from_slice(b"c__builtin__\nbytearray\n");
                self.string(&latin1(value))?;
                self.string("latin-1")?;
                self.out.extend_from_slice(b"\x86R");
            }
        }
        Ok(())
    }
}

/// Decodes bytes as latin-1.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&c| char::from(c)).collect()
}

#[cold]
fn too_large() -> Error {
    Error::new(
        ErrorKind::OutOfRange,
        "value too large for the pickle protocol",
    )
}
