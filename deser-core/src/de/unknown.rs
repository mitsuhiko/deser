// most of this is only used by derived structs
#![cfg_attr(not(feature = "derive"), allow(dead_code))]
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use crate::Source;
use crate::State;
use crate::error::{Error, ErrorKind, push_expected};
use crate::sync::{Mutex, MutexGuard};

/// What happens with keys of structs that no field takes.
///
/// Derived structs ignore keys they do not know by default.  This policy
/// changes that for all structs of a deserialization, while
/// `#[deser(deny_unknown_fields)]` rejects unknown keys for a single type
/// regardless of the policy.  It's an extension value in the [`State`] (see
/// [`set`](Self::set)).
///
/// Only the struct that the key is given to decides: flattened fields are
/// asked if they take a key first (see
/// [`value_for_key`](crate::de::Sink::value_for_key)), so a key is only
/// unknown if neither the struct nor any of its flattened fields take it.
///
/// ```
/// use deser::de::{DeserializeDriver, UnknownFields};
/// use deser::{Deserialize, Event};
///
/// #[derive(Deserialize)]
/// struct Config {
///     name: String,
/// }
///
/// let mut out = None::<Config>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// UnknownFields::Error.set(driver.state_mut());
/// driver.emit(Event::map_start()).unwrap();
/// driver.emit("name").unwrap();
/// driver.emit("demo").unwrap();
/// driver.emit("nmae").unwrap();
/// let err = driver.emit("demo").unwrap_err();
/// assert_eq!(err.message(), "unknown field `nmae`, expected `name`");
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum UnknownFields {
    /// Unknown keys are ignored.
    #[default]
    Ignore,
    /// Unknown keys are rejected.
    Error,
    /// Unknown keys are ignored but reported to a [`IgnoredFields`].
    Collect(IgnoredFields),
}

/// The policy of deserializations that do not set one.
static IGNORE: UnknownFields = UnknownFields::Ignore;

impl UnknownFields {
    /// Returns the policy of a deserialization.
    #[inline]
    pub fn of(state: &State) -> &UnknownFields {
        state.get::<UnknownFields>().unwrap_or(&IGNORE)
    }

    /// Sets the policy of a deserialization.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.get_mut::<UnknownFields>() = self;
    }
}

/// Collects the keys ignored with [`UnknownFields::Collect`].
///
/// Every ignored key is reported as an [`Error`] which carries the same
/// information as if the key was rejected: the location (if the format
/// provides it) and the context registered in the state (for instance the
/// path with `deser-path`).  The line and column are only available if the
/// format provides the [`Source`] (for instance with its `track_locations`
/// option).
///
/// The collector is shared between its clones, the clone in the state
/// reports to the one that was set up:
///
/// ```
/// use deser::de::{DeserializeDriver, IgnoredFields, UnknownFields};
/// use deser::{Deserialize, Event};
///
/// #[derive(Deserialize)]
/// struct Config {
///     name: String,
/// }
///
/// let ignored = IgnoredFields::new();
/// let mut out = None::<Config>;
/// {
///     let mut driver = DeserializeDriver::new(&mut out);
///     UnknownFields::Collect(ignored.clone()).set(driver.state_mut());
///     for event in [
///         Event::map_start(),
///         "name".into(),
///         "demo".into(),
///         "nmae".into(),
///         "x".into(),
///         Event::MapEnd,
///     ] {
///         driver.emit(event).unwrap();
///     }
/// }
/// let ignored = ignored.take();
/// assert_eq!(ignored.len(), 1);
/// assert_eq!(ignored[0].message(), "unknown field `nmae`, expected `name`");
/// ```
#[derive(Clone, Default)]
pub struct IgnoredFields {
    errors: Arc<Mutex<Vec<Error>>>,
}

impl IgnoredFields {
    /// Creates an empty collector.
    pub fn new() -> IgnoredFields {
        IgnoredFields::default()
    }

    /// Takes the ignored keys reported so far.
    pub fn take(&self) -> Vec<Error> {
        core::mem::take(&mut *self.lock())
    }

    /// Returns the number of ignored keys reported so far.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Returns `true` if no ignored keys were reported.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Error>> {
        self.errors.lock()
    }

    fn push(&self, err: Error) {
        self.lock().push(err);
    }
}

