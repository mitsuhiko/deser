use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;

use crate::BytesFormat;
use crate::error::{Error, ErrorKind};
use crate::ext::ExtValue;
use crate::text::{Slice, Text};

/// An atom is a primitive value for serialization and deserialization.
///
/// Atoms are values that are sent directly to a serializer or deserializer.
/// Examples for this are booleans or integers.  This is in contrast to
/// compound values like maps, structs or sequences.
///
/// Atoms are non exhaustive which means that new variants might appear
/// in the future.  Deser tries to build around this restriction for instance
/// through the default handling of atoms
/// ([`default_atom`](crate::de::default_atom)) so that one always has
/// something to call.
///
/// Values which are not part of the core data model are represented as
/// [`Atom::Ext`].  For more information see [`ext`](crate::ext).
///
/// [`Bytes`] can carry a representation for formats without native bytes
/// which formats with native bytes ignore.
///
/// Floats are [`F32`](Atom::F32) or [`F64`](Atom::F64) depending on their
/// precision.  A single precision float is a value of its own as its
/// shortest text differs from the one of the same value as `f64` (`0.1f32`
/// is `0.1`, as `f64` it's `0.10000000149011612`).  Sinks that do not care
/// about the precision only need to handle `F64`: the default handling of
/// atoms widens `F32` (see
/// [`default_atom`](crate::de::default_atom)).
///
/// Text whose type the format cannot express is [`Lexical`](Atom::Lexical).
/// It's a string for everybody who does not care, see there for more
/// information.  A value whose type the format inferred from its text is
/// [`Implicit`](Atom::Implicit), it carries the text for types that do not
/// accept the value.
#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
// The tag is a full word in front of the values: moving atoms (which
// happens for every value) then copies whole words.  With a byte sized tag
// small values are stored next to the tag and atoms are written and read
// in pieces of different sizes, which stalls loads (and was 5-15% slower
// for numbers in binary formats).
#[repr(C, u64)]
pub enum Atom<'a> {
    Null,
    Bool(bool),
    Str(Text<'a>),
    /// The lexical form of a value whose type the format cannot express.
    ///
    /// Some formats cannot say what type a piece of text is: everything in
    /// a query string is text, and so are the keys of JSON objects.  Such
    /// text is emitted as a lexical atom and the sink it's delivered to
    /// decides what it means: numbers and booleans parse it, strings take
    /// it as it is.  Text that is known to be a string (like a string value
    /// in JSON, where the number `42` could have been written instead of
    /// `"42"`) is [`Str`](Atom::Str).
    ///
    /// Sinks receive it as [`Str`](Atom::Str) unless they handle it (see
    /// [`default_atom`](crate::de::default_atom)).  Sinks
    /// that borrow strings have to handle it themselves, the fallback does
    /// not borrow for the lifetime of the input.  Serializers write it as
    /// string.
    ///
    /// Integers and floats parse lexical atoms with [`str::parse`], how
    /// booleans are spelled, if empty text is a missing value and if text is
    /// a sequence of one element depends on the
    /// [`LexicalRules`](crate::de::LexicalRules) of the deserialization,
    /// which the format sets.  All other types that accept strings accept
    /// lexical atoms as string.
    Lexical(Text<'a>),
    Bytes(Bytes<'a>),
    Char(char),
    U64(u64),
    I64(i64),
    /// A single precision float.
    ///
    /// Formats write it with the precision of an `f32`, for instance text
    /// formats with the shortest text that reads back as the same `f32`.
    /// Formats do not produce it when they read floats (the precision of
    /// a float in the input is unknown or, like in CBOR, an encoding
    /// detail).  Sinks receive it as [`F64`](Atom::F64) unless they handle
    /// it (see [`default_atom`](crate::de::default_atom)).
    F32(f32),
    /// A double precision float.
    F64(f64),
    /// A value that extends the data model.
    ///
    /// See [`ext`](crate::ext) for more information.
    Ext(ExtValue<'a>),
    /// A value whose type the format inferred from its text.
    ///
    /// See [`Implicit`] for more information.
    Implicit(Implicit<'a>),
}

impl<'a> Atom<'a> {
    /// Makes a static clone of the atom decoupling the lifetimes.
    pub fn to_static(&self) -> Atom<'static> {
        match *self {
            Atom::Null => Atom::Null,
            Atom::Bool(v) => Atom::Bool(v),
            Atom::Str(ref v) => Atom::Str(v.to_static()),
            Atom::Lexical(ref v) => Atom::Lexical(v.to_static()),
            Atom::Bytes(ref v) => Atom::Bytes(v.to_static()),
            Atom::Char(v) => Atom::Char(v),
            Atom::U64(v) => Atom::U64(v),
            Atom::I64(v) => Atom::I64(v),
            Atom::F32(v) => Atom::F32(v),
            Atom::F64(v) => Atom::F64(v),
            Atom::Ext(ref v) => Atom::Ext(v.to_static()),
            Atom::Implicit(ref v) => Atom::Implicit(v.to_static()),
        }
    }

    /// Returns an atom borrowing from this one.
    ///
    /// This is useful to pass a stored atom on without cloning its data.
    pub fn as_borrowed(&self) -> Atom<'_> {
        match *self {
            Atom::Null => Atom::Null,
            Atom::Bool(v) => Atom::Bool(v),
            Atom::Str(ref v) => Atom::Str(v.as_borrowed()),
            Atom::Lexical(ref v) => Atom::Lexical(v.as_borrowed()),
            Atom::Bytes(ref v) => Atom::Bytes(v.as_borrowed()),
            Atom::Char(v) => Atom::Char(v),
            Atom::U64(v) => Atom::U64(v),
            Atom::I64(v) => Atom::I64(v),
            Atom::F32(v) => Atom::F32(v),
            Atom::F64(v) => Atom::F64(v),
            Atom::Ext(ref v) => Atom::Ext(v.as_borrowed()),
            Atom::Implicit(ref v) => Atom::Implicit(v.as_borrowed()),
        }
    }

    /// Returns the text of a [`Str`](Atom::Str) or [`Lexical`](Atom::Lexical)
    /// atom.
    ///
    /// ```
    /// use deser::Atom;
    ///
    /// assert_eq!(Atom::Lexical("42".into()).as_str(), Some("42"));
    /// assert_eq!(Atom::Str("42".into()).as_str(), Some("42"));
    /// assert_eq!(Atom::U64(42).as_str(), None);
    /// ```
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Atom::Str(v) | Atom::Lexical(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the human readable name of the atom.
    pub fn name(&self) -> &str {
        match *self {
            Atom::Null => "null",
            Atom::Bool(_) => "bool",
            Atom::Str(_) | Atom::Lexical(_) => "string",
            Atom::Bytes(_) => "bytes",
            Atom::Char(_) => "char",
            Atom::U64(_) => "unsigned integer",
            Atom::I64(_) => "signed integer",
            Atom::F32(_) | Atom::F64(_) => "float",
            Atom::Ext(ref v) => v.name(),
            Atom::Implicit(ref v) => v.value().name(),
        }
    }

    /// Creates an "unexpected" error.
    ///
    /// This is the error that [`default_atom`](crate::de::default_atom)
    /// returns for atoms it cannot pass on in another form, with what the
    /// sink expects.  Sinks (and
    /// [`Deserialize::deserialize_atom`](crate::de::Deserialize::deserialize_atom))
    /// should pass the atoms they do not accept to `default_atom` rather
    /// than returning this error, so that extension values are passed on
    /// as their fallback and lexical atoms as strings.
    ///
    /// ```
    /// use deser::Atom;
    ///
    /// let err = Atom::Bool(true).unexpected_error("u32");
    /// assert_eq!(err.message(), "unexpected bool, expected u32");
    /// ```
    pub fn unexpected_error(&self, expectation: &str) -> Error {
        Error::new(
            ErrorKind::InvalidType,
            format!("unexpected {}, expected {}", self.name(), expectation),
        )
    }
}

/// A value whose type the format inferred from its text.
///
/// Some formats write values as text and infer their type from it: in YAML
/// `42` is an integer, `1.10` a float, `true` a boolean and `~` null, but
/// only because these plain scalars look like it.  The format resolves the
/// value with its own rules (which are not the ones of Rust, `0x1F` is an
/// integer in YAML) and emits it together with its text.
///
/// Types that accept the value receive it, types that reject it receive
/// the text as [`Str`](Atom::Str) instead (see
/// [`default_atom`](crate::de::default_atom)).  This
/// means that a `u32` is `31` for `0x1F` while a `String` is `"0x1F"` and an
/// `Option<String>` is `None` for `~` while a `String` is `"~"`.  If both are
/// rejected, the error is the one of the value.  Enums look up their
/// variants by the value and then by the text.  Types that take any value
/// (like dynamic values) keep both.  Serializers write the text if it's
/// the same value in their format (`1.10` stays `1.10` in JSON and YAML,
/// `0x1F` stays `0x1F` in YAML), otherwise they write the value.
///
/// ```
/// use deser::{Atom, Implicit, ImplicitValue};
///
/// let atom = Atom::Implicit(Implicit::new("0x1F", ImplicitValue::U64(31)));
/// let mut out = None::<u32>;
/// deser::de::DeserializeDriver::new(&mut out).emit(atom.clone()).unwrap();
/// assert_eq!(out, Some(31));
///
/// let mut out = None::<String>;
/// deser::de::DeserializeDriver::new(&mut out).emit(atom).unwrap();
/// assert_eq!(out.as_deref(), Some("0x1F"));
/// ```
#[derive(Clone)]
pub struct Implicit<'a> {
    // the kind of the value is the tag of the text (see `ImplicitValue::kind`)
    text: Text<'a>,
    // the value (see `ImplicitValue::bits`)
    bits: u64,
}

impl<'a> Implicit<'a> {
    /// Creates a value from its text and the value inferred from it.
    #[inline]
    pub fn new<T: Into<Text<'a>>>(text: T, value: ImplicitValue) -> Implicit<'a> {
        let (kind, bits) = value.pack();
        Implicit {
            text: text.into().with_tag(kind),
            bits,
        }
    }

    /// Returns the text of the value.
    #[inline]
    pub fn text(&self) -> &Text<'a> {
        &self.text
    }

    /// Returns the inferred value.
    #[inline]
    pub fn value(&self) -> ImplicitValue {
        ImplicitValue::unpack(self.text.tag(), self.bits)
    }

    /// Splits the value into its text and the inferred value.
    #[inline]
    pub fn into_parts(self) -> (Text<'a>, ImplicitValue) {
        let value = self.value();
        (self.text.with_tag(0), value)
    }

    /// Returns a value borrowing from this one.
    #[inline]
    pub fn as_borrowed(&self) -> Implicit<'_> {
        Implicit {
            text: self.text.as_borrowed().with_tag(self.text.tag()),
            bits: self.bits,
        }
    }

    /// Makes a static clone decoupling the lifetimes.
    #[inline]
    pub fn to_static(&self) -> Implicit<'static> {
        Implicit {
            text: self.text.to_static().with_tag(self.text.tag()),
            bits: self.bits,
        }
    }
}

