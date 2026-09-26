use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut, Range};
use std::sync::Arc;

use deser::ext::{BorrowedExtension, ExtValue, Extension};
use deser::{Atom, Bytes, EventData, Position};

use crate::index::ValueIndex;
use crate::map::Map;
use crate::seq::Seq;
use crate::tree;

/// A dynamic value.
///
/// A value is made of its [`Kind`], which holds the actual data, and
/// optional [`Meta`] data.  The value dereferences to its kind, which is
/// where the accessors are defined and what is matched on:
///
/// ```
/// use deser_value::{value, Kind};
///
/// let value = value!({"name": "Jane", "roles": ["admin"]});
/// assert_eq!(value["name"].as_str(), Some("Jane"));
///
/// match &*value["roles"] {
///     Kind::Seq(roles) => assert_eq!(roles.len(), 1),
///     _ => unreachable!(),
/// }
/// ```
///
/// # Meta Data
///
/// Values retain information that is not part of the data model.  This is
/// held in the [`Meta`] of a value, which is only allocated if there is
/// such information:
///
/// * The [event data](deser::State::event) of the value, for instance CBOR
///   tags or formatting hints.  It's captured when the value is
///   deserialized and attached again when it's serialized.
/// * The [`Span`] of the value in the input, if the format tracks
///   locations.  Values that are deserialized from a value with spans report
///   errors at the original location.
///
/// Meta data is ignored when values are compared or hashed and it's not
/// shown in the debug output.
///
/// # Numbers
///
/// Integers are held as [`Kind::U64`] and [`Kind::I64`].  Values created by
/// this crate hold non-negative integers as `U64` and integers which do not
/// fit into 64 bits (`u128` and `i128`) as [`Kind::Ext`].  Integers compare
/// by their value: `I64(1)` is equal to `U64(1)`.  Floats compare by
/// their bits, which means that `NaN` is equal to itself and `-0.0` is not
/// equal to `0.0`.
///
/// # Extensions
///
/// Values which extend the data model (see [`deser::ext`]) are held as
/// [`Kind::Ext`].  They retain their type and are serialized as extension
/// values again.  The accessors (such as [`as_str`](Kind::as_str)) look at
/// them through their fallback.
///
/// # Nesting
///
/// All operations on values (including dropping, cloning, comparing and
/// formatting) are implemented without recursion, deeply nested values do
/// not overflow the stack.
pub struct Value {
    pub(crate) kind: Kind,
    pub(crate) meta: Option<Box<Meta>>,
}

