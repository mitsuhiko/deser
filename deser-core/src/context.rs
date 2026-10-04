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
/// decoded from strings ([`BytesFormat`](crate::BytesFormat)), limits for
/// untrusted input ([`Limits`](crate::de::Limits)), whether formats
/// provide source locations ([`TrackLocations`](crate::TrackLocations)) or
/// data that types need, such as the variants of open enums.  It's created
/// once and given to every serialization or deserialization that uses it,
/// usually with the deserializer and serializer configurations of the
/// formats (for instance `deser_json::DeserializerConfig::builder().context(context)`):
///
/// ```
/// use deser::de::DuplicateKeys;
/// use deser::Context;
/// use std::collections::BTreeMap;
///
/// let context = Context::with(DuplicateKeys::Last);
///
/// let config = deser_json::DeserializerConfig::builder()
///     .context(context.clone())
///     .build();
/// let map: BTreeMap<String, u32> = config.from_str(r#"{"a": 1, "a": 2}"#).unwrap();
/// assert_eq!(map["a"], 2);
/// ```
///
/// A single deserialization can be given a context of its own in the setup
/// callback of [`Deserializer::deserialize_with`](crate::de::Deserializer::deserialize_with)
/// (and serializations in the one of `serialize_with`).  Its values take
/// precedence, the values of the format's context are added for the types
/// it has no value for:
///
/// ```
/// use deser::de::{Deserializer, DuplicateKeys, Limits};
/// use deser::Context;
/// use std::collections::BTreeMap;
///
/// let config = deser_json::DeserializerConfig::builder()
///     .context(Context::with(DuplicateKeys::Last))
///     .build();
/// let limits = Context::with(Limits::builder().max_items(2).build());
///
/// let input = r#"{"a": 1, "a": 2, "b": 3}"#;
/// let err = deser_json::Deserializer::from_str_with_config(input, config)
///     .deserialize_with::<BTreeMap<String, u32>, _>(|driver| {
///         driver.set_context(limits.clone())
///     })
///     .unwrap_err();
/// assert_eq!(err.to_string(), "LimitExceeded: too many items at line 1 column 18");
/// ```
///
/// The readers and writers of [`io`](crate::io) have `set_context` methods
/// too.
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
/// so that contexts can be shared between threads.  Values that collect
/// results (like [`UnknownFields::Collect`](crate::de::UnknownFields::Collect))
/// are shared too: everything that uses the context reports to them, so
/// they belong into the state of a single deserialization instead.
///
/// # Values
///
/// These are the values that deser itself reads from a context.  Unless
/// noted otherwise, a value in the [`State`](crate::State) takes
/// precedence over the one of the context.
///
/// Deserialization:
///
/// * [`UnknownFields`](crate::de::UnknownFields): what happens with keys
///   of structs that no field takes.  Ignored by default.
/// * [`DuplicateKeys`](crate::de::DuplicateKeys): what happens if a key is
///   given more than once.  Rejected by default (query strings and
///   environment variables use the last value).
/// * [`LexicalRules`](crate::de::LexicalRules): how
///   [lexical atoms](crate::Atom::Lexical) are interpreted.  Strict by
///   default (query strings, environment variables and CSV are lenient).
/// * [`Limits`](crate::de::Limits): limits of the nesting depth, the
///   number of events and items and the length of strings and bytes.
///   Unlimited by default.  Only read from the context and enforced by
///   the [`DeserializeDriver`](crate::de::DeserializeDriver).
/// * [`CollectErrors`](crate::de::CollectErrors): collects the errors of
///   the whole deserialization instead of failing on the first one,
///   optionally up to a limit.  Off by default.  Only read from the
///   context.
/// * [`TrackLocations`](crate::TrackLocations): asks the formats to
///   provide the [`Source`](crate::Source) to resolve input ranges into
///   lines and columns.  Off by default.
///
/// Serialization and deserialization:
///
/// * [`BytesFormat`](crate::BytesFormat): how bytes are represented in
///   formats without native bytes.  Base64 by default.
/// * [`OpenEnums`](crate::OpenEnums) (with the `open-enums` feature): the
///   registered variants of open enums.  Required to deserialize open
///   enums, only the registered variants can be deserialized.
///
/// Any other type that is [`Debug`], [`Send`], [`Sync`] and `'static` can
/// be a value too.  Types and formats read their own values with
/// [`State::get`](crate::State::get) (which falls back to the context) or
/// [`State::context`](crate::State::context):
///
/// ```
/// use deser::{Context, State};
///
/// #[derive(Debug)]
/// struct Greeting(&'static str);
///
/// let mut state = State::new();
/// state.set_context(Context::with(Greeting("hello")));
/// assert_eq!(state.get::<Greeting>().unwrap().0, "hello");
/// ```
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

    /// Creates a context with a value.
    pub fn with<T: Debug + Send + Sync + 'static>(value: T) -> Context {
        let mut context = Context::new();
        context.set(value);
        context
    }

    /// Sets a value, replacing a value of the same type.
    ///
    /// If the values are shared with clones of the context, they are copied
    /// (the values themselves are shared).
    pub fn set<T: Debug + Send + Sync + 'static>(&mut self, value: T) {
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
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.values.as_ref().is_none_or(|values| values.is_empty())
    }

    /// Adds the values of `defaults` whose types this context has no value
    /// for.
    ///
    /// Returns `false` if nothing was added.
    pub(crate) fn fill_from(&mut self, defaults: &Context) -> bool {
        let Some(ref defaults) = defaults.values else {
            return false;
        };
        let own = match self.values {
            Some(ref mut own) if !own.is_empty() => own,
            _ => {
                // the values are shared with the defaults
                self.values = Some(defaults.clone());
                return !defaults.is_empty();
            }
        };
        if Arc::ptr_eq(own, defaults) {
            return false;
        }
        let missing: Vec<Entry> = defaults
            .iter()
            .filter(|(key, _)| !own.iter().any(|(own_key, _)| own_key.0 == key.0))
            .cloned()
            .collect();
        if missing.is_empty() {
            return false;
        }
        Arc::make_mut(own).extend(missing);
        true
    }
}

