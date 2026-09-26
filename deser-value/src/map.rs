use std::cmp::Ordering;
use std::collections::hash_map::{DefaultHasher, RandomState};
use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher};
use std::iter::FusedIterator;
use std::sync::OnceLock;

use deser::Order;
use indexmap::{Equivalent, IndexMap};

use crate::tree;
use crate::value::{Kind, Value};

/// Hashes the keys of maps.
///
/// The keys are random per process which protects against collisions
/// provoked by untrusted input.  Unlike [`RandomState`] this has no size
/// which keeps maps small.
#[derive(Clone, Copy, Default)]
pub(crate) struct MapHasher;

impl BuildHasher for MapHasher {
    type Hasher = DefaultHasher;

    #[inline]
    fn build_hasher(&self) -> DefaultHasher {
        static KEYS: OnceLock<RandomState> = OnceLock::new();
        KEYS.get_or_init(RandomState::new).build_hasher()
    }
}

pub(crate) type Entries = IndexMap<Value, Value, MapHasher>;

pub(crate) struct MapInner {
    pub(crate) entries: Entries,
    pub(crate) order: Order,
}

/// A map of values.
///
/// Maps retain the order in which the entries were inserted and every key
/// is unique.  Keys can be any value, not just strings.  Additionally a
/// map holds the [`Order`] of its entries which is passed on to formats when
/// the map is serialized.  The order is not considered when maps are
/// compared: maps are equal if they contain the same entries.  The
/// exception are entries with maps or sequences as keys, which are compared
/// in order (this allows comparing maps without recursion).
///
/// ```
/// use deser_value::{Map, Value};
///
/// let mut map = Map::new();
/// map.insert("name", "Jane");
/// map.insert(42, true);
/// assert_eq!(map.get("name"), Some(&Value::from("Jane")));
/// assert_eq!(map.get(&42), Some(&Value::from(true)));
/// assert_eq!(map.keys().collect::<Vec<_>>(), [&Value::from("name"), &Value::from(42)]);
/// ```
///
/// Keys can be looked up by anything that implements [`MapKey`], which
/// includes strings, integers and values.
pub struct Map {
    pub(crate) inner: Box<MapInner>,
}

impl Map {
    /// Creates an empty map.
    pub fn new() -> Map {
        Map::with_capacity(0)
    }

    /// Creates an empty map with a capacity.
    pub fn with_capacity(capacity: usize) -> Map {
        Map {
            inner: Box::new(MapInner {
                entries: IndexMap::with_capacity_and_hasher(capacity, MapHasher),
                order: Order::Natural,
            }),
        }
    }

    /// Returns the order of the entries.
    pub fn order(&self) -> Order {
        self.inner.order
    }

    /// Sets the order of the entries.
    pub fn set_order(&mut self, order: Order) {
        self.inner.order = order;
    }

    /// Sets the order of the entries and returns the map.
    pub fn with_order(mut self, order: Order) -> Map {
        self.inner.order = order;
        self
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.inner.entries.len()
    }