/// The data of a [`Value`].
///
/// See [`Value`] for more information.
#[non_exhaustive]
pub enum Kind {
    Null,
    Bool(bool),
    /// An unsigned integer.
    U64(u64),
    /// A signed integer.
    ///
    /// Values created by this crate only use this for negative integers.
    I64(i64),
    /// A single precision float.
    ///
    /// It keeps the precision for serialization (`0.1f32` is written as
    /// `0.1`), otherwise it behaves like the same value as `F64`: they
    /// compare equal and hash the same.
    F32(f32),
    /// A double precision float.
    F64(f64),
    Char(char),
    Str(String),
    /// The lexical form of a value whose type the format cannot express.
    ///
    /// This is created from [`Atom::Lexical`], for instance for the keys of
    /// JSON objects.  It's serialized as lexical atom again, so types that
    /// parse lexical atoms (like numbers) can be deserialized from it.
    /// Otherwise it behaves like a string: it compares equal to and hashes
    /// like the same [`Str`](Kind::Str) and the accessors for strings
    /// return it.
    Lexical(String),
    Bytes(Bytes<'static>),
    /// A value extending the data model.
    Ext(ExtValue<'static>),
    Seq(Seq),
    Map(Map),
}

/// Meta data of a [`Value`].
///
/// See [`Value`] for more information.
#[derive(Clone, Default, Debug)]
pub struct Meta {
    event_data: EventData,
    span: Option<Span>,
}

impl Meta {
    /// Creates empty meta data.
    pub fn new() -> Meta {
        Meta::default()
    }

    /// Returns the event data of the value.
    ///
    /// See [`EventData`] and [`deser::State::event`].
    pub fn event_data(&self) -> &EventData {
        &self.event_data
    }

    /// Returns the event data of the value mutably.
    pub fn event_data_mut(&mut self) -> &mut EventData {
        &mut self.event_data
    }

    /// Returns the span of the value in the input.
    pub fn span(&self) -> Option<&Span> {
        self.span.as_ref()
    }

    /// Sets the span of the value.
    pub fn set_span(&mut self, span: Option<Span>) {
        self.span = span;
    }

    /// Returns `true` if the meta data is empty.
    pub fn is_empty(&self) -> bool {
        self.event_data.is_empty() && self.span.is_none()
    }

    pub(crate) fn from_parts(event_data: EventData, span: Option<Span>) -> Meta {
        Meta { event_data, span }
    }

    pub(crate) fn span_mut(&mut self) -> Option<&mut Span> {
        self.span.as_mut()
    }
}

/// The location of a value in its input.
///
/// Spans are captured when values are deserialized from a format that
/// tracks locations (see [`deser::State::source`]), for instance with
/// the `track_locations` option of `deser-json`.
#[derive(Clone)]
pub struct Span {
    range: Range<usize>,
    // for maps and sequences: the end of the start event and the start of
    // the end event.
    pub(crate) inner: Option<(usize, usize)>,
    source: Arc<str>,
}

impl Span {
    /// Creates a span of a range in a source.
    pub fn new(range: Range<usize>, source: Arc<str>) -> Span {
        Span {
            range,
            inner: None,
            source,
        }
    }

    /// Returns the byte range in the source.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Returns the source.
    pub fn source(&self) -> &Arc<str> {
        &self.source
    }

    /// Returns the text of the span in the source.
    pub fn text(&self) -> Option<&str> {
        self.source.get(self.range.clone())
    }

    /// Returns the position (with line and column) of the start of the span.
    pub fn start(&self) -> Position {
        Position::of(self.source.as_bytes(), self.range.start)
    }

    /// Returns the position (with line and column) of the end of the span.
    ///
    /// The end is exclusive.
    pub fn end(&self) -> Position {
        Position::of(self.source.as_bytes(), self.range.end)
    }

    pub(crate) fn start_range(&self) -> (usize, usize) {
        match self.inner {
            Some((start_end, _)) => (self.range.start, start_end),
            None => (self.range.start, self.range.end),
        }
    }

    pub(crate) fn end_range(&self) -> Option<(usize, usize)> {
        self.inner.map(|(_, end_start)| (end_start, self.range.end))
    }

    pub(crate) fn set_end(&mut self, end_start: usize, end: usize) {
        self.inner = Some((self.range.end, end_start));
        self.range.end = end;
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Span")
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

static NULL: Value = Value::null();

impl Value {
    /// Creates a null value.
    pub const fn null() -> Value {
        Value {
            kind: Kind::Null,
            meta: None,
        }
    }

    /// Creates a value from its kind.
    pub const fn new(kind: Kind) -> Value {
        Value { kind, meta: None }
    }

    /// Creates a value holding an extension value.
    ///
    /// ```
    /// use deser::ext::Uuid;
    /// use deser_value::Value;
    ///
    /// let value = Value::ext(Uuid([0; 16]));
    /// assert!(value.downcast_ext::<Uuid>().is_some());
    /// ```
    pub fn ext<T: Extension>(value: T) -> Value {
        Value::from(ExtValue::owned(value))
    }

    /// Creates a value holding bytes.
    pub fn bytes<B: Into<Vec<u8>>>(data: B) -> Value {
        Value::new(Kind::Bytes(Bytes::new(data.into())))
    }

    /// Returns the kind of the value.
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    /// Returns the kind of the value mutably.
    pub fn kind_mut(&mut self) -> &mut Kind {
        &mut self.kind
    }

    /// Converts the value into its kind, discarding the meta data.
    pub fn into_kind(self) -> Kind {
        self.kind
    }

    /// Converts the value into its kind and meta data.
    pub fn into_parts(self) -> (Kind, Option<Meta>) {
        (self.kind, self.meta.map(|meta| *meta))
    }

    /// Returns the meta data of the value, if there is any.
    pub fn meta(&self) -> Option<&Meta> {
        self.meta.as_deref()
    }

    /// Returns the meta data of the value mutably.
    ///
    /// Empty meta data is created if the value has none.
    pub fn meta_mut(&mut self) -> &mut Meta {
        self.meta.get_or_insert_with(Default::default)
    }

    /// Removes the meta data of the value and returns it.
    pub fn take_meta(&mut self) -> Option<Meta> {
        self.meta.take().map(|meta| *meta)
    }

    /// Sets the meta data of the value.
    pub fn set_meta(&mut self, meta: Option<Meta>) {
        self.meta = meta.filter(|meta| !meta.is_empty()).map(Box::new);
    }

    /// Sets the meta data of the value and returns it.
    pub fn with_meta(mut self, meta: Meta) -> Value {
        self.set_meta(Some(meta));
        self
    }

    /// Returns the span of the value in the input.
    ///
    /// This is a shortcut for the span of the [`Meta`].
    pub fn span(&self) -> Option<&Span> {
        self.meta().and_then(Meta::span)
    }

    /// Returns the event data of the value.
    ///
    /// This is a shortcut for the event data of the [`Meta`].
    pub fn event_data(&self) -> Option<&EventData> {
        self.meta().map(Meta::event_data)
    }

    /// Takes the value out, leaving null in its place.
    pub fn take(&mut self) -> Value {
        std::mem::take(self)
    }
}

impl Default for Value {
    fn default() -> Value {
        Value::null()
    }
}

impl Deref for Value {
    type Target = Kind;

    fn deref(&self) -> &Kind {
        &self.kind
    }
}

impl DerefMut for Value {
    fn deref_mut(&mut self) -> &mut Kind {
        &mut self.kind
    }
}

impl Clone for Value {
    fn clone(&self) -> Value {
        Value {
            kind: self.kind.clone(),
            meta: self.meta.clone(),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        tree::eq_kind(&self.kind, &other.kind)
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        tree::hash_kind(&self.kind, state);
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        tree::fmt_kind(&self.kind, f)
    }
}

impl<I: ValueIndex> std::ops::Index<I> for Value {
    type Output = Value;

    /// Looks up a value in a sequence (by index) or a map (by key).
    ///
    /// Returns null if the value does not exist.
    fn index(&self, index: I) -> &Value {
        index.index_into(&self.kind).unwrap_or(&NULL)
    }
}

impl<I: ValueIndex> std::ops::IndexMut<I> for Value {
    /// Looks up a value in a sequence (by index) or a map (by key) mutably.
    ///
    /// Keys that do not exist are inserted with null and a null value turns
    /// into a map if it's indexed by a key.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds, or if the value cannot be
    /// indexed by the index.
    fn index_mut(&mut self, index: I) -> &mut Value {
        index.index_or_insert(self)
    }
}

impl Kind {
    /// Returns the human readable name of the kind.
    pub fn name(&self) -> &str {
        match self {
            Kind::Null => "null",
            Kind::Bool(_) => "bool",
            Kind::U64(_) => "unsigned integer",
            Kind::I64(_) => "signed integer",
            Kind::F32(_) | Kind::F64(_) => "float",
            Kind::Char(_) => "char",
            Kind::Str(_) | Kind::Lexical(_) => "string",
            Kind::Bytes(_) => "bytes",
            Kind::Ext(ext) => ext.name(),
            Kind::Seq(_) => "sequence",
            Kind::Map(_) => "map",
        }
    }

    /// Returns `true` if this is null.
    ///
    /// Extension values that fall back to null count as null.
    pub fn is_null(&self) -> bool {
        match self {
            Kind::Null => true,
            Kind::Ext(ext) => matches!(ext.fallback(), Atom::Null),
            _ => false,
        }
    }

    /// Returns the value of a bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Kind::Bool(value) => Some(*value),
            Kind::Ext(ext) => match ext.fallback() {
                Atom::Bool(value) => Some(value),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns the value of an integer if it fits into `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        self.as_i128().and_then(|value| u64::try_from(value).ok())
    }

    /// Returns the value of an integer if it fits into `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        self.as_i128().and_then(|value| i64::try_from(value).ok())
    }

    /// Returns the value of an integer if it fits into `u128`.
    pub fn as_u128(&self) -> Option<u128> {
        match self {
            Kind::Ext(ext) if ext.is::<u128>() => ext.downcast_ref::<u128>().copied(),
            _ => self.as_i128().and_then(|value| u128::try_from(value).ok()),
        }
    }

    /// Returns the value of an integer if it fits into `i128`.
    pub fn as_i128(&self) -> Option<i128> {
        match self {
            Kind::U64(value) => Some(i128::from(*value)),
            Kind::I64(value) => Some(i128::from(*value)),
            Kind::Ext(ext) => {
                if let Some(value) = ext.downcast_ref::<i128>() {
                    Some(*value)
                } else if let Some(value) = ext.downcast_ref::<u128>() {
                    i128::try_from(*value).ok()
                } else {
                    match ext.fallback() {
                        Atom::U64(value) => Some(i128::from(value)),
                        Atom::I64(value) => Some(i128::from(value)),
                        _ => None,
                    }
                }
            }
            _ => None,
        }
    }

    /// Returns the value of a number as `f64`.
    ///
    /// Integers are converted, which can lose precision.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Kind::F64(value) => Some(*value),
            Kind::F32(value) => Some(f64::from(*value)),
            Kind::U64(value) => Some(*value as f64),
            Kind::I64(value) => Some(*value as f64),
            Kind::Ext(ext) => match ext.fallback() {
                Atom::F64(value) => Some(value),
                Atom::F32(value) => Some(f64::from(value)),
                Atom::U64(value) => Some(value as f64),
                Atom::I64(value) => Some(value as f64),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns the value of a char.
    pub fn as_char(&self) -> Option<char> {
        match self {
            Kind::Char(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the value of a string.
    ///
    /// For extension values this returns the fallback if it's a string
    /// that borrows from the value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Kind::Str(value) | Kind::Lexical(value) => Some(value),
            Kind::Ext(ext) => match ext.fallback() {
                Atom::Str(Cow::Borrowed(value)) => Some(value),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns the data of bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Kind::Bytes(value) => Some(value.data()),
            _ => None,
        }
    }

    /// Returns the sequence.
    pub fn as_seq(&self) -> Option<&Seq> {
        match self {
            Kind::Seq(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the sequence mutably.
    pub fn as_seq_mut(&mut self) -> Option<&mut Seq> {
        match self {
            Kind::Seq(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the map.
    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Kind::Map(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the map mutably.
    pub fn as_map_mut(&mut self) -> Option<&mut Map> {
        match self {
            Kind::Map(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the extension value.
    pub fn as_ext(&self) -> Option<&ExtValue<'static>> {
        match self {
            Kind::Ext(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the extension value if it's of type `T`.
    ///
    /// For extensions that borrow, use
    /// [`downcast_ext_value`](Self::downcast_ext_value).
    pub fn downcast_ext<T: Extension>(&self) -> Option<&T> {
        self.as_ext().and_then(|ext| ext.downcast_ref::<T>())
    }

    /// Returns the extension value if it's of the extension with the key `K`.
    ///
    /// See [`BorrowedExtension`].
    ///
    /// ```
    /// use deser::ext::Number;
    /// use deser_value::Value;
    ///
    /// let value: Value = deser_json::from_str("0.10000000000000000001").unwrap();
    /// let number = value.downcast_ext_value::<Number>().unwrap();
    /// assert_eq!(number.as_str(), "0.10000000000000000001");
    /// ```
    pub fn downcast_ext_value<K: BorrowedExtension>(&self) -> Option<&K::Value<'_>> {
        self.as_ext().and_then(|ext| ext.downcast_value_ref::<K>())
    }

    /// Returns `true` if this is a string.
    ///
    /// This is also `true` for [lexical](Kind::Lexical) values.
    pub fn is_str(&self) -> bool {
        matches!(self, Kind::Str(_) | Kind::Lexical(_))
    }

    /// Returns `true` if this is a [lexical](Kind::Lexical) value.
    pub fn is_lexical(&self) -> bool {
        matches!(self, Kind::Lexical(_))
    }

    /// Returns `true` if this is a sequence.
    pub fn is_seq(&self) -> bool {
        matches!(self, Kind::Seq(_))
    }

    /// Returns `true` if this is a map.
    pub fn is_map(&self) -> bool {
        matches!(self, Kind::Map(_))
    }

    /// Looks up a value in a sequence (by index) or a map (by key).
    ///
    /// ```
    /// use deser_value::value;
    ///
    /// let value = value!({"items": [1, 2], 42: "answer"});
    /// assert_eq!(value.get("items").and_then(|x| x.get(1)), Some(&value!(2)));
    /// assert_eq!(value.get(&value!(42)), Some(&value!("answer")));
    /// assert_eq!(value.get("missing"), None);
    /// ```
    pub fn get<I: ValueIndex>(&self, index: I) -> Option<&Value> {
        index.index_into(self)
    }

    /// Looks up a value in a sequence (by index) or a map (by key) mutably.
    pub fn get_mut<I: ValueIndex>(&mut self, index: I) -> Option<&mut Value> {
        index.index_into_mut(self)
    }

    /// Returns `true` for maps and sequences that are not empty.
    #[inline]
    pub(crate) fn has_children(&self) -> bool {
        match self {
            Kind::Seq(seq) => !seq.is_empty(),
            Kind::Map(map) => !map.is_empty(),
            _ => false,
        }
    }
}

impl Clone for Kind {
    fn clone(&self) -> Kind {
        tree::clone_kind(self)
    }
}

impl PartialEq for Kind {
    fn eq(&self, other: &Kind) -> bool {
        tree::eq_kind(self, other)
    }
}

impl Eq for Kind {}

impl Hash for Kind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        tree::hash_kind(self, state);
    }
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        tree::fmt_kind(self, f)
    }
}

impl From<Kind> for Value {
    fn from(kind: Kind) -> Value {
        Value::new(kind)
    }
}

impl From<()> for Value {
    fn from(_: ()) -> Value {
        Value::null()
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::new(Kind::Bool(value))
    }
}

impl From<char> for Value {
    fn from(value: char) -> Value {
        Value::new(Kind::Char(value))
    }
}

macro_rules! from_unsigned {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for Value {
                fn from(value: $ty) -> Value {
                    Value::new(Kind::U64(value as u64))
                }
            }
        )*
    };
}

macro_rules! from_signed {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for Value {
                fn from(value: $ty) -> Value {
                    Value::new(Kind::from_i64(value as i64))
                }
            }
        )*
    };
}

from_unsigned!(u8, u16, u32, u64, usize);
from_signed!(i8, i16, i32, i64, isize);

impl Kind {
    /// Creates an integer, non-negative integers are held as `U64`.
    pub(crate) fn from_i64(value: i64) -> Kind {
        match u64::try_from(value) {
            Ok(value) => Kind::U64(value),
            Err(_) => Kind::I64(value),
        }
    }

    /// Creates a kind from an extension value.
    ///
    /// Integers of extensions which fit into 64 bits are converted.
    pub(crate) fn from_ext(ext: ExtValue<'_>) -> Kind {
        if let Some(&value) = ext.downcast_ref::<u128>() {
            if let Ok(value) = u64::try_from(value) {
                return Kind::U64(value);
            }
        } else if let Some(&value) = ext.downcast_ref::<i128>()
            && let Ok(value) = i64::try_from(value)
        {
            return Kind::from_i64(value);
        }
        Kind::Ext(ext.to_static())
    }
}

impl From<u128> for Value {
    fn from(value: u128) -> Value {
        Value::new(match u64::try_from(value) {
            Ok(value) => Kind::U64(value),
            Err(_) => Kind::Ext(ExtValue::owned(value)),
        })
    }
}

impl From<i128> for Value {
    fn from(value: i128) -> Value {
        Value::new(match i64::try_from(value) {
            Ok(value) => Kind::from_i64(value),
            Err(_) => Kind::Ext(ExtValue::owned(value)),
        })
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Value {
        Value::new(Kind::F32(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        Value::new(Kind::F64(value))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::new(Kind::Str(value.to_string()))
    }
}

impl From<String> for Value {
    fn from(value: String) -> Value {
        Value::new(Kind::Str(value))
    }
}

impl From<&String> for Value {
    fn from(value: &String) -> Value {
        Value::new(Kind::Str(value.clone()))
    }
}

impl From<Cow<'_, str>> for Value {
    fn from(value: Cow<'_, str>) -> Value {
        Value::new(Kind::Str(value.into_owned()))
    }
}

impl From<Bytes<'_>> for Value {
    fn from(value: Bytes<'_>) -> Value {
        Value::new(Kind::Bytes(owned_bytes(value)))
    }
}

impl From<ExtValue<'_>> for Value {
    /// Creates a value from an extension value.
    ///
    /// Integers of extensions (`u128` and `i128`) which fit into 64 bits
    /// are converted into `U64` and `I64`.
    fn from(value: ExtValue<'_>) -> Value {
        Value::new(Kind::from_ext(value))
    }
}

impl From<Seq> for Value {
    fn from(value: Seq) -> Value {
        Value::new(Kind::Seq(value))
    }
}

impl From<Map> for Value {
    fn from(value: Map) -> Value {
        Value::new(Kind::Map(value))
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Value {
        match value {
            Some(value) => value.into(),
            None => Value::null(),
        }
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(value: Vec<T>) -> Value {
        Value::from(value.into_iter().collect::<Seq>())
    }
}

impl<T: Clone + Into<Value>> From<&[T]> for Value {
    fn from(value: &[T]) -> Value {
        Value::from(value.iter().cloned().collect::<Seq>())
    }
}

impl<T: Into<Value>> FromIterator<T> for Value {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Value {
        Value::from(iter.into_iter().collect::<Seq>())
    }
}

impl<K: Into<Value>, V: Into<Value>> FromIterator<(K, V)> for Value {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Value {
        Value::from(iter.into_iter().collect::<Map>())
    }
}

/// Converts bytes into owned bytes, retaining the fallback.
pub(crate) fn owned_bytes(bytes: Bytes<'_>) -> Bytes<'static> {
    let fallback = bytes.fallback;
    let owned = Bytes::new(bytes.into_owned());
    match fallback {
        Some(format) => owned.with_fallback(format),
        None => owned,
    }
}

impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        matches!(self.kind, Kind::Str(ref value) | Kind::Lexical(ref value) if value == other)
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl PartialEq<String> for Value {
    fn eq(&self, other: &String) -> bool {
        *self == **other
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        matches!(self.kind, Kind::Bool(value) if value == *other)
    }
}

impl PartialEq<f64> for Value {
    fn eq(&self, other: &f64) -> bool {
        match self.kind {
            Kind::F64(value) => value == *other,
            Kind::F32(value) => f64::from(value) == *other,
            _ => false,
        }
    }
}

impl PartialEq<f32> for Value {
    fn eq(&self, other: &f32) -> bool {
        *self == f64::from(*other)
    }
}

macro_rules! eq_int {
    ($($ty:ty),*) => {
        $(
            impl PartialEq<$ty> for Value {
                fn eq(&self, other: &$ty) -> bool {
                    match self.kind {
                        Kind::U64(value) => i128::from(value) == *other as i128,
                        Kind::I64(value) => i128::from(value) == *other as i128,
                        _ => false,
                    }
                }
            }
        )*
    };
}

eq_int!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);