impl fmt::Debug for Implicit<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Implicit")
            .field("text", &self.text)
            .field("value", &self.value())
            .finish()
    }
}

impl PartialEq for Implicit<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.value() == other.value()
    }
}

/// The value of an [`Implicit`] atom.
///
/// These are the types formats infer from text.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ImplicitValue {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
}

impl ImplicitValue {
    /// Splits the value into a kind (`0` to `7`, stored in the tag of the
    /// text of an [`Implicit`]) and the bits of the value.
    #[inline]
    fn pack(self) -> (u8, u64) {
        match self {
            ImplicitValue::Null => (0, 0),
            ImplicitValue::Bool(value) => (1, value as u64),
            ImplicitValue::U64(value) => (2, value),
            ImplicitValue::I64(value) => (3, value as u64),
            ImplicitValue::F64(value) => (4, value.to_bits()),
        }
    }

    /// Joins a value split with [`pack`](Self::pack).
    #[inline]
    fn unpack(kind: u8, bits: u64) -> ImplicitValue {
        match kind {
            1 => ImplicitValue::Bool(bits != 0),
            2 => ImplicitValue::U64(bits),
            3 => ImplicitValue::I64(bits as i64),
            4 => ImplicitValue::F64(f64::from_bits(bits)),
            _ => ImplicitValue::Null,
        }
    }

