use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

use crate::adapters::bytes::BytesFormat;
use crate::error::{Error, ErrorKind};
use crate::ext::ExtValue;

/// An atom is a primitive value for serialization and deserialization.
///
/// Atoms are values that are sent directly to a serializer or deserializer.
/// Examples for this are booleans or integers.  This is in contrast to
/// compound values like maps, structs or sequences.
///
/// Atoms are non exhaustive which means that new variants might appear
/// in the future.  Deser tries to build around this restriction for instance
/// through APIs like [`unexpected_atom`](crate::de::Sink::unexpected_atom) so that
/// one always have something to call.
///
/// Values which are not part of the core data model are represented as
/// [`Atom::Ext`].  For more information see [`ext`](crate::ext).
///
/// Some atoms carry metadata that consumers can ignore: [`Float`] knows the
/// kind of float it came from and [`Bytes`] can carry a representation for
/// formats without native bytes.
#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
pub enum Atom<'a> {
    Null,
    Bool(bool),
    Str(Cow<'a, str>),
    Bytes(Bytes<'a>),
    Char(char),
    U64(u64),
    I64(i64),
    Float(Float),
    /// A value that extends the data model.
    ///
    /// See [`ext`](crate::ext) for more information.
    Ext(ExtValue<'a>),
}

impl<'a> Atom<'a> {
    /// Makes a static clone of the atom decoupling the lifetimes.
    pub fn to_static(&self) -> Atom<'static> {
        match *self {
            Atom::Null => Atom::Null,
            Atom::Bool(v) => Atom::Bool(v),
            Atom::Str(ref v) => Atom::Str(Cow::Owned(v.to_string())),
            Atom::Bytes(ref v) => Atom::Bytes(v.to_static()),
            Atom::Char(v) => Atom::Char(v),
            Atom::U64(v) => Atom::U64(v),
            Atom::I64(v) => Atom::I64(v),
            Atom::Float(v) => Atom::Float(v),
            Atom::Ext(ref v) => Atom::Ext(v.to_static()),
        }
    }

    /// Returns an atom borrowing from this one.
    ///
    /// This is useful to pass a stored atom on without cloning its data.
    pub fn as_borrowed(&self) -> Atom<'_> {
        match *self {
            Atom::Null => Atom::Null,
            Atom::Bool(v) => Atom::Bool(v),
            Atom::Str(ref v) => Atom::Str(Cow::Borrowed(v)),
            Atom::Bytes(ref v) => Atom::Bytes(v.as_borrowed()),
            Atom::Char(v) => Atom::Char(v),
            Atom::U64(v) => Atom::U64(v),
            Atom::I64(v) => Atom::I64(v),
            Atom::Float(v) => Atom::Float(v),
            Atom::Ext(ref v) => Atom::Ext(v.as_borrowed()),
        }
    }

    /// Returns the human readable name of the atom.
    pub fn name(&self) -> &str {
        match *self {
            Atom::Null => "null",
            Atom::Bool(_) => "bool",
            Atom::Str(_) => "string",
            Atom::Bytes(_) => "bytes",
            Atom::Char(_) => "char",
            Atom::U64(_) => "unsigned integer",
            Atom::I64(_) => "signed integer",
            Atom::Float(_) => "float",
            Atom::Ext(ref v) => v.name(),
        }
    }

    /// Creates an "unexpected" error.
    ///
    /// This is useful when implementing sinks that do not want to deal with an
    /// atom of a specific type.  The default implementation of a
    /// [`Sink`](crate::de::Sink) uses this method as follows:
    ///
    /// ```
    /// # use deser::{Atom, Error, State, de::Sink};
    /// # struct MySink;
    /// impl<'de> Sink<'de> for MySink {
    ///     fn atom(&mut self, atom: Atom, _state: &mut State) -> Result<(), Error> {
    ///         Err(atom.unexpected_error(&self.expecting()))
    ///     }
    /// }
    /// ```
    pub fn unexpected_error(&self, expectation: &str) -> Error {
        Error::new(
            ErrorKind::Unexpected,
            format!("unexpected {}, expected {}", self.name(), expectation),
        )
    }
}

macro_rules! impl_from {
    ($ty:ty, $atom:ident) => {
        impl From<$ty> for Event<'static> {
            fn from(value: $ty) -> Self {
                Event::Atom(Atom::$atom(value as _))
            }
        }
    };
}

