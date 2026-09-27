//! `HashMap` and `HashSet` of `hashbrown`.
//!
//! They are serialized like the maps and sets of the standard library.
use core::hash::{BuildHasher, Hash};

use ::hashbrown::{HashMap, HashSet, hash_map};

use crate::State;
use crate::adapters::ser_impls::{serialize_as_map, serialize_as_set};
use crate::adapters::{DeserializeAs, MapSkipError, Same, SerializeAs, skip_map_sink};
use crate::de::impls::{MapOut, MapTarget, SetTarget, map_sink, set_sink};
use crate::de::{Deserialize, SinkHandle};
use crate::error::Error;
use crate::event::ContainerShape;
use crate::ser::impls::{serialize_map, serialize_set};
use crate::ser::{Chunk, Describe};

serialize_map! {
    [K, V, S: Sync] HashMap<K, V, S> => Arbitrary;
}

serialize_as_map! {
    [K, V, S: Sync, KA, VA] HashMap<K, V, S> => HashMap<KA, VA>, Arbitrary;
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

impl<'de, K, V, S> Deserialize<'de> for HashMap<K, V, S>
where
    K: Hash + Eq + Deserialize<'de>,
    V: Deserialize<'de>,
    S: BuildHasher + Default + Send,
{
    #[inline]
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Slot(out))
    }

    /// Merges the entries into the map, the values of keys that exist are
    /// replaced (not updated).
    fn deserialize_update(value: &mut Self) -> SinkHandle<'_, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Update(value))
    }
}

impl<'de, K, V, S, KA, VA> DeserializeAs<'de, HashMap<K, V, S>> for HashMap<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<HashMap<K, V, S>>) -> SinkHandle<'_, 'de> {
        map_sink::<_, K, V, KA, VA>(MapOut::Slot(out))
    }
}

impl<'de, K, V, S, KA, VA> DeserializeAs<'de, HashMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<HashMap<K, V, S>>) -> SinkHandle<'_, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out)
    }
}

impl<K, V, S, KA, VA> SerializeAs<HashMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Sync,
    V: Sync,
    S: Sync,
    KA: SerializeAs<K>,
    VA: SerializeAs<V>,
{
    fn serialize_as<'a>(
        value: &'a HashMap<K, V, S>,
        state: &mut State,
    ) -> Result<Chunk<'a>, Error> {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, S>>>::serialize_as(value, state)
    }

    fn container_shape_as(value: &HashMap<K, V, S>) -> ContainerShape {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, S>>>::container_shape_as(value)
    }

    fn describe_as(value: &HashMap<K, V, S>, d: &mut dyn Describe) {
        <HashMap<KA, VA> as SerializeAs<HashMap<K, V, S>>>::describe_as(value, d)
    }
}

serialize_set! {
    [T, S: Sync] HashSet<T, S> => Arbitrary;
}

serialize_as_set! {
    [T, S: Sync, A] HashSet<T, S> => HashSet<A>, Arbitrary;
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

impl<'de, T, S> Deserialize<'de> for HashSet<T, S>
where
    T: Hash + Eq + Deserialize<'de>,
    S: BuildHasher + Default + Send,
{
    #[inline]
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        set_sink::<_, T, Same>(out)
    }
}

impl<'de, T, S, A> DeserializeAs<'de, HashSet<T, S>> for HashSet<A>
where
    T: Hash + Eq + Send,
    S: BuildHasher + Default + Send,
    A: DeserializeAs<'de, T>,
{
    fn deserialize_into_as(out: &mut Option<HashSet<T, S>>) -> SinkHandle<'_, 'de> {
        set_sink::<_, T, A>(out)
    }
}
