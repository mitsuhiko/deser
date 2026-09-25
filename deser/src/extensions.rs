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

trait DebugAny: Any + Debug + Send {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Debug + Send + 'static> DebugAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Functions to clone values of a type behind a `dyn DebugAny`.
#[derive(Copy, Clone)]
struct CloneFns {
    clone: fn(&dyn DebugAny) -> Box<dyn DebugAny>,
    clone_into: fn(&mut dyn DebugAny, &dyn DebugAny),
}

impl CloneFns {
    fn of<T: Clone + Debug + Send + 'static>() -> CloneFns {
        CloneFns {
            clone: |value| Box::new(value.as_any().downcast_ref::<T>().unwrap().clone()),
            clone_into: |target, value| {
                target
                    .as_any_mut()
                    .downcast_mut::<T>()
                    .unwrap()
                    .clone_from(value.as_any().downcast_ref::<T>().unwrap())
            },
        }
    }
}

/// A value attached to the current event.
struct EventEntry {
    key: TypeKey,
    // values are retained when they are deactivated so that their memory
    // can be reused for the next event.
    active: bool,
    value: Box<dyn DebugAny>,
    fns: CloneFns,
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
    replayable: Vec<(TypeKey, CloneFns)>,
    // Invariant: the value of an entry is always of the type of its key.
    events: Vec<EventEntry>,
    // `true` if any of the event entries is active
    has_event_data: bool,
}

impl Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(key, value)| (key, value)))
            .entries(
                self.events
                    .iter()
                    .filter(|entry| entry.active)
                    .map(|entry| (entry.key, &entry.value)),
            )
            .finish()
    }
}

impl Extensions {
    #[inline]
    fn position(&self, key: TypeId) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k.0 == key)
    }

    #[inline]
    pub fn get<T: Debug + Send + 'static>(&self) -> Option<&T> {
        let index = self.position(TypeId::of::<T>())?;
        let value: &dyn DebugAny = &*self.entries[index].1;
        // SAFETY: values are always stored with the key of their type
        Some(unsafe { &*(value as *const dyn DebugAny).cast::<T>() })
    }

    #[inline]
    pub fn get_mut<T: Default + Debug + Send + 'static>(&mut self) -> &mut T {
        let index = match self.position(TypeId::of::<T>()) {
            Some(index) => index,
            None => self.insert_default::<T>(),
        };
        let value: &mut dyn DebugAny = &mut *self.entries[index].1;
        // SAFETY: values are always stored with the key of their type
        unsafe { &mut *(value as *mut dyn DebugAny).cast::<T>() }
    }

    #[cold]
    fn insert_default<T: Default + Debug + Send + 'static>(&mut self) -> usize {
        self.entries
            .push((TypeKey::of::<T>(), Box::new(T::default())));
        self.entries.len() - 1
    }

    /// Marks an extension type as replayable.
    pub fn set_replayable<T: Clone + Debug + Send + 'static>(&mut self) {
        let key = TypeKey::of::<T>();
        if !self.replayable.iter().any(|(k, _)| *k == key) {
            self.replayable.push((key, CloneFns::of::<T>()));
        }
    }

    /// Returns `true` if data is attached to the current event.
    #[inline(always)]
    pub fn has_event_data(&self) -> bool {
        self.has_event_data
    }

    #[inline]
    fn event_position(&self, key: TypeId) -> Option<usize> {
        self.events.iter().position(|entry| entry.key.0 == key)
    }

    /// Returns the data of a type attached to the current event.
    #[inline]
    pub fn event<T: Debug + Send + 'static>(&self) -> Option<&T> {
        if !self.has_event_data {
            return None;
        }
        let entry = &self.events[self.event_position(TypeId::of::<T>())?];
        if !entry.active {
            return None;
        }
        // SAFETY: values are always stored with the key of their type
        Some(unsafe { &*(&*entry.value as *const dyn DebugAny).cast::<T>() })
    }

    /// Returns the data of a type attached to the current event mutably.
    ///
    /// If no such data is attached yet, the default value is attached.
    #[inline]
    pub fn event_mut<T: Default + Clone + Debug + Send + 'static>(&mut self) -> &mut T {
        let index = match self.event_position(TypeId::of::<T>()) {
            Some(index) => index,
            None => self.insert_event::<T>(),
        };
        let entry = &mut self.events[index];
        // SAFETY: values are always stored with the key of their type
        let value = unsafe { &mut *(&mut *entry.value as *mut dyn DebugAny).cast::<T>() };
        if !entry.active {
            // the value of a previous event is reset in place which retains
            // the memory of collections
            value.clone_from(&T::default());
            entry.active = true;
            self.has_event_data = true;
        }
        value
    }

    #[cold]
    fn insert_event<T: Default + Clone + Debug + Send + 'static>(&mut self) -> usize {
        self.events.push(EventEntry {
            key: TypeKey::of::<T>(),
            active: false,
            value: Box::new(T::default()),
            fns: CloneFns::of::<T>(),
        });
        self.events.len() - 1
    }

    /// Detaches all data from the current event.
    #[inline(always)]
    pub fn clear_event_data(&mut self) {
        if self.has_event_data {
            self.deactivate_events();
        }
    }

    #[inline(never)]
    fn deactivate_events(&mut self) {
        for entry in self.events.iter_mut() {
            entry.active = false;
        }
        self.has_event_data = false;
    }

    /// Captures the values of the replayable extensions and the event data.
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = Snapshot::default();
        if !self.replayable.is_empty() {
            snapshot.replayable = self
                .replayable
                .iter()
                .filter_map(|&(key, fns)| {
                    self.position(key.0)
                        .map(|index| (key, (fns.clone)(&*self.entries[index].1), fns))
                })
                .collect();
        }
        if self.has_event_data {
            snapshot.events = self
                .events
                .iter()
                .filter(|entry| entry.active)
                .map(|entry| (entry.key, (entry.fns.clone)(&*entry.value), entry.fns))
                .collect();
        }
        snapshot
    }

    /// Restores the values from a snapshot.
    ///
    /// The event data is replaced by the event data of the snapshot.
    pub fn restore(&mut self, snapshot: &Snapshot) {
        // the clone functions belong to the type of the key
        for (key, value, fns) in snapshot.replayable.iter() {
            match self.position(key.0) {
                Some(index) => (fns.clone_into)(&mut *self.entries[index].1, &**value),
                None => self.entries.push((*key, (fns.clone)(&**value))),
            }
        }
        self.clear_event_data();
        for (key, value, fns) in snapshot.events.iter() {
            match self.event_position(key.0) {
                Some(index) => {
                    let entry = &mut self.events[index];
                    (fns.clone_into)(&mut *entry.value, &**value);
                    entry.active = true;
                }
                None => self.events.push(EventEntry {
                    key: *key,
                    active: true,
                    value: (fns.clone)(&**value),
                    fns: *fns,
                }),
            }
            self.has_event_data = true;
        }
    }
}