impl_from!(u64, U64);
impl_from!(i64, I64);
impl_from!(usize, U64);
impl_from!(isize, I64);
impl_from!(bool, Bool);
impl_from!(char, Char);

impl From<f64> for Event<'static> {
    fn from(value: f64) -> Self {
        Event::Atom(Atom::Float(Float::new(value)))
    }
}

impl From<f32> for Event<'static> {
    fn from(value: f32) -> Self {
        Event::Atom(Atom::Float(Float::from_f32(value)))
    }
}

impl From<u128> for Event<'static> {
    fn from(value: u128) -> Self {
        Event::Atom(Atom::Ext(ExtValue::owned(value)))
    }
}

impl From<i128> for Event<'static> {
    fn from(value: i128) -> Self {
        Event::Atom(Atom::Ext(ExtValue::owned(value)))
    }
}

impl From<()> for Event<'static> {
    fn from(_: ()) -> Event<'static> {
        Event::Atom(Atom::Null)
    }
}

impl<'a> From<&'a str> for Event<'a> {
    fn from(value: &'a str) -> Event<'a> {
        Event::Atom(Atom::Str(Cow::Borrowed(value)))
    }
}

impl<'a> From<Cow<'a, str>> for Event<'a> {
    fn from(value: Cow<'a, str>) -> Event<'a> {
        Event::Atom(Atom::Str(value))
    }
}

impl<'a> From<&'a [u8]> for Event<'a> {
    fn from(value: &'a [u8]) -> Event<'a> {
        Event::Atom(Atom::Bytes(Bytes::borrowed(value)))
    }
}

impl From<String> for Event<'static> {
    fn from(value: String) -> Event<'static> {
        Event::Atom(Atom::Str(Cow::Owned(value)))
    }
}

impl<'a> From<Atom<'a>> for Event<'a> {
    fn from(atom: Atom<'a>) -> Self {
        Event::Atom(atom)
    }
}

/// An event represents an atomic serialization and deserialization event.
///
/// ## Serialization
///
/// [`Event`] and [`Chunk`](crate::ser::Chunk) are two close relatives.  A chunk
/// is stateful whereas [`Event`] represents a single event from a chunk.
/// Atomic chunks directly create an event whereas compound chunks keep emitting
/// more chunks which again can produce events.  To go from chunks to events use
/// the [`SerializeDriver`](crate::ser::SerializeDriver) method.
///
/// ## Deserialization
///
/// During deserialization events are passed to a
/// [`DeserializeDriver`](crate::de::DeserializeDriver) to drive the deserialization.
///
/// The start events of maps and sequences carry the [`ContainerShape`].
#[derive(PartialEq, Clone)]
pub enum Event<'a> {
    Atom(Atom<'a>),
    MapStart(ContainerShape),
    MapEnd,
    SeqStart(ContainerShape),
    SeqEnd,
}

impl<'a> Event<'a> {
    /// Creates the start event of a map with the default shape.
    pub const fn map_start() -> Event<'static> {
        Event::MapStart(ContainerShape::new())
    }

    /// Creates the start event of a sequence with the default shape.
    pub const fn seq_start() -> Event<'static> {
        Event::SeqStart(ContainerShape::new())
    }

    /// Returns an event borrowing from this one.
    pub fn as_borrowed(&self) -> Event<'_> {
        match *self {
            Event::Atom(ref atom) => Event::Atom(atom.as_borrowed()),
            Event::MapStart(shape) => Event::MapStart(shape),
            Event::MapEnd => Event::MapEnd,
            Event::SeqStart(shape) => Event::SeqStart(shape),
            Event::SeqEnd => Event::SeqEnd,
        }
    }

    /// Makes a static clone of the event decoupling the lifetimes.
    pub fn to_static(&self) -> Event<'static> {
        match *self {
            Event::Atom(ref atom) => Event::Atom(atom.to_static()),
            Event::MapStart(shape) => Event::MapStart(shape),
            Event::MapEnd => Event::MapEnd,
            Event::SeqStart(shape) => Event::SeqStart(shape),
            Event::SeqEnd => Event::SeqEnd,
        }
    }
}

impl fmt::Debug for Event<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the default shape is left out to keep the output short
        let (name, shape) = match *self {
            Event::Atom(ref atom) => return f.debug_tuple("Atom").field(atom).finish(),
            Event::MapStart(shape) => ("MapStart", shape),
            Event::MapEnd => return f.write_str("MapEnd"),
            Event::SeqStart(shape) => ("SeqStart", shape),
            Event::SeqEnd => return f.write_str("SeqEnd"),
        };
        if shape == ContainerShape::new() {
            f.write_str(name)
        } else {
            f.debug_tuple(name).field(&shape).finish()
        }
    }
}

