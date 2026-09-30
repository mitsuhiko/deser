//! `HashMap` and `HashSet` of `hashbrown`.
//!
//! They are serialized like the maps and sets of the standard library.
use core::hash::{BuildHasher, Hash};

use ::hashbrown::{HashMap, HashSet, hash_map};

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
    [K, V, S: Sync, KA, VA, AS: Sync] HashMap<K, V, S> => HashMap<KA, VA, AS>, Arbitrary;
}

impl<K, V, S> MapTarget<K, V> for HashMap<K, V, S>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
{
    const NAME: &'static str = "HashMap";

    #[inline]
    fn insert_entry(&mut self, key: K, value: V, replace: bool) -> bool {
        match self.entry(key) {
            hash_map::Entry::Vacant(entry) => {
                entry.insert(value);
                false
            }
            hash_map::Entry::Occupied(mut entry) => {
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

    fn merge(&mut self, mut other: Self) {
        // the smaller map is moved into the larger one
        if other.len() > self.len() {
            core::mem::swap(self, &mut other);
            for (key, value) in other {
                self.entry(key).or_insert(value);
            }
        } else {
            self.extend(other);
        }
    }
}

// the hasher of the adapter is not used
impl<'de, K, V, S, KA, VA, AS> Deserialize<'de, HashMap<K, V, S>> for HashMap<KA, VA, AS>
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
        out: &'out mut Option<HashMap<K, V, S>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out), state)
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).
    fn deserialize_update<'out>(
        value: &'out mut HashMap<K, V, S>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Update(value), state)
    }
}

impl<'de, K, V, S, KA, VA> Deserialize<'de, HashMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    fn deserialize_into<'out>(
        out: &'out mut Option<HashMap<K, V, S>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out, state)
    }
}

impl<K, V, S, KA, VA> Serialize<HashMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Sync,
    V: Sync,
    S: Sync,
    KA: Serialize<K>,
    VA: Serialize<V>,
{
    fn serialize<'a>(value: &'a HashMap<K, V, S>, state: &mut State) -> Result<Chunk<'a>, Error> {
        <HashMap<KA, VA, S> as Serialize<HashMap<K, V, S>>>::serialize(value, state)
    }

    fn container_shape(value: &HashMap<K, V, S>) -> ContainerShape {
        <HashMap<KA, VA, S> as Serialize<HashMap<K, V, S>>>::container_shape(value)
    }

    fn describe(value: &HashMap<K, V, S>, d: &mut dyn Describe) {
        <HashMap<KA, VA, S> as Serialize<HashMap<K, V, S>>>::describe(value, d)
    }
}

// the hasher of the adapter is not used
serialize_set! {
    [T, S: Sync, A, AS: Sync] HashSet<T, S> => HashSet<A, AS>, Arbitrary;
}

impl<T, S> SetTarget<T> for HashSet<T, S>
where
    T: Hash + Eq + Send,
    S: BuildHasher + Default + Send,
{
    const NAME: &'static str = "HashSet";

    #[inline]
    fn insert_element(&mut self, value: T) {
        self.insert(value);
    }

    #[inline]
    fn reserve_elements(&mut self, additional: usize) {
        self.reserve(additional);
    }
}

set_collection! {
    [T: Hash + Eq + Send, S: BuildHasher + Default + Send] HashSet<T, S>;
}

// the hasher of the adapter is not used
impl<'de, T, S, A, AS> Deserialize<'de, HashSet<T, S>> for HashSet<A, AS>
where
    T: Hash + Eq + Send,
    S: BuildHasher + Default + Send,
    A: Deserialize<'de, T>,
    AS: Send,
{
    #[inline]
    fn deserialize_into<'out>(
        out: &'out mut Option<HashSet<T, S>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        set_sink::<_, T, A>(out, state)
    }

    collection_methods!(set HashSet<T, S>);
}
