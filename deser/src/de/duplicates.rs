use crate::State;
use crate::error::{Error, ErrorKind};

/// What happens if a key is given more than once.
///
/// JSON objects can contain the same key more than once and query strings
/// commonly repeat keys.  Where a single value is expected (the field of a
/// struct or an entry of a map) the policy in the [`State`] decides what
/// happens (see [`State::set_duplicate_keys`]).  The default is
/// [`Last`](Self::Last).
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::de::{DeserializeDriver, DuplicateKeys};
/// use deser::Event;
///
/// let mut out = None::<BTreeMap<String, u32>>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// driver.state_mut().set_duplicate_keys(DuplicateKeys::Error);
/// driver.emit(Event::map_start()).unwrap();
/// for value in [1u64, 2] {
///     driver.emit("a").unwrap();
///     driver.emit(value).unwrap();
/// }
/// let err = driver.emit(Event::MapEnd).unwrap_err();
/// assert_eq!(err.message(), "duplicate key in map");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DuplicateKeys {
    /// The last value is used.
    #[default]
    Last,
    /// The first value is used, later ones are ignored.
    First,
    /// Duplicate keys are rejected.
    Error,
}

impl DuplicateKeys {
    /// Decides if a duplicate value is used.
    ///
    /// Returns `Ok(true)` if the value replaces the previous one, `Ok(false)`
    /// if it's ignored.  The name is used for the error.
    #[cold]
    pub(crate) fn resolve(self, what: impl FnOnce() -> String) -> Result<bool, Error> {
        match self {
            DuplicateKeys::Last => Ok(true),
            DuplicateKeys::First => Ok(false),
            DuplicateKeys::Error => Err(Error::new(ErrorKind::Unexpected, what())),
        }
    }
}

/// Marks a field of a struct as seen.
///
/// Returns `true` if it was seen before.
#[inline(always)]
pub fn mark_seen(seen: &mut [u64], index: usize) -> bool {
    let (word, bit) = (index / 64, 1u64 << (index % 64));
    let seen_before = seen[word] & bit != 0;
    seen[word] |= bit;
    seen_before
}

/// Decides if the value of a field given more than once is used.
///
/// Returns `Ok(true)` if the value replaces the previous one.
#[cold]
pub fn duplicate_field(name: &str, state: &State) -> Result<bool, Error> {
    state
        .duplicate_keys()
        .resolve(|| format!("duplicate field '{}'", name))
}
