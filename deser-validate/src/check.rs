//! The `Check` adapter.
use std::marker::PhantomData;

use deser_core::adapters::{DeserializeAs, Same, SerializeAs};
use deser_core::de::{OwnedSink, SinkHandle, checked_update};
use deser_core::ser::{Chunk, Describe};
use deser_core::{Atom, ContainerShape, Error, State};

use crate::checked::CheckSink;
use crate::{Validator, Violation};

/// An adapter that validates values with `V`.
///
/// The value is deserialized with the adapter `A` ([`Same`] by default)
/// and validated once it's complete.  If it's invalid, deserialization
/// fails with an error that points at the start of the value and has the
/// [`Violation`](crate::Violation) attached.  The type of the field does
/// not change, and the adapter composes with the other adapters and the
/// containers:
///
/// ```
/// use deser::Deserialize;
/// use deser::adapters::DisplayFromStr;
/// use deser_validate::{Check, Email, MaxLen, NonEmpty, validator};
///
/// validator!(NonZero(port: &u16) => *port != 0, "must not be zero");
///
/// #[derive(Deserialize, Debug)]
/// struct Server {
///     #[deser(as = Check<NonZero>)]
///     port: u16,
///     #[deser(as = Vec<Check<Email>>)]
///     admins: Vec<String>,
///     #[deser(as = Option<Check<(NonEmpty, MaxLen<64>)>>)]
///     name: Option<String>,
///     // validates the value that `DisplayFromStr` parsed
///     #[deser(as = Check<NonZero, DisplayFromStr>, default)]
///     legacy_port: u16,
/// }
///
/// let err = deser_json::from_str::<Server>(r#"{"port": 0, "admins": []}"#).unwrap_err();
/// assert_eq!(
///     err.to_string(),
///     "Unexpected: invalid value: must not be zero at line 1 column 10"
/// );
/// let err = deser_json::from_str::<Server>(r#"{"port": 1, "admins": ["x"]}"#).unwrap_err();
/// assert_eq!(
///     err.to_string(),
///     "Unexpected: invalid value: must be an email address at line 1 column 24"
/// );
/// ```
///
/// Values that are missing (see [`DeserializeAs::initial_value_as`]) are
/// only used if they are valid, otherwise the value is required.
/// Serialization uses the inner adapter.
///
/// # Checks Across Fields
///
/// On a type, `Check` wraps its derived implementation (written as `_`).
/// The validator then sees the whole value:
///
/// ```
/// use deser::Deserialize;
/// use deser_validate::{Check, validator};
///
/// #[derive(Deserialize, Debug)]
/// #[deser(deserialize_as = Check<PortRangeRules, _>)]
/// struct PortRange {
///     min: u16,
///     max: u16,
/// }
///
/// validator!(PortRangeRules(range: &PortRange) => range.min <= range.max, "min is larger than max");
///
/// let err = deser_json::from_str::<PortRange>(r#"{"min": 90, "max": 80}"#).unwrap_err();
/// assert_eq!(err.to_string(), "Unexpected: invalid value: min is larger than max at line 1 column 1");
/// ```
///
/// # Updates
///
/// When a value is updated (see
/// [`Deserialize::deserialize_update`](deser_core::Deserialize::deserialize_update)),
/// it's updated with `A` and validated once the update is complete.  Types
/// that update in place (like derived structs) are merged and then
/// validated as a whole.  If the updated value is invalid, the update fails
/// but the value keeps the update, like values keep what an update changed
/// before it failed.
pub struct Check<V, A = Same>(PhantomData<fn() -> (V, A)>);

impl<'de, T, V, A> DeserializeAs<'de, T> for Check<V, A>
where
    T: Send,
    V: Validator<T> + 'static,
    A: DeserializeAs<'de, T>,
{
    fn deserialize_into_as(out: &mut Option<T>) -> SinkHandle<'_, 'de> {
        CheckSink::<T, V, T>::handle(out, OwnedSink::deserialize_as::<A>(), |value| value)
    }

    fn initial_value_as() -> Option<T> {
        A::initial_value_as().filter(|value| V::validate(value).is_ok())
    }

    /// Updates the value with `A` and validates it once the update is
    /// complete.
    fn deserialize_update_as(value: &mut T) -> SinkHandle<'_, 'de>
    where
        T: Send,
    {
        checked_update(value, A::deserialize_update_as, |value| {
            V::validate(value).map_err(Violation::into_error)
        })
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<T>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        A::__private_atom_into_as(out, atom, state)?;
        validate_slot::<T, V>(out)
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<T>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        A::__private_borrowed_atom_into_as(out, atom, state)?;
        validate_slot::<T, V>(out)
    }
}

/// Validates the value in a slot, the invalid value is removed.
///
/// The error gets the location of the atom from the driver.
fn validate_slot<T, V: Validator<T>>(slot: &mut Option<T>) -> Result<(), Error> {
    if let Some(ref value) = *slot
        && let Err(violation) = V::validate(value)
    {
        *slot = None;
        return Err(violation.into_error());
    }
    Ok(())
}

impl<T: ?Sized, V: 'static, A: SerializeAs<T>> SerializeAs<T> for Check<V, A> {
    fn serialize_as<'a>(value: &'a T, state: &mut State) -> Result<Chunk<'a>, Error> {
        A::serialize_as(value, state)
    }

    fn finish_as(value: &T, state: &mut State) -> Result<(), Error> {
        A::finish_as(value, state)
    }

    fn is_optional_as(value: &T) -> bool {
        A::is_optional_as(value)
    }

    fn container_shape_as(value: &T) -> ContainerShape {
        A::container_shape_as(value)
    }

    fn describe_as(value: &T, d: &mut dyn Describe) {
        A::describe_as(value, d)
    }
}
