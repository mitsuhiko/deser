use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use deser_core::State;
use deser_core::ext::{BigInt, Datetime, ExtValue, Number, Timestamp};
use deser_core::ser::{self, SerializeDriver};
use deser_core::{Atom, Error, ErrorKind, Event, Serialize};

use crate::format::Format;
use crate::uid::Uid;
use crate::{write_ascii, write_binary, write_xml};

/// Configures how values are serialized to property lists.
///
/// The [`format`](Self::format) selects the encoding, by default XML is
/// written.
///
/// ```
/// use deser_plist::{Format, SerializerConfig};
///
/// const BINARY: SerializerConfig = SerializerConfig::new().format(Format::Binary);
/// let bytes = BINARY.to_vec(&vec![1, 2, 3]).unwrap();
/// assert!(bytes.starts_with(b"bplist00"));
///
/// const ASCII: SerializerConfig = SerializerConfig::new().format(Format::Ascii);
/// assert_eq!(ASCII.to_string(&vec![1, 2, 3]).unwrap(), "(\n\t1,\n\t2,\n\t3,\n)\n");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerializerConfig {
    format: Format,
}

impl SerializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> SerializerConfig {
        SerializerConfig {
            format: Format::Xml,
        }
    }

    /// Sets the format to write.
    pub const fn format(mut self, format: Format) -> SerializerConfig {
        self.format = format;
        self
    }

    /// Serializes the given value.
    pub fn to_vec(&self, value: &dyn Serialize) -> Result<Vec<u8>, Error> {
        self.to_vec_with(value, |_| {})
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_vec_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        self.serialize_driver(&mut driver)
    }

    /// Serializes the given value into a string.
    ///
    /// This fails for the binary format.
    pub fn to_string(&self, value: &dyn Serialize) -> Result<String, Error> {
        if !self.format.is_text() {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "binary property lists cannot be written to strings",
            ));
        }
        let bytes = self.to_vec(value)?;
        // the text writers only write UTF-8
        Ok(String::from_utf8(bytes).unwrap())
    }

    /// Serializes the value of a driver.
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
    ) -> Result<Vec<u8>, Error> {
        let mut builder = Builder::default();
        driver.drive_sink(&mut builder)?;
        let tree = builder.finish()?;
        Ok(match self.format {
            Format::Xml => write_xml::write(&tree)?.into_bytes(),
            Format::Ascii => write_ascii::write(&tree).into_bytes(),
            Format::Binary => write_binary::write(&tree),
        })
    }
}

/// Serializes values into property lists.
///
/// A property list holds a single value.  The serializer is used to
/// [`drive`](ser::Serializer::drive) a configured driver.
///
/// ```
/// use deser_plist::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&true).unwrap();
/// assert!(serializer.output().ends_with(b"<plist version=\"1.0\">\n<true/>\n</plist>\n"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct Serializer {
    config: SerializerConfig,
    out: Vec<u8>,
    written: bool,
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
            out: Vec::new(),
            written: false,
        }
    }

    /// Serializes a value.
    ///
    /// A property list holds a single value, serializing a second value
    /// fails.
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
        if self.written {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "a property list holds a single value",
            ));
        }
        self.out = self.config.serialize_driver(driver)?;
        self.written = true;
        Ok(())
    }
}

/// Serializes a value to an XML property list.
///
/// This uses the default [`SerializerConfig`].
///
/// ```
/// let bytes = deser_plist::to_vec(&vec!["a", "b"]).unwrap();
/// assert!(bytes.ends_with(b"<array>\n\t<string>a</string>\n\t<string>b</string>\n</array>\n</plist>\n"));
/// ```
pub fn to_vec(value: &dyn Serialize) -> Result<Vec<u8>, Error> {
    SerializerConfig::new().to_vec(value)
}

/// Serializes a value to an XML property list in a string.
///
/// This uses the default [`SerializerConfig`].
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// A value of a property list.
pub(crate) enum Node {
    Bool(bool),
    Int(i128),
    Real(f64),
    Real32(f32),
    Str(String),
    Data(Vec<u8>),
    Date(Timestamp),
    Uid(u64),
    Array(Vec<usize>),
    Dict(Vec<(String, usize)>),
}