    /// Returns `true` if the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.entries.is_empty()
    }

    /// Returns the value of a key.
    pub fn get<K: MapKey + ?Sized>(&self, key: &K) -> Option<&Value> {
        self.inner.entries.get(&key.__key_ref())
    }

    /// Returns the value of a key mutably.
    pub fn get_mut<K: MapKey + ?Sized>(&mut self, key: &K) -> Option<&mut Value> {
        self.inner.entries.get_mut(&key.__key_ref())
    }

    /// Returns the key and value of a key.
    ///
    /// This is useful to get to the meta data of the key.
    pub fn get_key_value<K: MapKey + ?Sized>(&self, key: &K) -> Option<(&Value, &Value)> {
        self.inner.entries.get_key_value(&key.__key_ref())
    }

    /// Returns the index of a key.
    pub fn get_index_of<K: MapKey + ?Sized>(&self, key: &K) -> Option<usize> {
        self.inner.entries.get_index_of(&key.__key_ref())
    }

    /// Returns the key and value at an index.
    pub fn get_index(&self, index: usize) -> Option<(&Value, &Value)> {
        self.inner.entries.get_index(index)
    }

    /// Returns the key and the mutable value at an index.
    pub fn get_index_mut(&mut self, index: usize) -> Option<(&Value, &mut Value)> {
        self.inner.entries.get_index_mut(index)
    }

    /// Returns `true` if the map contains a key.
    pub fn contains_key<K: MapKey + ?Sized>(&self, key: &K) -> bool {
        self.inner.entries.contains_key(&key.__key_ref())
    }

    /// Inserts a value for a key.
    ///
    /// If the key already exists, its value is replaced (the entry keeps its
    /// position and key) and the old value is returned.  Otherwise the entry
    /// is added at the end.
    pub fn insert<K: Into<Value>, V: Into<Value>>(&mut self, key: K, value: V) -> Option<Value> {
        self.inner.entries.insert(key.into(), value.into())
    }

    /// Returns the value of a key, inserting the result of `f` if the key
    /// does not exist.
    pub fn get_or_insert_with<K: Into<Value>, F: FnOnce() -> Value>(
        &mut self,
        key: K,
        f: F,
    ) -> &mut Value {
        self.inner.entries.entry(key.into()).or_insert_with(f)
    }

    /// Removes a key and returns its value.
    ///
    /// The order of the remaining entries is retained.
    pub fn remove<K: MapKey + ?Sized>(&mut self, key: &K) -> Option<Value> {
        self.inner.entries.shift_remove(&key.__key_ref())
    }

    /// Removes a key and returns it together with its value.
    ///
    /// The order of the remaining entries is retained.
    pub fn remove_entry<K: MapKey + ?Sized>(&mut self, key: &K) -> Option<(Value, Value)> {
        self.inner.entries.shift_remove_entry(&key.__key_ref())
    }

    /// Retains the entries for which the predicate returns `true`.
    pub fn retain<F: FnMut(&Value, &mut Value) -> bool>(&mut self, f: F) {
        self.inner.entries.retain(f);
    }

    /// Sorts the entries with a comparison function.
    pub fn sort_by<F>(&mut self, mut f: F)
    where
        F: FnMut(&Value, &Value, &Value, &Value) -> Ordering,
    {
        self.inner
            .entries
            .sort_by(|k1, v1, k2, v2| f(k1, v1, k2, v2));
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        if self
            .inner
            .entries
            .iter()
            .any(|(k, v)| k.has_children() || v.has_children())
        {
            let entries = std::mem::take(&mut self.inner.entries);
            tree::drop_entries(entries);
        } else {
            self.inner.entries.clear();
        }
    }

    /// Returns an iterator over the entries.
    pub fn iter(&self) -> Iter<'_> {
        Iter(self.inner.entries.iter())
    }

    /// Returns an iterator over the entries with mutable values.
    pub fn iter_mut(&mut self) -> IterMut<'_> {
        IterMut(self.inner.entries.iter_mut())
    }

    /// Returns an iterator over the keys.
    pub fn keys(&self) -> Keys<'_> {
        Keys(self.inner.entries.keys())
    }

    /// Returns an iterator over the values.
    pub fn values(&self) -> Values<'_> {
        Values(self.inner.entries.values())
    }

    /// Returns an iterator over the mutable values.
    pub fn values_mut(&mut self) -> ValuesMut<'_> {
        ValuesMut(self.inner.entries.values_mut())
    }
}

impl Default for Map {
    fn default() -> Map {
        Map::new()
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        if self
            .inner
            .entries
            .iter()
            .any(|(k, v)| k.has_children() || v.has_children())
        {
            tree::drop_entries(std::mem::take(&mut self.inner.entries));
        }
    }
}

impl Clone for Map {
    fn clone(&self) -> Map {
        match tree::clone_map(self) {
            Kind::Map(map) => map,
            _ => unreachable!(),
        }
    }
}

impl PartialEq for Map {
    fn eq(&self, other: &Map) -> bool {
        tree::eq_map(self, other)
    }
}

impl Eq for Map {}

impl Hash for Map {
    fn hash<H: Hasher>(&self, state: &mut H) {
        tree::hash_map(self, state);
    }
}

impl fmt::Debug for Map {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        tree::fmt_map(self, f)
    }
}

impl<K: Into<Value>, V: Into<Value>> FromIterator<(K, V)> for Map {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Map {
        let iter = iter.into_iter();
        let mut map = Map::with_capacity(iter.size_hint().0);
        map.extend(iter);
        map
    }
}