/// The captured values of the replayable extensions and the event data.
#[derive(Default)]
pub struct Snapshot {
    replayable: Vec<(TypeKey, Box<dyn DebugAny>, CloneFns)>,
    events: Vec<(TypeKey, Box<dyn DebugAny>, CloneFns)>,
}

impl Clone for Snapshot {
    fn clone(&self) -> Snapshot {
        fn clone_values(
            values: &[(TypeKey, Box<dyn DebugAny>, CloneFns)],
        ) -> Vec<(TypeKey, Box<dyn DebugAny>, CloneFns)> {
            values
                .iter()
                .map(|(key, value, fns)| (*key, (fns.clone)(&**value), *fns))
                .collect()
        }
        Snapshot {
            replayable: clone_values(&self.replayable),
            events: clone_values(&self.events),
        }
    }
}

impl Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(
                self.replayable
                    .iter()
                    .chain(self.events.iter())
                    .map(|(key, value, _)| (key, value)),
            )
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

#[test]
fn test_event_data() {
    #[derive(Debug, Default, PartialEq)]
    struct Tags(Vec<u64>);

    // derived clones do not forward `clone_from`
    impl Clone for Tags {
        fn clone(&self) -> Tags {
            Tags(self.0.clone())
        }

        fn clone_from(&mut self, source: &Tags) {
            self.0.clone_from(&source.0);
        }
    }
    #[derive(Debug, Default, Clone, PartialEq)]
    struct Span(usize, usize);

    let mut ext = Extensions::default();
    assert!(!ext.has_event_data());
    assert_eq!(ext.event::<Tags>(), None);

    ext.event_mut::<Tags>().0.extend([1, 2]);
    assert!(ext.has_event_data());
    assert_eq!(ext.event::<Tags>(), Some(&Tags(vec![1, 2])));
    assert_eq!(ext.event::<Span>(), None);

    // clearing detaches the data but retains the memory
    ext.clear_event_data();
    assert!(!ext.has_event_data());
    assert_eq!(ext.event::<Tags>(), None);
    let tags = ext.event_mut::<Tags>();
    assert_eq!(tags, &Tags(vec![]));
    assert!(tags.0.capacity() >= 2);
    tags.0.push(3);
    *ext.event_mut::<Span>() = Span(1, 2);

    // snapshots capture only the attached data
    let snapshot = ext.snapshot();
    ext.clear_event_data();
    *ext.event_mut::<Span>() = Span(3, 4);
    ext.restore(&snapshot);
    assert_eq!(ext.event::<Tags>(), Some(&Tags(vec![3])));
    assert_eq!(ext.event::<Span>(), Some(&Span(1, 2)));

    ext.clear_event_data();
    ext.event_mut::<Span>();
    let empty = ext.snapshot();
    *ext.event_mut::<Tags>() = Tags(vec![4]);
    ext.restore(&empty);
    assert_eq!(ext.event::<Tags>(), None);
    assert_eq!(ext.event::<Span>(), Some(&Span(0, 0)));
}
