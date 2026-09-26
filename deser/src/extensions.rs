use std::any::{Any, TypeId, type_name};
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

trait DebugAny: Any + Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: Any + Debug + Send + Sync + 'static> DebugAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// The values of event data are also `Sync` so that captured event data
/// can be shared between threads (see [`EventData`]).
trait EventAny: DebugAny + Sync {}

impl<T: DebugAny + Sync> EventAny for T {}

/// Functions to clone values of a type behind a `dyn DebugAny`.
#[derive(Copy, Clone)]
struct CloneFns {
    clone: fn(&dyn DebugAny) -> Box<dyn DebugAny>,
    clone_into: fn(&mut dyn DebugAny, &dyn DebugAny),
}

impl CloneFns {
    fn of<T: Clone + Debug + Send + Sync + 'static>() -> CloneFns {
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

/// Functions to clone event data behind a `dyn EventAny`.
#[derive(Copy, Clone)]
struct EventFns {
    clone: fn(&dyn EventAny) -> Box<dyn EventAny>,
    clone_into: fn(&mut dyn EventAny, &dyn EventAny),
}

impl EventFns {
    fn of<T: Clone + Debug + Send + Sync + 'static>() -> EventFns {
        EventFns {
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
    value: Box<dyn EventAny>,
    fns: EventFns,
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
    pub fn get<T: Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        let index = self.position(TypeId::of::<T>())?;
        let value: &dyn DebugAny = &*self.entries[index].1;
        // SAFETY: values are always stored with the key of their type
        Some(unsafe { &*(value as *const dyn DebugAny).cast::<T>() })
    }

    #[inline]
    pub fn get_mut<T: Default + Debug + Send + Sync + 'static>(&mut self) -> &mut T {
        let index = match self.position(TypeId::of::<T>()) {
            Some(index) => index,
            None => self.insert_default::<T>(),
        };
        let value: &mut dyn DebugAny = &mut *self.entries[index].1;
        // SAFETY: values are always stored with the key of their type
        unsafe { &mut *(value as *mut dyn DebugAny).cast::<T>() }
    }