impl<K: Into<Value>, V: Into<Value>> Extend<(K, V)> for Map {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

macro_rules! iterator {
    ($(#[$attr:meta])* $name:ident<$lt:lifetime>, $inner:ty, $item:ty) => {
        $(#[$attr])*
        pub struct $name<$lt>($inner);

        impl<$lt> Iterator for $name<$lt> {
            type Item = $item;

            #[inline]
            fn next(&mut self) -> Option<$item> {
                self.0.next()
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                self.0.size_hint()
            }
        }

        impl<$lt> DoubleEndedIterator for $name<$lt> {
            #[inline]
            fn next_back(&mut self) -> Option<$item> {
                self.0.next_back()
            }
        }

        impl<$lt> ExactSizeIterator for $name<$lt> {}

        impl<$lt> FusedIterator for $name<$lt> {}
    };
}

iterator!(
    /// An iterator over the entries of a [`Map`].
    Iter<'a>, indexmap::map::Iter<'a, Value, Value>, (&'a Value, &'a Value)
);
iterator!(
    /// An iterator over the entries of a [`Map`] with mutable values.
    IterMut<'a>, indexmap::map::IterMut<'a, Value, Value>, (&'a Value, &'a mut Value)
);
iterator!(
    /// An iterator over the keys of a [`Map`].
    Keys<'a>, indexmap::map::Keys<'a, Value, Value>, &'a Value
);
iterator!(
    /// An iterator over the values of a [`Map`].
    Values<'a>, indexmap::map::Values<'a, Value, Value>, &'a Value
);
iterator!(
    /// An iterator over the mutable values of a [`Map`].
    ValuesMut<'a>, indexmap::map::ValuesMut<'a, Value, Value>, &'a mut Value
);

/// An owning iterator over the entries of a [`Map`].
pub struct IntoIter(indexmap::map::IntoIter<Value, Value>);

impl Iterator for IntoIter {
    type Item = (Value, Value);

    #[inline]
    fn next(&mut self) -> Option<(Value, Value)> {
        self.0.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for IntoIter {
    #[inline]
    fn next_back(&mut self) -> Option<(Value, Value)> {
        self.0.next_back()
    }
}

impl ExactSizeIterator for IntoIter {}

impl FusedIterator for IntoIter {}

impl IntoIterator for Map {
    type Item = (Value, Value);
    type IntoIter = IntoIter;

    fn into_iter(mut self) -> IntoIter {
        IntoIter(std::mem::take(&mut self.inner.entries).into_iter())
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'a Value, &'a Value);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Map {
    type Item = (&'a Value, &'a mut Value);
    type IntoIter = IterMut<'a>;

    fn into_iter(self) -> IterMut<'a> {
        self.iter_mut()
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A type that map keys can be looked up with.
///
/// This is implemented for strings, integers, bools, chars and
/// [`Value`]s.  Looking up keys with it does not require the key to be
/// converted into a value first.
pub trait MapKey: sealed::Sealed {
    #[doc(hidden)]
    fn __key_ref(&self) -> KeyRef<'_>;
}

/// A borrowed map key.
#[doc(hidden)]
pub enum KeyRef<'a> {
    Str(&'a str),
    Int(i128),
    Bool(bool),
    Char(char),
    Value(&'a Value),
}

impl Hash for KeyRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            KeyRef::Str(value) => tree::hash_str(value, state),
            KeyRef::Int(value) => tree::hash_int(value, state),
            KeyRef::Bool(value) => tree::hash_bool(value, state),
            KeyRef::Char(value) => tree::hash_char(value, state),
            KeyRef::Value(value) => value.hash(state),
        }
    }
}

impl Equivalent<Value> for KeyRef<'_> {
    fn equivalent(&self, key: &Value) -> bool {
        match (self, &key.kind) {
            (KeyRef::Str(a), Kind::Str(b)) => *a == b,
            (KeyRef::Int(a), Kind::U64(b)) => *a == i128::from(*b),
            (KeyRef::Int(a), Kind::I64(b)) => *a == i128::from(*b),
            (KeyRef::Bool(a), Kind::Bool(b)) => a == b,
            (KeyRef::Char(a), Kind::Char(b)) => a == b,
            (KeyRef::Value(a), _) => *a == key,
            _ => false,
        }
    }
}

impl sealed::Sealed for str {}

impl MapKey for str {
    fn __key_ref(&self) -> KeyRef<'_> {
        KeyRef::Str(self)
    }
}

impl sealed::Sealed for String {}

impl MapKey for String {
    fn __key_ref(&self) -> KeyRef<'_> {
        KeyRef::Str(self)
    }
}

impl sealed::Sealed for bool {}

impl MapKey for bool {
    fn __key_ref(&self) -> KeyRef<'_> {
        KeyRef::Bool(*self)
    }
}

impl sealed::Sealed for char {}

impl MapKey for char {
    fn __key_ref(&self) -> KeyRef<'_> {
        KeyRef::Char(*self)
    }
}

impl sealed::Sealed for Value {}

impl MapKey for Value {
    fn __key_ref(&self) -> KeyRef<'_> {
        KeyRef::Value(self)
    }
}

impl<T: MapKey + ?Sized> sealed::Sealed for &T {}

impl<T: MapKey + ?Sized> MapKey for &T {
    fn __key_ref(&self) -> KeyRef<'_> {
        (**self).__key_ref()
    }
}

macro_rules! int_key {
    ($($ty:ty),*) => {
        $(
            impl sealed::Sealed for $ty {}

            impl MapKey for $ty {
                fn __key_ref(&self) -> KeyRef<'_> {
                    KeyRef::Int(*self as i128)
                }
            }
        )*
    };
}

int_key!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);
