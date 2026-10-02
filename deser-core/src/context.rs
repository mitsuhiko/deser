use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::TypeId;
use core::fmt::{self, Debug};

use crate::extensions::{DebugAny, TypeKey};

/// A value of a context with the key of its type.
type Entry = (TypeKey, Arc<dyn DebugAny>);

/// Configuration for serializations and deserializations.
///
/// A context holds typed values which are given to a serialization or
/// deserialization from the outside: policies like how unknown fields are
/// handled ([`UnknownFields`](crate::de::UnknownFields)), how bytes are
/// decoded from strings ([`BytesFormat`](crate::BytesFormat)) or data
/// that types need, such as the variants of open enums.  It's created once
/// and passed to every serialization or deserialization that uses it, for
/// instance with
/// [`Deserializer::deserialize_in`](crate::de::Deserializer::deserialize_in):
///
/// ```
/// use deser::de::{DeserializeDriver, DuplicateKeys};
/// use deser::{Context, Event};
/// use std::collections::BTreeMap;
///
/// let context = Context::new().with(DuplicateKeys::Last);
///
/// let mut out = None::<BTreeMap<String, u32>>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// driver.set_context(&context);
/// for event in [
///     Event::map_start(),
///     "a".into(),
///     1u64.into(),
///     "a".into(),
///     2u64.into(),
///     Event::MapEnd,
/// ] {
///     driver.emit(event).unwrap();
/// }
/// drop(driver);
/// assert_eq!(out.unwrap()["a"], 2);
/// ```
///
/// The values of the context are the defaults of the extension values of
/// the [`State`](crate::State): [`State::get`](crate::State::get) returns
/// the value of the state if there is one and the value of the context
/// otherwise.  The values that the state holds change during a
/// serialization or deserialization (for instance a format or a type sets
/// a value for a part of the data), the context does not change.
///
/// Cloning a context is cheap, the values are shared.  Values are
/// [`Debug`], [`Send`] and [`Sync`] like the extension values of the state
/// so that contexts can be shared between threads.
#[derive(Clone, Default)]
pub struct Context {
    // `None` for the empty context so that it does not allocate.
    // Invariant: the value of an entry is always of the type of its key.
    values: Option<Arc<Vec<Entry>>>,
}

impl Context {
    /// Creates an empty context.
    pub const fn new() -> Context {
        Context { values: None }
    }

    /// Returns the context with a value, replacing a value of the same type.
    pub fn with<T: Debug + Send + Sync + 'static>(mut self, value: T) -> Context {
        self.insert(value);
        self
    }

    /// Inserts a value, replacing a value of the same type.
    ///
    /// If the values are shared with clones of the context, they are copied
    /// (the values themselves are shared).
    pub fn insert<T: Debug + Send + Sync + 'static>(&mut self, value: T) {
        let values = Arc::make_mut(self.values.get_or_insert_with(Default::default));
        let value: Arc<dyn DebugAny> = Arc::new(value);
        match values
            .iter_mut()
            .find(|(key, _)| key.0 == TypeId::of::<T>())
        {
            Some(entry) => entry.1 = value,
            None => values.push((TypeKey::of::<T>(), value)),
        }
    }

    /// Returns the value of a type.
    #[inline]
    pub fn get<T: Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        let values = self.values.as_deref()?;
        self.lookup(values, TypeId::of::<T>()).map(|value| {
            // SAFETY: values are always stored with the key of their type
            unsafe { &*(value as *const dyn DebugAny).cast::<T>() }
        })
    }

    #[inline]
    fn lookup<'a>(&self, values: &'a [Entry], key: TypeId) -> Option<&'a dyn DebugAny> {
        values
            .iter()
            .find(|(k, _)| k.0 == key)
            .map(|(_, value)| &**value)
    }

    /// Returns `true` if the context holds no values.
    pub fn is_empty(&self) -> bool {
        self.values.as_ref().is_none_or(|values| values.is_empty())
    }
}

impl Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        if let Some(ref values) = self.values {
            map.entries(values.iter().map(|(key, value)| (key, value)));
        }
        map.finish()
    }
}

#[test]
fn test_context() {
    let empty = Context::new();
    assert!(empty.is_empty());
    assert_eq!(empty.get::<u32>(), None);

    let context = Context::new().with(1u32).with("x");
    assert_eq!(context.get::<u32>(), Some(&1));
    assert_eq!(context.get::<&str>(), Some(&"x"));
    assert_eq!(context.get::<u64>(), None);

    // clones share the values until they are changed
    let mut other = context.clone();
    other.insert(2u32);
    assert_eq!(context.get::<u32>(), Some(&1));
    assert_eq!(other.get::<u32>(), Some(&2));
    assert_eq!(other.get::<&str>(), Some(&"x"));
    assert_eq!(format!("{:?}", other), r#"{u32: 2, &str: "x"}"#);
}
