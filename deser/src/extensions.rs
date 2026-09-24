use std::any::{type_name, Any, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::hash::{BuildHasherDefault, Hash, Hasher};

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

#[derive(Default, Debug)]
pub struct Extensions {
    map: RefCell<HashMap<TypeKey, Box<dyn DebugAny>, BuildHasherDefault<TypeIdHasher>>>,
}

impl Extensions {
    pub fn get<T: Default + Debug + 'static>(&self) -> Ref<'_, T> {
        match Ref::filter_map(self.map.borrow(), |m| {
            m.get(&TypeKey::of::<T>())
                .and_then(|b| (**b).as_any().downcast_ref())
        }) {
            Ok(rv) => rv,
            Err(map) => {
                drop(map);
                self.insert_default::<T>();
                self.get()
            }
        }
    }

    pub fn get_mut<T: Default + Debug + 'static>(&self) -> RefMut<'_, T> {
        RefMut::map(self.map.borrow_mut(), |m| {
            m.entry(TypeKey::of::<T>())
                .or_insert_with(|| Box::new(T::default()))
                .as_mut()
                .as_any_mut()
                .downcast_mut()
                .unwrap()
        })
    }

    #[cold]
    fn insert_default<T: Default + Debug + 'static>(&self) {
        self.map
            .borrow_mut()
            .insert(TypeKey::of::<T>(), Box::new(T::default()));
    }
}