/// The values of a property list, the first one is the top value.
pub(crate) struct Tree {
    pub(crate) nodes: Vec<Node>,
}

/// An open container of the builder.
enum Open {
    Array(usize, Vec<usize>),
    Dict(usize, Vec<(String, usize)>, Option<String>),
}

/// Builds the tree from serialization events.
#[derive(Default)]
struct Builder {
    nodes: Vec<Node>,
    stack: Vec<Open>,
}

impl ser::EventSink for Builder {
    fn event(&mut self, event: Event, _state: &mut State) -> Result<(), Error> {
        Builder::event(self, event)
    }
}

impl Builder {
    fn finish(self) -> Result<Tree, Error> {
        if self.nodes.is_empty() || !self.stack.is_empty() {
            return Err(Error::new(ErrorKind::Unexpected, "incomplete value"));
        }
        Ok(Tree { nodes: self.nodes })
    }

    fn event(&mut self, event: Event) -> Result<(), Error> {
        let Some(open) = self.stack.last_mut() else {
            if !self.nodes.is_empty() {
                return Err(Error::new(ErrorKind::Unexpected, "unexpected event"));
            }
            return match self.value(event)? {
                Some(_) => Ok(()),
                None => Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "property lists cannot hold null values",
                )),
            };
        };
        match open {
            Open::Dict(_, _, key @ None) => match event {
                Event::Atom(atom) => {
                    *key = Some(key_to_string(atom)?);
                    Ok(())
                }
                Event::MapEnd => self.close(),
                _ => Err(unsupported_key()),
            },
            Open::Dict(_, _, key @ Some(_)) => {
                let key = key.take().unwrap();
                // map entries with null values are skipped
                if let Some(value) = self.value(event)? {
                    self.add_entry(key, value);
                }
                Ok(())
            }
            Open::Array(..) => {
                if event == Event::SeqEnd {
                    return self.close();
                }
                match self.value(event)? {
                    Some(value) => {
                        self.add_item(value);
                        Ok(())
                    }
                    None => Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "property lists cannot hold null values in arrays",
                    )),
                }
            }
        }
    }

    /// Returns the open container a value was added to.
    ///
    /// If the value opened a container, it's on top of the stack and the
    /// parent is below it.
    fn parent(&mut self, value: usize) -> &mut Open {
        let len = self.stack.len();
        let opened = match self.stack[len - 1] {
            Open::Array(id, _) | Open::Dict(id, _, _) => id == value,
        };
        &mut self.stack[if opened { len - 2 } else { len - 1 }]
    }

    fn add_entry(&mut self, key: String, value: usize) {
        match self.parent(value) {
            Open::Dict(_, entries, _) => entries.push((key, value)),
            Open::Array(..) => unreachable!(),
        }
    }

    fn add_item(&mut self, value: usize) {
        match self.parent(value) {
            Open::Array(_, items) => items.push(value),
            Open::Dict(..) => unreachable!(),
        }
    }

    /// Closes the innermost container.
    fn close(&mut self) -> Result<(), Error> {
        match self.stack.pop() {
            Some(Open::Array(id, items)) => self.nodes[id] = Node::Array(items),
            Some(Open::Dict(id, entries, None)) => self.nodes[id] = Node::Dict(entries),
            Some(Open::Dict(_, _, Some(_))) => {
                return Err(Error::new(ErrorKind::Unexpected, "map without value"));
            }
            None => return Err(Error::new(ErrorKind::Unexpected, "unexpected end")),
        }
        Ok(())
    }

    /// Adds the value of the first event of a value.  Maps and sequences
    /// are pushed to the stack.  Returns `None` for null.
    fn value(&mut self, event: Event) -> Result<Option<usize>, Error> {
        let id = self.nodes.len();
        let node = match event {
            Event::Atom(atom) => match convert_atom(atom)? {
                Some(node) => node,
                None => return Ok(None),
            },
            Event::MapStart(_) => {
                self.stack.push(Open::Dict(id, Vec::new(), None));
                Node::Dict(Vec::new())
            }
            Event::SeqStart(_) => {
                self.stack.push(Open::Array(id, Vec::new()));
                Node::Array(Vec::new())
            }
            Event::MapEnd | Event::SeqEnd => {
                return Err(Error::new(ErrorKind::Unexpected, "unexpected end event"));
            }
        };
        self.nodes.push(node);
        Ok(Some(id))
    }
}