    /// Returns the value for an atom if it's one of the inferred types.
    pub fn from_atom(atom: &Atom<'_>) -> Option<ImplicitValue> {
        match *atom {
            Atom::Null => Some(ImplicitValue::Null),
            Atom::Bool(value) => Some(ImplicitValue::Bool(value)),
            Atom::U64(value) => Some(ImplicitValue::U64(value)),
            Atom::I64(value) => Some(ImplicitValue::I64(value)),
            Atom::F64(value) => Some(ImplicitValue::F64(value)),
            _ => None,
        }
    }

    /// Returns the value as atom.
    #[inline]
    pub fn to_atom(self) -> Atom<'static> {
        match self {
            ImplicitValue::Null => Atom::Null,
            ImplicitValue::Bool(value) => Atom::Bool(value),
            ImplicitValue::U64(value) => Atom::U64(value),
            ImplicitValue::I64(value) => Atom::I64(value),
            ImplicitValue::F64(value) => Atom::F64(value),
        }
    }

    /// Returns `true` if both are the same value.
    ///
    /// Unlike `==`, floats are compared by their bits: `NaN` is the same
    /// as `NaN` and `0.0` is not the same as `-0.0`.  This is useful to
    /// check if text reads back as the same value.
    ///
    /// ```
    /// use deser::ImplicitValue;
    ///
    /// assert!(
    ///     ImplicitValue::F64(f64::NAN).is_same(ImplicitValue::F64(f64::NAN))
    /// );
    /// assert!(!ImplicitValue::F64(0.0).is_same(ImplicitValue::F64(-0.0)));
    /// assert!(!ImplicitValue::U64(1).is_same(ImplicitValue::F64(1.0)));
    /// ```
    pub fn is_same(self, other: ImplicitValue) -> bool {
        match (self, other) {
            (ImplicitValue::F64(a), ImplicitValue::F64(b)) => a.to_bits() == b.to_bits(),
            (a, b) => a == b,
        }
    }

    /// Returns the human readable name of the value.
    pub fn name(&self) -> &'static str {
        match *self {
            ImplicitValue::Null => "null",
            ImplicitValue::Bool(_) => "bool",
            ImplicitValue::U64(_) => "unsigned integer",
            ImplicitValue::I64(_) => "signed integer",
            ImplicitValue::F64(_) => "float",
        }
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
        Event::Atom(Atom::F64(value))
    }
}