/// The kind of a float before it was widened to `f64`.
///
/// Floats are passed through the data model as `f64` which represents all
/// narrower binary floats exactly.  The kind tells which type the value came
/// from so that formats can write the shortest representation or use a
/// native encoding.  It is never required for correctness, formats which
/// ignore it write the `f64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FloatKind {
    /// A 64 bit float.
    #[default]
    F64,
    /// A 32 bit float.
    F32,
}

/// A float in the data model.
///
/// This is an `f64` together with the [`FloatKind`] it was widened from.
/// The kind always matches the value: a float of kind [`FloatKind::F32`]
/// has a value that is exactly representable as `f32`.
#[derive(Clone, Copy, PartialEq)]
pub struct Float {
    value: f64,
    kind: FloatKind,
}

impl Float {
    /// Creates a float from an `f64`.
    #[inline]
    pub const fn new(value: f64) -> Float {
        Float {
            value,
            kind: FloatKind::F64,
        }
    }

    /// Creates a float from an `f32`.
    #[inline]
    pub const fn from_f32(value: f32) -> Float {
        Float {
            value: value as f64,
            kind: FloatKind::F32,
        }
    }

    /// Creates a float of the given kind.
    ///
    /// Returns `None` if the value cannot be represented exactly in the
    /// kind.
    pub fn with_kind(value: f64, kind: FloatKind) -> Option<Float> {
        let exact = match kind {
            FloatKind::F64 => true,
            FloatKind::F32 => value.is_nan() || (value as f32) as f64 == value,
        };
        exact.then_some(Float { value, kind })
    }

    /// Returns the value.
    #[inline]
    pub const fn value(self) -> f64 {
        self.value
    }

    /// Returns the kind of float the value came from.
    #[inline]
    pub const fn kind(self) -> FloatKind {
        self.kind
    }
}

impl From<f64> for Float {
    #[inline]
    fn from(value: f64) -> Float {
        Float::new(value)
    }
}

impl From<f32> for Float {
    #[inline]
    fn from(value: f32) -> Float {
        Float::from_f32(value)
    }
}

impl fmt::Debug for Float {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            FloatKind::F64 => fmt::Debug::fmt(&self.value, f),
            FloatKind::F32 => write!(f, "{:?}f32", self.value as f32),
        }
    }
}

/// Bytes in the data model.
///
/// Bytes can carry a [`BytesFormat`] as fallback which formats without
/// native bytes (such as JSON) use instead of their configured format.
/// Formats with native bytes ignore it.  This is set by
/// [`BytesFallback`](crate::adapters::bytes::BytesFallback).
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub struct Bytes<'a> {
    /// The data.
    pub data: Cow<'a, [u8]>,
    /// The format used by formats without native bytes, if any.
    pub fallback: Option<&'static BytesFormat>,
}

impl<'a> Bytes<'a> {
    /// Creates bytes from borrowed or owned data.
    #[inline]
    pub fn new<D: Into<Cow<'a, [u8]>>>(data: D) -> Bytes<'a> {
        Bytes {
            data: data.into(),
            fallback: None,
        }
    }

    /// Creates bytes borrowing the data.
    #[inline]
    pub const fn borrowed(data: &'a [u8]) -> Bytes<'a> {
        Bytes {
            data: Cow::Borrowed(data),
            fallback: None,
        }
    }

    /// Sets the format that is used by formats without native bytes.
    #[inline]
    pub fn with_fallback(mut self, format: &'static BytesFormat) -> Bytes<'a> {
        self.fallback = Some(format);
        self
    }

    /// Returns the data.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns the data, borrowed or owned.
    #[inline]
    pub fn into_data(self) -> Cow<'a, [u8]> {
        self.data
    }

    /// Returns the data as owned vector.
    #[inline]
    pub fn into_owned(self) -> Vec<u8> {
        self.data.into_owned()
    }

    /// Returns bytes borrowing from these.
    pub fn as_borrowed(&self) -> Bytes<'_> {
        Bytes {
            data: Cow::Borrowed(&self.data),
            fallback: self.fallback,
        }
    }