/// Contexts are equal if they share their values (one is a clone of the
/// other and neither was changed since) or if both are empty.  The values
/// themselves are not compared, they do not need to implement `PartialEq`.
impl PartialEq for Context {
    fn eq(&self, other: &Context) -> bool {
        match (&self.values, &other.values) {
            (Some(a), Some(b)) if Arc::ptr_eq(a, b) => true,
            _ => self.is_empty() && other.is_empty(),
        }
    }
}

impl Eq for Context {}

// The values are shared and only lent out immutably.  As they are `Sync`
// they can only change through synchronized interior mutability (like
// mutexes and atomics) which is unwind safe.  Without this the contexts
// (and the configurations of the formats which hold one) could not be
// used across `catch_unwind`.
impl core::panic::UnwindSafe for Context {}
impl core::panic::RefUnwindSafe for Context {}

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

    let mut context = Context::with(1u32);
    context.set("x");
    assert_eq!(context.get::<u32>(), Some(&1));
    assert_eq!(context.get::<&str>(), Some(&"x"));
    assert_eq!(context.get::<u64>(), None);

    // clones share the values until they are changed
    let mut other = context.clone();
    other.set(2u32);
    assert_eq!(context.get::<u32>(), Some(&1));
    assert_eq!(other.get::<u32>(), Some(&2));
    assert_eq!(other.get::<&str>(), Some(&"x"));
    assert_eq!(format!("{:?}", other), r#"{u32: 2, &str: "x"}"#);

    // contexts are equal if they share their values
    assert_eq!(context, context.clone());
    assert_ne!(context, other);
    assert_ne!(context, Context::with(1u32));
    assert_eq!(empty, Context::default());

    fn unwind_safe<T: core::panic::UnwindSafe + core::panic::RefUnwindSafe>() {}
    unwind_safe::<Context>();
}

#[test]
fn test_fill_from() {
    let defaults = {
        let mut context = Context::with(1u32);
        context.set("x");
        context
    };

    // an empty context shares the values of the defaults
    let mut context = Context::new();
    assert!(context.fill_from(&defaults));
    assert_eq!(context, defaults);
    assert!(!context.fill_from(&defaults));
    assert!(!context.fill_from(&Context::new()));

    // values of the context are kept, the others are added
    let mut context = Context::with(2u32);
    assert!(context.fill_from(&defaults));
    assert_eq!(context.get::<u32>(), Some(&2));
    assert_eq!(context.get::<&str>(), Some(&"x"));
    assert!(!context.fill_from(&defaults));
    assert_eq!(defaults.get::<u32>(), Some(&1));
}