impl From<f32> for Event<'static> {
    fn from(value: f32) -> Self {
        Event::Atom(Atom::F32(value))
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
        Event::Atom(Atom::Str(Text::borrowed(value)))
    }
}

impl<'a> From<Cow<'a, str>> for Event<'a> {
    fn from(value: Cow<'a, str>) -> Event<'a> {
        Event::Atom(Atom::Str(value.into()))
    }
}

impl<'a> From<Text<'a>> for Event<'a> {
    fn from(value: Text<'a>) -> Event<'a> {
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
        Event::Atom(Atom::Str(value.into()))
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
/// [`Event`] and [`Emit`](crate::ser::Emit) are two close relatives.  An
/// [`Emit`](crate::ser::Emit) can be stateful whereas [`Event`] represents a
/// single event.  Atoms directly create an event whereas the emitters of
/// compound values keep handing out values which again produce events.  To
/// go from [`Emit`](crate::ser::Emit)s to events use
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

/// Bytes in the data model.
///
/// The data is borrowed or owned, like a `Cow<'a, [u8]>` but with a more
/// compact representation (see [`Text`]).
///
/// Bytes can carry a [`BytesFormat`] as fallback which formats without
/// native bytes (such as JSON) use instead of their configured format.
/// Formats with native bytes ignore it.  This is set by
/// [`BytesFallback`](crate::adapters::BytesFallback).
#[derive(Clone)]
#[non_exhaustive]
pub struct Bytes<'a> {
    data: Slice<'a>,
    /// The format used by formats without native bytes, if any.
    pub fallback: Option<&'static BytesFormat>,
}

impl<'a> Bytes<'a> {
    /// Creates bytes from borrowed or owned data.
    #[inline]
    pub fn new<D: Into<Cow<'a, [u8]>>>(data: D) -> Bytes<'a> {
        Bytes {
            data: Slice::from_cow(data.into()),
            fallback: None,
        }
    }

    /// Creates bytes borrowing the data.
    #[inline]
    pub const fn borrowed(data: &'a [u8]) -> Bytes<'a> {
        Bytes {
            data: Slice::borrowed(data),
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
        self.data.as_slice()
    }

    /// Returns `true` if the data borrows for `'a`.
    #[inline]
    pub fn is_borrowed(&self) -> bool {
        !self.data.is_owned()
    }

    /// Returns the data if it borrows for `'a`.
    ///
    /// This is used by types which borrow from the data that is
    /// deserialized (like `&'de [u8]`).
    #[inline]
    pub fn borrowed_data(&self) -> Option<&'a [u8]> {
        self.data.borrowed_slice()
    }

    /// Returns the data, borrowed or owned.
    #[inline]
    pub fn into_data(self) -> Cow<'a, [u8]> {
        self.data.into_cow()
    }

    /// Returns the data as owned vector.
    #[inline]
    pub fn into_owned(self) -> Vec<u8> {
        self.data.into_box().into_vec()
    }

    /// Returns bytes borrowing from these.
    pub fn as_borrowed(&self) -> Bytes<'_> {
        Bytes {
            data: self.data.reborrow(),
            fallback: self.fallback,
        }
    }

    /// Makes a static clone decoupling the lifetimes.
    pub fn to_static(&self) -> Bytes<'static> {
        Bytes {
            data: self.data.to_static(),
            fallback: self.fallback,
        }
    }
}

