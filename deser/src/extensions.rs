use std::any::{type_name, Any, TypeId};
use std::fmt::{self, Debug};

#[derive(Copy, Clone)]
pub struct TypeKey(TypeId, &'static str);

impl TypeKey {
    pub fn of<T: 'static>() -> TypeKey {
        TypeKey(TypeId::of::<T>(), type_name::<T>())
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
}

impl<T: Any + Debug + 'static> DebugAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

type CloneFn = fn(&dyn DebugAny) -> Box<dyn DebugAny>;

fn clone_value<T: Clone + Debug + 'static>(value: &dyn DebugAny) -> Box<dyn DebugAny> {
    Box::new(value.as_any().downcast_ref::<T>().unwrap().clone())
}

/// Typed values stored in a state.
///
/// There are typically only a handful of extension types in a state which
/// is why they are held in a vector and looked up linearly.  This is much
/// faster than a hash map for small numbers of entries.
#[derive(Default)]
pub struct Extensions {
    // Invariant: the value of an entry is always of the type of its key.
    entries: Vec<(TypeKey, Box<dyn DebugAny>)>,
    replayable: Vec<(TypeKey, CloneFn)>,
}

impl Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(key, value)| (key, value)))
            .finish()
    }
}

impl Extensions {
    #[inline]
    fn position(&self, key: TypeId) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k.0 == key)
    }

    #[inline]
    pub fn get<T: Debug + 'static>(&self) -> Option<&T> {
        let index = self.position(TypeId::of::<T>())?;
        let value: &dyn DebugAny = &*self.entries[index].1;
        // SAFETY: values are always stored with the key of their type
        Some(unsafe { &*(value as *const dyn DebugAny).cast::<T>() })
    }

    #[inline]
    pub fn get_mut<T: Default + Debug + 'static>(&mut self) -> &mut T {
        let index = match self.position(TypeId::of::<T>()) {
            Some(index) => index,
            None => self.insert_default::<T>(),
        };
        let value: &mut dyn DebugAny = &mut *self.entries[index].1;
        // SAFETY: values are always stored with the key of their type
        unsafe { &mut *(value as *mut dyn DebugAny).cast::<T>() }
    }

    #[cold]
    fn insert_default<T: Default + Debug + 'static>(&mut self) -> usize {
        self.entries
            .push((TypeKey::of::<T>(), Box::new(T::default())));
        self.entries.len() - 1
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
                    self.position(key.0)
                        .map(|index| (key, clone(&*self.entries[index].1), clone))
                })
                .collect(),
        )
    }

    /// Restores the values from a snapshot.
    pub fn restore(&mut self, snapshot: &Snapshot) {
        for (key, value, clone) in snapshot.0.iter() {
            // the clone function belongs to the type of the key
            let value = clone(&**value);
            match self.position(key.0) {
                Some(index) => self.entries[index].1 = value,
                None => self.entries.push((*key, value)),
            }
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

#[test]
fn test_extensions() {
    #[derive(Debug, Default, Clone, PartialEq)]
    struct A(u32);
    #[derive(Debug, Default, Clone, PartialEq)]
    struct B(String);
    #[derive(Debug, Default, Clone, PartialEq)]
    struct C(u8);

    let mut ext = Extensions::default();
    assert_eq!(ext.get::<A>(), None);
    ext.get_mut::<A>().0 = 42;
    ext.get_mut::<B>().0.push_str("hello");
    ext.get_mut::<C>().0 = 1;
    assert_eq!(ext.get::<A>(), Some(&A(42)));
    assert_eq!(ext.get::<B>(), Some(&B("hello".into())));
    assert_eq!(ext.get::<C>(), Some(&C(1)));

    ext.set_replayable::<A>();
    ext.set_replayable::<B>();
    ext.set_replayable::<A>();
    let snapshot = ext.snapshot();
    ext.get_mut::<A>().0 = 1;
    ext.get_mut::<B>().0.clear();
    ext.get_mut::<C>().0 = 2;
    ext.restore(&snapshot);
    assert_eq!(ext.get::<A>(), Some(&A(42)));
    assert_eq!(ext.get::<B>(), Some(&B("hello".into())));
    assert_eq!(ext.get::<C>(), Some(&C(2)));

    // restoring into empty extensions inserts the values
    let mut other = Extensions::default();
    other.restore(&snapshot.clone());
    assert_eq!(other.get::<A>(), Some(&A(42)));
    assert_eq!(other.get::<C>(), None);
    assert_eq!(
        format!("{:?}", other),
        format!(
            "{{{}: A(42), {}: B(\"hello\")}}",
            std::any::type_name::<A>(),
            std::any::type_name::<B>()
        )
    );
}