    /// Makes a static clone decoupling the lifetimes.
    pub fn to_static(&self) -> Bytes<'static> {
        Bytes {
            data: Cow::Owned(self.data.to_vec()),
            fallback: self.fallback,
        }
    }
}

impl Deref for Bytes<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.data
    }
}

impl AsRef<[u8]> for Bytes<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl<'a> From<&'a [u8]> for Bytes<'a> {
    fn from(data: &'a [u8]) -> Bytes<'a> {
        Bytes::borrowed(data)
    }
}

impl From<Vec<u8>> for Bytes<'static> {
    fn from(data: Vec<u8>) -> Bytes<'static> {
        Bytes::new(data)
    }
}

impl<'a> From<Cow<'a, [u8]>> for Bytes<'a> {
    fn from(data: Cow<'a, [u8]>) -> Bytes<'a> {
        Bytes::new(data)
    }
}

impl fmt::Debug for Bytes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.data[..], f)?;
        if let Some(format) = self.fallback {
            write!(f, " as {}", format.name())?;
        }
        Ok(())
    }
}

/// How significant the order of the elements of a container is.
///
/// The default ([`Order::Natural`]) means the natural semantics of the
/// container: the order of sequences is significant, the order of maps is
/// not but the emitted order is kept.  Only deviations are marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Order {
    /// The natural semantics of the container.
    #[default]
    Natural,
    /// The order is not significant and the elements are emitted in an
    /// arbitrary order that can change between runs (`HashMap`, `HashSet`).
    Arbitrary,
    /// The order is not significant and the elements are sorted
    /// (`BTreeMap`, `BTreeSet`).
    Sorted,
    /// The order is significant, also for maps.
    Significant,
}

impl Order {
    const fn to_bits(self) -> u32 {
        match self {
            Order::Natural => 0,
            Order::Arbitrary => 1,
            Order::Sorted => 2,
            Order::Significant => 3,
        }
    }

    const fn from_bits(bits: u32) -> Order {
        match bits & ORDER_MASK {
            1 => Order::Arbitrary,
            2 => Order::Sorted,
            3 => Order::Significant,
            _ => Order::Natural,
        }
    }
}

const ORDER_MASK: u32 = 0b11;
const UNKNOWN_LEN: usize = usize::MAX;

/// Facts about a map or sequence.
///
/// The shape is carried by [`Event::MapStart`] and [`Event::SeqStart`].  It
/// holds information that formats can use to encode or decode a container,
/// all of which can be ignored:
///
/// * [`order`](Self::order): how significant the order of the elements is.
/// * [`len`](Self::len): the number of elements (entries for maps) if known.
///
/// ```
/// use deser::{ContainerShape, Order};
///
/// const SHAPE: ContainerShape = ContainerShape::new().with_order(Order::Sorted);
/// assert_eq!(SHAPE.order(), Order::Sorted);
/// assert_eq!(SHAPE.len(), None);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContainerShape {
    len: usize,
    flags: u32,
}

impl ContainerShape {
    /// Creates the default shape: unknown length and natural order.
    #[inline]
    pub const fn new() -> ContainerShape {
        ContainerShape {
            len: UNKNOWN_LEN,
            flags: 0,
        }
    }

    /// Sets the number of elements.
    #[inline]
    pub const fn with_len(mut self, len: usize) -> ContainerShape {
        self.len = len;
        self
    }

    /// Sets the order.
    #[inline]
    pub const fn with_order(mut self, order: Order) -> ContainerShape {
        self.flags = (self.flags & !ORDER_MASK) | order.to_bits();
        self
    }

    /// Returns the number of elements (entries for maps) if known.
    #[inline]
    #[allow(clippy::len_without_is_empty)]
    pub const fn len(&self) -> Option<usize> {
        if self.len == UNKNOWN_LEN {
            None
        } else {
            Some(self.len)
        }
    }

    /// Returns how significant the order of the elements is.
    #[inline]
    pub const fn order(&self) -> Order {
        Order::from_bits(self.flags)
    }
}

impl Default for ContainerShape {
    fn default() -> ContainerShape {
        ContainerShape::new()
    }
}

impl fmt::Debug for ContainerShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerShape")
            .field("len", &self.len())
            .field("order", &self.order())
            .finish()
    }
}

#[test]
fn test_sizes() {
    assert_eq!(std::mem::size_of::<Atom>(), 32);
    assert_eq!(std::mem::size_of::<Event>(), 32);
}