impl PartialEq for Bytes<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.data() == other.data() && self.fallback == other.fallback
    }
}

impl Deref for Bytes<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.data()
    }
}

impl AsRef<[u8]> for Bytes<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.data()
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
        fmt::Debug::fmt(self.data(), f)?;
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
const MULTIMAP: u32 = 0b100;
const LEN_HINT: u32 = 0b1000;
const UNKNOWN_LEN: usize = usize::MAX;

/// The maximum number of bytes that
/// [`ContainerShape::cautious_capacity`] preallocates.
const MAX_PREALLOCATION: usize = 1024 * 1024;

/// Facts about a map or sequence.
///
/// The shape is carried by [`Event::MapStart`] and [`Event::SeqStart`].  It
/// holds information that formats can use to encode or decode a container,
/// all of which can be ignored:
///
/// * [`order`](Self::order): how significant the order of the elements is.
/// * [`len`](Self::len): the number of elements (entries for maps) if known.
///   Formats that can only estimate it give a hint instead (see
///   [`with_len_hint`](Self::with_len_hint)).
/// * [`is_multimap`](Self::is_multimap): the keys of the map can be given
///   more than once.
///
/// ```
/// use deser::{ContainerShape, Order};
///
/// const SHAPE: ContainerShape =
///     ContainerShape::new().with_order(Order::Sorted);
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
    ///
    /// Serializers can rely on it, for instance to write the length in
    /// front of the elements.
    #[inline]
    pub const fn with_len(mut self, len: usize) -> ContainerShape {
        self.len = len;
        self.flags &= !LEN_HINT;
        self
    }

    /// Sets an estimate of the number of elements.
    ///
    /// The estimate is only used to preallocate containers (see
    /// [`cautious_capacity`](Self::cautious_capacity)), [`len`](Self::len)
    /// remains unknown.  A format that cannot know the length of a
    /// container before its end (like the number of records of a CSV file)
    /// can give one so that the container does not grow element by element.
    ///
    /// ```
    /// use deser::ContainerShape;
    ///
    /// let shape = ContainerShape::new().with_len_hint(10);
    /// assert_eq!(shape.len(), None);
    /// assert_eq!(shape.cautious_capacity::<u64>(), 10);
    ///
    /// // the length replaces the estimate once it's known
    /// assert_eq!(shape.with_len(3).len(), Some(3));
    /// ```
    #[inline]
    pub const fn with_len_hint(mut self, len: usize) -> ContainerShape {
        self.len = len;
        self.flags |= LEN_HINT;
        self
    }

    /// Sets the order.
    #[inline]
    pub const fn with_order(mut self, order: Order) -> ContainerShape {
        self.flags = (self.flags & !ORDER_MASK) | order.to_bits();
        self
    }

    /// Returns the number of elements (entries for maps) if known.
    ///
    /// During deserialization the length comes from the input and is not
    /// trusted: a few bytes can declare a container with billions of
    /// elements that are never sent.  To preallocate a container use
    /// [`cautious_capacity`](Self::cautious_capacity) instead.
    #[inline]
    #[allow(clippy::len_without_is_empty)]
    pub const fn len(&self) -> Option<usize> {
        if self.len == UNKNOWN_LEN || self.flags & LEN_HINT != 0 {
            None
        } else {
            Some(self.len)
        }
    }

    /// Returns the number of elements of type `T` to preallocate.
    ///
    /// This is the [`len`](Self::len) of the shape (or its estimate, see
    /// [`with_len_hint`](Self::with_len_hint)), capped so that no more
    /// than about a megabyte is preallocated, and `0` if the length is
    /// unknown.  As the length comes from the input it must not be trusted
    /// for allocations: a container that is larger grows as its elements
    /// arrive instead.
    ///
    /// ```
    /// use deser::ContainerShape;
    ///
    /// let shape = ContainerShape::new().with_len(10);
    /// assert_eq!(shape.cautious_capacity::<u64>(), 10);
    ///
    /// let shape = ContainerShape::new().with_len(usize::MAX - 1);
    /// assert_eq!(shape.cautious_capacity::<u64>(), 1024 * 1024 / 8);
    /// assert_eq!(ContainerShape::new().cautious_capacity::<u64>(), 0);
    /// ```
    #[inline]
    pub const fn cautious_capacity<T>(&self) -> usize {
        let max = match core::mem::size_of::<T>() {
            0 => MAX_PREALLOCATION,
            size => MAX_PREALLOCATION / size,
        };
        match self.len {
            UNKNOWN_LEN => 0,
            len if len < max => len,
            _ => max,
        }
    }

    /// Returns how significant the order of the elements is.
    #[inline]
    pub const fn order(&self) -> Order {
        Order::from_bits(self.flags)
    }

    /// Marks a map as a multimap: its keys can be given more than once.
    ///
    /// Formats where keys can repeat (like query strings with `a=1&a=2`,
    /// the elements of XML or the columns of CSV files) emit their maps
    /// with this flag and pass on every occurrence of a key as an entry of
    /// its own, in the order of the input.  How repeated keys are resolved
    /// is up to the type that receives the map:
    ///
    /// * The fields of derived structs and the values of maps whose type is
    ///   a collection (like `Vec<T>` or `HashSet<T>`) collect the values
    ///   of all occurrences of their key.  A key that is given once is a
    ///   collection of one value and a key that is missing is an empty
    ///   collection.
    /// * Other fields and values receive a single value,
    ///   [`DuplicateKeys`](crate::de::DuplicateKeys) in the
    ///   [`State`](crate::State) decides which one: the last one, the first
    ///   one or an error (the default).
    ///
    /// Types that do not know about multimaps receive the entries like
    /// those of any other map.  See [`State::is_multimap`](crate::State::is_multimap).
    ///
    /// ```
    /// use deser::de::DeserializeDriver;
    /// use deser::{Atom, ContainerShape, Deserialize, Event};
    ///
    /// #[derive(Deserialize, Debug, PartialEq)]
    /// struct Query {
    ///     tag: Vec<String>,
    ///     page: u32,
    ///     user: Vec<String>,
    /// }
    ///
    /// let mut out = None::<Query>;
    /// let mut driver = DeserializeDriver::new(&mut out);
    /// driver
    ///     .emit(Event::MapStart(ContainerShape::new().with_multimap(true)))
    ///     .unwrap();
    /// for (key, value) in [("tag", "a"), ("page", "1"), ("tag", "b")] {
    ///     driver.emit(key).unwrap();
    ///     driver.emit(Atom::Lexical(value.into())).unwrap();
    /// }
    /// driver.emit(Event::MapEnd).unwrap();
    /// drop(driver);
    /// assert_eq!(out, Some(Query {
    ///     tag: vec!["a".into(), "b".into()],
    ///     page: 1,
    ///     user: vec![],
    /// }));
    /// ```
    #[inline]
    pub const fn with_multimap(mut self, yes: bool) -> ContainerShape {
        if yes {
            self.flags |= MULTIMAP;
        } else {
            self.flags &= !MULTIMAP;
        }
        self
    }

    /// Returns `true` if the keys of the map can be given more than once.
    ///
    /// See [`with_multimap`](Self::with_multimap).
    #[inline]
    pub const fn is_multimap(&self) -> bool {
        self.flags & MULTIMAP != 0
    }
}

