use std::any::{type_name, Any, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::hash::{Hash, Hasher};

pub struct TypeKey(TypeId, &'static str);

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
    map: RefCell<HashMap<TypeKey, Box<dyn DebugAny>>>,
}

impl Extensions {
    pub fn insert<T: Debug + 'static>(&self, value: T) {
        self.map
            .borrow_mut()
            .insert(TypeKey::of::<T>(), Box::new(value));
    }

    pub fn get<T: Default + Debug + 'static>(&self) -> Ref<'_, T> {
        self.ensure::<T>();
        Ref::map(self.map.borrow(), |m| {
            m.get(&TypeKey::of::<T>())
                .and_then(|b| (**b).as_any().downcast_ref())
                .unwrap()
        })
    }

    pub fn get_mut<T: Default + Debug + 'static>(&self) -> RefMut<'_, T> {
        self.ensure::<T>();
        RefMut::map(self.map.borrow_mut(), |m| {
            m.get_mut(&TypeKey::of::<T>())
                .and_then(|b| (**b).as_any_mut().downcast_mut())
                .unwrap()
        })
    }

    fn ensure<T: Default + Debug + 'static>(&self) {
        if self.map.borrow().get(&TypeKey::of::<T>()).is_none() {
            self.insert(T::default());
        }
    }
}
