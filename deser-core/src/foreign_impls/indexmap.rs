//! `IndexMap` and `IndexSet` of `indexmap`.
//!
//! They are serialized like `HashMap` and `HashSet` except that the entries
//! are emitted in their order (which is the natural order of maps and
//! sequences).  When deserialized, the entries keep the order of the data.
//!
//! Written as adapters (like `IndexMap<KA, VA>`), the hasher is not used.
use core::hash::{BuildHasher, Hash};

use ::indexmap::{IndexMap, IndexSet, map};

use alloc::borrow::Cow;

use crate::State;
use crate::adapters::{MapSkipError, skip_map_sink};
use crate::de::impls::{
    MapOut, MapTarget, SetTarget, collection_methods, map_sink, set_collection, set_sink,
};
use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::event::ContainerShape;
use crate::ser::impls::{serialize_map, serialize_set};
use crate::ser::{Chunk, Describe, Serialize};

// the hasher of the adapter is not used
serialize_map! {
    [K, V, S: Sync, KA, VA, AS: Sync] IndexMap<K, V, S> => IndexMap<KA, VA, AS>, Natural;
}

impl<K, V, S, KA, VA> Serialize<IndexMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Sync,
    V: Sync,
    S: Sync,
    KA: Serialize<K>,
    VA: Serialize<V>,
{
    fn serialize<'a>(value: &'a IndexMap<K, V, S>, state: &mut State) -> Result<Chunk<'a>, Error> {
        <IndexMap<KA, VA, S> as Serialize<IndexMap<K, V, S>>>::serialize(value, state)
    }

    fn container_shape(value: &IndexMap<K, V, S>) -> ContainerShape {
        <IndexMap<KA, VA, S> as Serialize<IndexMap<K, V, S>>>::container_shape(value)
    }

    fn describe(value: &IndexMap<K, V, S>, d: &mut dyn Describe) {
        <IndexMap<KA, VA, S> as Serialize<IndexMap<K, V, S>>>::describe(value, d)
    }
}

impl<K, V, S> MapTarget<K, V> for IndexMap<K, V, S>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
{
    const NAME: &'static str = "IndexMap";

    #[inline]
    fn insert_entry(&mut self, key: K, value: V, replace: bool) -> bool {
        match self.entry(key) {
            map::Entry::Vacant(entry) => {
                entry.insert(value);
                false
            }
            map::Entry::Occupied(mut entry) => {
                if replace {
                    entry.insert(value);
                }
                true
            }
        }
    }

    #[inline]
    fn reserve_entries(&mut self, additional: usize) {
        self.reserve(additional);
    }

    #[inline]
    fn entry_mut(&mut self, key: &K) -> Option<&mut V> {
        self.get_mut(key)
    }

    fn merge(&mut self, other: Self) {
        if self.is_empty() {
            *self = other;
        } else {
            // existing keys keep their position, new ones are appended
            self.extend(other);
        }
    }
}

// the hasher of the adapter is not used
impl<'de, K, V, S, KA, VA, AS> Deserialize<'de, IndexMap<K, V, S>> for IndexMap<KA, VA, AS>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
    AS: Send,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<IndexMap<K, V, S>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<IndexMap<K, V, S> as MapTarget<K, V>>::NAME)
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).  New keys are appended.
    fn deserialize_update<'out>(
        value: &'out mut IndexMap<K, V, S>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Update(value), state)
    }
}

impl<'de, K, V, S, KA, VA> Deserialize<'de, IndexMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    fn deserialize_into<'out>(
        out: &'out mut Option<IndexMap<K, V, S>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<IndexMap<K, V, S> as MapTarget<K, V>>::NAME)
    }
}

// the hasher of the adapter is not used
serialize_set! {
    [T, S: Sync, A, AS: Sync] IndexSet<T, S> => IndexSet<A, AS>, Natural;
}

impl<T, S> SetTarget<T> for IndexSet<T, S>
where
    T: Hash + Eq + Send,
    S: BuildHasher + Default + Send,
{
    const NAME: &'static str = "IndexSet";

    #[inline]
    fn insert_element(&mut self, value: T) {
        self.insert(value);
    }

    #[inline]
    fn reserve_elements(&mut self, additional: usize) {
        self.reserve(additional);
    }
}

// the hasher of the adapter is not used
impl<'de, T, S, A, AS> Deserialize<'de, IndexSet<T, S>> for IndexSet<A, AS>
where
    T: Hash + Eq + Send,
    S: BuildHasher + Default + Send,
    A: Deserialize<'de, T>,
    AS: Send,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<IndexSet<T, S>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, A>(out, state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed(<IndexSet<T, S> as SetTarget<T>>::NAME)
    }

    collection_methods!(set IndexSet<T, S>);
}

set_collection! {
    [T: Hash + Eq + Send, S: BuildHasher + Default + Send] IndexSet<T, S>;
}