impl Default for ContainerShape {
    fn default() -> ContainerShape {
        ContainerShape::new()
    }
}

impl fmt::Debug for ContainerShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("ContainerShape");
        s.field("len", &self.len()).field("order", &self.order());
        // rare, only shown if set
        if self.flags & LEN_HINT != 0 && self.len != UNKNOWN_LEN {
            s.field("len_hint", &self.len);
        }
        if self.is_multimap() {
            s.field("is_multimap", &true);
        }
        s.finish()
    }
}

/// Removes the length from container starts, for tests.
#[cfg(test)]
pub(crate) fn without_len(event: Event<'static>) -> Event<'static> {
    match event {
        Event::MapStart(shape) => Event::MapStart(
            ContainerShape::new()
                .with_order(shape.order())
                .with_multimap(shape.is_multimap()),
        ),
        Event::SeqStart(shape) => Event::SeqStart(ContainerShape::new().with_order(shape.order())),
        event => event,
    }
}

// Every value goes through atoms and events, they have to stay small.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(core::mem::size_of::<Atom<'static>>() == 32);
    assert!(core::mem::size_of::<Event<'static>>() == 32);
    assert!(core::mem::size_of::<Implicit<'static>>() == 24);
};
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(core::mem::size_of::<Atom<'static>>() == 24);
    assert!(core::mem::size_of::<Event<'static>>() == 24);
    assert!(core::mem::size_of::<Implicit<'static>>() == 16);
};

#[test]
fn test_implicit_packing() {
    for value in [
        ImplicitValue::Null,
        ImplicitValue::Bool(false),
        ImplicitValue::Bool(true),
        ImplicitValue::U64(u64::MAX),
        ImplicitValue::I64(i64::MIN),
        ImplicitValue::I64(-1),
        ImplicitValue::F64(-0.0),
        ImplicitValue::F64(f64::NAN),
        ImplicitValue::F64(1.1),
    ] {
        for implicit in [
            Implicit::new("text", value),
            Implicit::new(String::from("text"), value),
        ] {
            let borrowed = implicit.text().is_borrowed();
            assert!(implicit.value().is_same(value));
            assert_eq!(implicit.text(), "text");
            assert_eq!(implicit.text().len(), 4);
            assert!(implicit.clone().value().is_same(value));
            assert!(implicit.as_borrowed().value().is_same(value));
            assert!(implicit.to_static().value().is_same(value));
            assert_eq!(implicit.clone().text().is_borrowed(), borrowed);
            let (text, inner) = implicit.into_parts();
            assert_eq!(text, "text");
            assert_eq!(text.tag(), 0);
            assert!(inner.is_same(value));
        }
    }
}