    #[cold]
    fn insert_default<T: Default + Debug + Send + Sync + 'static>(&mut self) -> usize {
        self.entries
            .push((TypeKey::of::<T>(), Box::new(T::default())));
        self.entries.len() - 1
    }

    /// Marks an extension type as replayable.
    pub fn set_replayable<T: Clone + Debug + Send + Sync + 'static>(&mut self) {
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
    pub fn event<T: Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        if !self.has_event_data {
            return None;
        }
        let entry = &self.events[self.event_position(TypeId::of::<T>())?];
        if !entry.active {
            return None;
        }
        // SAFETY: values are always stored with the key of their type
        Some(unsafe { &*(&*entry.value as *const dyn EventAny).cast::<T>() })
    }

    /// Returns the data of a type attached to the current event mutably.
    ///
    /// If no such data is attached yet, the default value is attached.
    #[inline]
    pub fn event_mut<T: Default + Clone + Debug + Send + Sync + 'static>(&mut self) -> &mut T {
        let index = match self.event_position(TypeId::of::<T>()) {
            Some(index) => index,
            None => self.insert_event::<T>(),
        };
        let entry = &mut self.events[index];
        // SAFETY: values are always stored with the key of their type
        let value = unsafe { &mut *(&mut *entry.value as *mut dyn EventAny).cast::<T>() };
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
    fn insert_event<T: Default + Clone + Debug + Send + Sync + 'static>(&mut self) -> usize {
        self.events.push(EventEntry {
            key: TypeKey::of::<T>(),
            active: false,
            value: Box::new(T::default()),
            fns: EventFns::of::<T>(),
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
        snapshot.events = self.capture_event_data();
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
        self.restore_event_data(&snapshot.events);
    }

    /// Captures the data attached to the current event.
    pub fn capture_event_data(&self) -> EventData {
        if !self.has_event_data {
            return EventData::default();
        }
        EventData {
            entries: self
                .events
                .iter()
                .filter(|entry| entry.active)
                .map(|entry| EventDataEntry {
                    key: entry.key,
                    value: (entry.fns.clone)(&*entry.value),
                    fns: entry.fns,
                })
                .collect(),
        }
    }

    /// Replaces the data attached to the current event.
    pub fn restore_event_data(&mut self, data: &EventData) {
        self.clear_event_data();
        self.attach_event_data(data);
    }

    /// Attaches data to the current event.
    ///
    /// Data of the same types that is already attached is replaced, other
    /// data is retained.
    pub fn attach_event_data(&mut self, data: &EventData) {
        for entry in data.entries.iter() {
            match self.event_position(entry.key.0) {
                Some(index) => {
                    let target = &mut self.events[index];
                    (entry.fns.clone_into)(&mut *target.value, &*entry.value);
                    target.active = true;
                }
                None => self.events.push(EventEntry {
                    key: entry.key,
                    active: true,
                    value: (entry.fns.clone)(&*entry.value),
                    fns: entry.fns,
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
    events: EventData,
}

impl Snapshot {
    /// Returns the captured event data.
    pub fn event_data(&self) -> &EventData {
        &self.events
    }
}

impl Clone for Snapshot {
    fn clone(&self) -> Snapshot {
        Snapshot {
            replayable: self
                .replayable
                .iter()
                .map(|(key, value, fns)| (*key, (fns.clone)(&**value), *fns))
                .collect(),
            events: self.events.clone(),
        }
    }
}

impl Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.replayable.iter().map(|(key, value, _)| (key, value)))
            .entries(
                self.events
                    .entries
                    .iter()
                    .map(|entry| (entry.key, &entry.value)),
            )
            .finish()
    }
}

/// Data attached to an event, detached from the event.
///
/// Formats and values attach data to individual events (see
/// [`State::event`](crate::State::event)), for instance tags or formatting
/// hints.  This captures such data so that it can be attached to an event
/// again later.  This is used by types which hold on to values outside of a
/// serialization or deserialization, such as the values of the
/// `deser-value` crate, so that data like CBOR tags survives.
///
/// ```
/// use deser::de::DeserializeDriver;
/// use deser::EventData;
///
/// #[derive(Debug, Default, Clone, PartialEq)]
/// struct Tag(u64);
///
/// let mut data = EventData::new();
/// data.insert(Tag(42));
/// assert_eq!(data.get::<Tag>(), Some(&Tag(42)));
///
/// let mut out = None::<bool>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// driver.state_mut().attach_event_data(&data);
/// assert_eq!(driver.state().event::<Tag>(), Some(&Tag(42)));
/// assert_eq!(driver.state().capture_event_data().get::<Tag>(), Some(&Tag(42)));
/// ```
///
/// Event data has to be [`Send`] and [`Sync`], which means that captured
/// event data is too.
#[derive(Default)]
pub struct EventData {
    // Invariant: the value of an entry is always of the type of its key and
    // there is at most one entry per key.
    entries: Vec<EventDataEntry>,
}

struct EventDataEntry {
    key: TypeKey,
    value: Box<dyn EventAny>,
    fns: EventFns,
}

impl EventData {
    /// Creates empty event data.
    pub const fn new() -> EventData {
        EventData {
            entries: Vec::new(),
        }
    }

    /// Returns `true` if no data is held.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn position(&self, key: TypeId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.key.0 == key)
    }

    /// Returns the data of a type.
    pub fn get<T: Debug + Send + 'static>(&self) -> Option<&T> {
        let entry = &self.entries[self.position(TypeId::of::<T>())?];
        // SAFETY: values are always stored with the key of their type
        Some(unsafe { &*(&*entry.value as *const dyn EventAny).cast::<T>() })
    }

    /// Returns the data of a type mutably.
    ///
    /// If there is no data of this type, the default value is inserted.
    pub fn get_mut<T: Default + Clone + Debug + Send + Sync + 'static>(&mut self) -> &mut T {
        let index = match self.position(TypeId::of::<T>()) {
            Some(index) => index,
            None => {
                self.entries.push(EventDataEntry {
                    key: TypeKey::of::<T>(),
                    value: Box::new(T::default()),
                    fns: EventFns::of::<T>(),
                });
                self.entries.len() - 1
            }
        };
        let entry = &mut self.entries[index];
        // SAFETY: values are always stored with the key of their type
        unsafe { &mut *(&mut *entry.value as *mut dyn EventAny).cast::<T>() }
    }

    /// Inserts data, replacing data of the same type.
    pub fn insert<T: Clone + Debug + Send + Sync + 'static>(&mut self, value: T) {
        let entry = EventDataEntry {
            key: TypeKey::of::<T>(),
            value: Box::new(value),
            fns: EventFns::of::<T>(),
        };
        match self.position(TypeId::of::<T>()) {
            Some(index) => self.entries[index] = entry,
            None => self.entries.push(entry),
        }
    }

    /// Removes the data of a type and returns it.
    pub fn remove<T: Debug + Send + 'static>(&mut self) -> Option<T> {
        let entry = self.entries.remove(self.position(TypeId::of::<T>())?);
        entry
            .value
            .into_any()
            .downcast::<T>()
            .ok()
            .map(|value| *value)
    }
}

impl Clone for EventData {
    fn clone(&self) -> EventData {
        EventData {
            entries: self
                .entries
                .iter()
                .map(|entry| EventDataEntry {
                    key: entry.key,
                    value: (entry.fns.clone)(&*entry.value),
                    fns: entry.fns,
                })
                .collect(),
        }
    }
}

impl Debug for EventData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|entry| (entry.key, &entry.value)))
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
