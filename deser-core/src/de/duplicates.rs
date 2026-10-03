use crate::State;
use crate::error::{Error, ErrorKind};
use alloc::format;
use alloc::string::String;

/// What happens if a key is given more than once.
///
/// JSON objects can contain the same key more than once and query strings
/// commonly repeat keys.  Where a single value is expected (the field of a
/// struct or an entry of a map) the policy decides what happens.  It's an
/// extension value, usually configured in the [`Context`](crate::Context)
/// (or in the [`State`], see [`set`](Self::set)).  The default is
/// [`Error`](Self::Error): if the same key could mean different values to
/// different parsers (a proxy might use the first value, the application the
/// last) the input is rejected.  Formats can have other defaults (query
/// strings and environment variables use the last value), which the
/// context overrides.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::de::DuplicateKeys;
/// use deser::Context;
///
/// type Map = BTreeMap<String, u32>;
///
/// let input = r#"{"a": 1, "a": 2}"#;
/// let err = deser_json::from_str::<Map>(input).unwrap_err();
/// assert_eq!(err.message(), "duplicate key in map");
///
/// let config = deser_json::DeserializerConfig::builder()
///     .context(Context::with(DuplicateKeys::Last))
///     .build();
/// assert_eq!(config.from_str::<Map>(input).unwrap()["a"], 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DuplicateKeys {
    /// The last value is used.
    Last,
    /// The first value is used, later ones are ignored.
    First,
    /// Duplicate keys are rejected.
    #[default]
    Error,
}

impl DuplicateKeys {
    /// Returns the policy of a deserialization.
    #[inline]
    pub fn of(state: &State) -> DuplicateKeys {
        state.get::<DuplicateKeys>().copied().unwrap_or_default()
    }

    /// Sets the policy of a deserialization.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.get_mut::<DuplicateKeys>() = self;
    }

    /// Sets the policy unless the state or the context has one.
    ///
    /// Formats use this for their default (see [`State::set_default`]).
    #[inline]
    pub fn set_default(self, state: &mut State) {
        state.set_default(self);
    }

    /// Decides if a duplicate value is used.
    ///
    /// Returns `Ok(true)` if the value replaces the previous one, `Ok(false)`
    /// if it's ignored.  The name is used for the error.
    #[cold]
    pub(crate) fn resolve(self, what: impl FnOnce() -> String) -> Result<bool, Error> {
        match self {
            DuplicateKeys::Last => Ok(true),
            DuplicateKeys::First => Ok(false),
            DuplicateKeys::Error => Err(Error::new(ErrorKind::DuplicateKey, what())),
        }
    }
}

/// Marks a field of a struct as seen.
///
/// Returns `true` if it was seen before.
#[cfg(feature = "derive")]
#[inline(always)]
pub fn mark_seen(seen: &mut [u64], index: usize) -> bool {
    let (word, bit) = (index / 64, 1u64 << (index % 64));
    let seen_before = seen[word] & bit != 0;
    seen[word] |= bit;
    seen_before
}

/// Returns `true` if the field with the index was seen.
#[cfg(feature = "derive")]
pub(crate) fn is_seen(seen: &[u64], index: usize) -> bool {
    seen[index / 64] & (1u64 << (index % 64)) != 0
}

/// Decides if the value of a field given more than once is used.
///
/// Returns `Ok(true)` if the value replaces the previous one.
#[cold]
pub fn duplicate_field(name: &str, state: &State) -> Result<bool, Error> {
    DuplicateKeys::of(state).resolve(|| format!("duplicate field `{}`", name))
}