/// Converts an atom into a node.  Returns `None` for null.
fn convert_atom(atom: Atom) -> Result<Option<Node>, Error> {
    Ok(Some(match atom {
        Atom::Null => return Ok(None),
        Atom::Bool(value) => Node::Bool(value),
        Atom::Str(value) | Atom::Lexical(value) => Node::Str(value.into_owned()),
        Atom::Char(value) => Node::Str(value.to_string()),
        Atom::U64(value) => Node::Int(value.into()),
        Atom::I64(value) => Node::Int(value.into()),
        Atom::F64(value) => Node::Real(value),
        Atom::F32(value) => Node::Real32(value),
        // property lists have binary data, bytes are never encoded
        Atom::Bytes(value) => Node::Data(value.into_owned()),
        Atom::Ext(ref ext) => return convert_ext(ext),
        // values whose type was inferred from text are written as value
        Atom::Implicit(value) => return convert_atom(value.value().to_atom()),
        _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
    }))
}

#[cold]
fn convert_ext(ext: &ExtValue) -> Result<Option<Node>, Error> {
    if let Some(value) = ext.downcast_ref::<Uid>() {
        return Ok(Some(Node::Uid(value.get())));
    }
    if let Some(&value) = ext.downcast_ref::<Timestamp>() {
        return Ok(Some(Node::Date(value)));
    }
    // offset date-times are instants, other date-times are strings
    if let Some(&value) = ext.downcast_ref::<Datetime>()
        && value.offset.is_some()
        && let Ok(value) = Timestamp::try_from(value)
    {
        return Ok(Some(Node::Date(value)));
    }
    let out_of_range = || Error::new(ErrorKind::OutOfRange, "integer out of range for plist");
    if let Some(&value) = ext.downcast_ref::<u128>() {
        return Ok(Some(Node::Int(
            i128::try_from(value).map_err(|_| out_of_range())?,
        )));
    }
    if let Some(&value) = ext.downcast_ref::<i128>() {
        return Ok(Some(Node::Int(value)));
    }
    if let Some(value) = ext.downcast_ref::<BigInt>().and_then(|x| x.to_i128()) {
        return Ok(Some(Node::Int(value)));
    }
    // numbers from text formats are integers if their text is one
    if let Some(value) = ext.downcast_value_ref::<Number>()
        && let Ok(value) = value.as_str().parse::<i128>()
    {
        return Ok(Some(Node::Int(value)));
    }
    match ext.fallback() {
        Atom::Ext(_) => Err(Error::new(
            ErrorKind::UnsupportedType,
            format!("property lists do not support {}", ext.name()),
        )),
        fallback => convert_atom(fallback),
    }
}

/// Converts a key into a string.
fn key_to_string(atom: Atom) -> Result<String, Error> {
    Ok(match atom {
        Atom::Implicit(value) => return key_to_string(value.value().to_atom()),
        Atom::Str(value) | Atom::Lexical(value) => value.into_owned(),
        Atom::Char(value) => value.to_string(),
        Atom::U64(value) => value.to_string(),
        Atom::I64(value) => value.to_string(),
        Atom::Bool(value) => value.to_string(),
        Atom::Ext(ref ext) => {
            if let Some(value) = ext.downcast_ref::<u128>() {
                value.to_string()
            } else if let Some(value) = ext.downcast_ref::<i128>() {
                value.to_string()
            } else {
                match ext.fallback() {
                    Atom::Ext(_) => return Err(unsupported_key()),
                    fallback => return key_to_string(fallback),
                }
            }
        }
        _ => return Err(unsupported_key()),
    })
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "dictionary keys of property lists must be strings",
    )
}
