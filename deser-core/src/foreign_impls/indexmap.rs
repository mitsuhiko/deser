//! `IndexMap` and `IndexSet` of `indexmap`.
//!
//! They are serialized like `HashMap` and `HashSet` except that the entries
//! are emitted in their order (which is the natural order of maps and
//! sequences).  When deserialized, the entries keep the order of the data.
//!
//! The adapters (`IndexMap<KA, VA>` and `IndexSet<A>`) need the default
//! hasher of `indexmap`, which requires `std`.
use core::hash::{BuildHasher, Hash};

use ::indexmap::{IndexMap, IndexSet, map};

use crate::adapters::{DeserializeAs, MapSkipError, Same, skip_map_sink};
use crate::de::impls::{
    MapOut, MapTarget, SetTarget, collection_methods, map_sink, set_collection, set_sink,
};
use crate::de::{Deserialize, SinkHandle};
use crate::ser::impls::{serialize_map, serialize_set};

serialize_map! {
    [K, V, S: Sync] IndexMap<K, V, S> => Natural;
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

impl<'de, K, V, S> Deserialize<'de> for IndexMap<K, V, S>
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
    /// replaced (not updated).  New keys are appended.
    fn deserialize_update(value: &mut Self) -> SinkHandle<'_, 'de> {
        map_sink::<_, K, V, Same, Same>(MapOut::Update(value))
    }
}

impl<'de, K, V, S, KA, VA> DeserializeAs<'de, IndexMap<K, V, S>> for MapSkipError<KA, VA>
where
    K: Hash + Eq + Send,
    V: Send,
    S: BuildHasher + Default + Send,
    KA: DeserializeAs<'de, K>,
    VA: DeserializeAs<'de, V>,
{
    fn deserialize_into_as(out: &mut Option<IndexMap<K, V, S>>) -> SinkHandle<'_, 'de> {
        skip_map_sink::<_, K, V, KA, VA>(out)
    }
}

serialize_set! {
    [T, S: Sync] IndexSet<T, S> => Natural;
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

impl<'de, T, S> Deserialize<'de> for IndexSet<T, S>
where
    T: Hash + Eq + Deserialize<'de>,
    S: BuildHasher + Default + Send,
{
    #[inline]
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        set_sink::<_, T, Same>(out)
    }

    collection_methods!(set Same);
}

set_collection! {
    [T: Hash + Eq + Send, S: BuildHasher + Default + Send] IndexSet<T, S>;
}

/// The adapters, they need the default hasher.
#[cfg(feature = "std")]
mod adapters {
    use core::hash::{BuildHasher, Hash};

    use ::indexmap::{IndexMap, IndexSet};

    use crate::State;
    use crate::adapters::ser_impls::{serialize_as_map, serialize_as_set};
    use crate::adapters::{DeserializeAs, MapSkipError, SerializeAs};
    use crate::de::SinkHandle;
    use crate::de::impls::{MapOut, collection_methods_as, map_sink, set_sink};
    use crate::error::Error;
    use crate::event::ContainerShape;
    use crate::ser::{Chunk, Describe};

    serialize_as_map! {
        [K, V, S: Sync, KA, VA] IndexMap<K, V, S> => IndexMap<KA, VA>, Natural;
    }

    impl<'de, K, V, S, KA, VA> DeserializeAs<'de, IndexMap<K, V, S>> for IndexMap<KA, VA>
    where
        K: Hash + Eq + Send,
        V: Send,
        S: BuildHasher + Default + Send,
        KA: DeserializeAs<'de, K>,
        VA: DeserializeAs<'de, V>,
    {
        fn deserialize_into_as(out: &mut Option<IndexMap<K, V, S>>) -> SinkHandle<'_, 'de> {
            map_sink::<_, K, V, KA, VA>(MapOut::Slot(out))
        }
    }

    impl<K, V, S, KA, VA> SerializeAs<IndexMap<K, V, S>> for MapSkipError<KA, VA>
    where
        K: Sync,
        V: Sync,
        S: Sync,
        KA: SerializeAs<K>,
        VA: SerializeAs<V>,
    {
        fn serialize_as<'a>(
            value: &'a IndexMap<K, V, S>,
            state: &mut State,
        ) -> Result<Chunk<'a>, Error> {
            <IndexMap<KA, VA> as SerializeAs<IndexMap<K, V, S>>>::serialize_as(value, state)
        }

        fn container_shape_as(value: &IndexMap<K, V, S>) -> ContainerShape {
            <IndexMap<KA, VA> as SerializeAs<IndexMap<K, V, S>>>::container_shape_as(value)
        }

        fn describe_as(value: &IndexMap<K, V, S>, d: &mut dyn Describe) {
            <IndexMap<KA, VA> as SerializeAs<IndexMap<K, V, S>>>::describe_as(value, d)
        }
    }

    serialize_as_set! {
        [T, S: Sync, A] IndexSet<T, S> => IndexSet<A>, Natural;
    }

    impl<'de, T, S, A> DeserializeAs<'de, IndexSet<T, S>> for IndexSet<A>
    where
        T: Hash + Eq + Send,
        S: BuildHasher + Default + Send,
        A: DeserializeAs<'de, T>,
    {
        fn deserialize_into_as(out: &mut Option<IndexSet<T, S>>) -> SinkHandle<'_, 'de> {
            set_sink::<_, T, A>(out)
        }

        collection_methods_as!(set IndexSet<T, S>);
    }
}
