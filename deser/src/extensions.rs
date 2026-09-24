use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::hash::{BuildHasherDefault, Hash, Hasher};

#[derive(Copy, Clone)]
pub struct TypeKey(TypeId, &'static str);

/// A hasher for type ids.
///
/// Type ids are already hashes, so there is no need to hash them again.
#[derive(Default)]
struct TypeIdHasher(u64);

impl Hasher for TypeIdHasher {
    fn write(&mut self, bytes: &[u8]) {
        // type ids only write integers, but be defensive
        for &byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(byte);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 ^= value;
    }

    fn write_u128(&mut self, value: u128) {
        self.0 ^= value as u64 ^ (value >> 64) as u64;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl TypeKey {
    pub fn of<T: 'static>() -> TypeKey {
        TypeKey(TypeId::of::<T>(), type_name::<T>())
    }
}

impl Hash for TypeKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for TypeKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for TypeKey {}

impl Debug for TypeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.1)
    }
}

trait DebugAny: Any + Debug {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Debug + 'static> DebugAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

type CloneFn = fn(&dyn DebugAny) -> Box<dyn DebugAny>;

fn clone_value<T: Clone + Debug + 'static>(value: &dyn DebugAny) -> Box<dyn DebugAny> {
    Box::new(value.as_any().downcast_ref::<T>().unwrap().clone())
}

#[derive(Default, Debug)]
pub struct Extensions {
    map: HashMap<TypeKey, Box<dyn DebugAny>, BuildHasherDefault<TypeIdHasher>>,
    replayable: Vec<(TypeKey, CloneFn)>,
}

impl Extensions {
    #[inline]
    pub fn get<T: Debug + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeKey::of::<T>())
            .and_then(|b| (**b).as_any().downcast_ref())
    }

    #[inline]
    pub fn get_mut<T: Default + Debug + 'static>(&mut self) -> &mut T {
        self.map
            .entry(TypeKey::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .as_mut()
            .as_any_mut()
            .downcast_mut()
            .unwrap()
    }

    /// Marks an extension type as replayable.
    pub fn set_replayable<T: Clone + Debug + 'static>(&mut self) {
        let key = TypeKey::of::<T>();
        if !self.replayable.iter().any(|(k, _)| *k == key) {
            self.replayable.push((key, clone_value::<T>));
        }
    }

    /// Captures the current values of all replayable extensions.
    pub fn snapshot(&self) -> Snapshot {
        if self.replayable.is_empty() {
            return Snapshot(Vec::new());
        }
        Snapshot(
            self.replayable
                .iter()
                .filter_map(|&(key, clone)| {
                    self.map
                        .get(&key)
                        .map(|value| (key, clone(&**value), clone))
                })
                .collect(),
        )
    }

    /// Restores the values from a snapshot.
    pub fn restore(&mut self, snapshot: &Snapshot) {
        for (key, value, clone) in snapshot.0.iter() {
            self.map.insert(*key, clone(&**value));
        }
    }
}

/// The captured values of the replayable extensions.
#[derive(Default)]
pub struct Snapshot(Vec<(TypeKey, Box<dyn DebugAny>, CloneFn)>);

impl Clone for Snapshot {
    fn clone(&self) -> Snapshot {
        Snapshot(
            self.0
                .iter()
                .map(|(key, value, clone)| (*key, clone(&**value), *clone))
                .collect(),
        )
    }
}

impl Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.0.iter().map(|(key, value, _)| (key, value)))
            .finish()
    }
}