impl fmt::Debug for IgnoredFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IgnoredFields").field(&*self.lock()).finish()
    }
}

/// Keys offered to a flattened value that it took before it knew that it
/// does not use them.
///
/// Internally tagged enums take all keys until they know their variant.  If
/// they are flattened into a struct, the keys the variant does not take are
/// unknown keys of that struct, which is only known once the struct already
/// handed them out.  They are reported through the state when the enum
/// finishes.  The struct the value is flattened into takes them after it
/// finished its flattened fields.  If that struct is flattened itself they
/// are left for the struct it's flattened into.
#[derive(Debug, Default)]
pub(crate) struct UnclaimedKeys(Vec<Error>);

/// Returns `true` if the names of unknown keys are needed.
///
/// Derived structs only retain the names of keys they do not know if this
/// returns `true` (or they need them for other reasons).
#[inline]
pub fn wants_unknown_fields(state: &State) -> bool {
    !matches!(UnknownFields::of(state), UnknownFields::Ignore)
}

/// Creates the error for an unknown key.
///
/// The expected field names are only listed if they are known, which is
/// not the case if there are flattened fields.
#[cold]
pub(crate) fn unknown_field_error(key: &str, fields: Option<&[&str]>) -> Error {
    let mut msg = format!("unknown field `{}`", key);
    if let Some(fields) = fields {
        push_expected(&mut msg, fields, "fields");
    }
    Error::new(ErrorKind::Unexpected, msg)
}

/// Handles a key of a struct that neither a field nor a flattened field took.
///
/// `offset` is the position of the key in the input, `fields` the names of
/// the fields of the struct (empty names are flattened fields).  If `deny`
/// is set, the key is rejected regardless of the policy.
#[cold]
pub fn unknown_field(
    key: &str,
    offset: Option<usize>,
    fields: &[&str],
    deny: bool,
    state: &mut State,
) -> Result<(), Error> {
    let fields = if fields.iter().any(|x| x.is_empty()) {
        None
    } else {
        Some(fields)
    };
    let make_error = || {
        let err = unknown_field_error(key, fields);
        match offset {
            Some(offset) => err.with_offset(offset),
            None => err,
        }
    };
    decide(make_error, deny, state)
}

/// Applies the policy for unknown keys to the error for a key.
fn decide(make_error: impl FnOnce() -> Error, deny: bool, state: &mut State) -> Result<(), Error> {
    if deny {
        return Err(make_error());
    }
    match UnknownFields::of(state) {
        UnknownFields::Ignore => Ok(()),
        UnknownFields::Error => Err(make_error()),
        UnknownFields::Collect(ignored) => {
            let ignored = ignored.clone();
            ignored.push(located(make_error(), state));
            Ok(())
        }
    }
}

/// Attaches the context of the current event to an error that is not
/// returned (and thus not located by the drivers and formats).
fn located(err: Error, state: &State) -> Error {
    let err = state.attach_error_context(err);
    match state.get::<Source>() {
        Some(source) => err.resolve_position(source.0.as_bytes()),
        None => err,
    }
}

/// Reports a key a flattened value took but did not use (see
/// [`UnclaimedKeys`]).
///
/// The error carries the context of the key, the state is the one of the
/// ongoing deserialization.
pub(crate) fn report_unclaimed_key(err: Error, state: &mut State) {
    state.get_mut::<UnclaimedKeys>().0.push(err);
}

/// Handles the keys that flattened values took but did not use.
///
/// This is invoked by derived structs after finishing their flattened
/// fields.  Structs that are flattened themselves (`standalone` is `false`)
/// leave the keys for the struct they are flattened into.
pub fn unclaimed_keys(standalone: bool, deny: bool, state: &mut State) -> Result<(), Error> {
    if !standalone {
        return Ok(());
    }
    let keys = match state.get::<UnclaimedKeys>() {
        Some(keys) if !keys.0.is_empty() => {
            core::mem::take(&mut state.get_mut::<UnclaimedKeys>().0)
        }
        _ => return Ok(()),
    };
    for err in keys {
        // the error already carries the context of the key
        decide(|| err, deny, state)?;
    }
    Ok(())
}
