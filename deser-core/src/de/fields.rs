//! Support for the sinks of derived structs.
//!
//! The logic that does not depend on the types of the fields lives here so
//! that it exists once instead of once per derived struct.  This is not part
//! of the public API, it's used by the derive through `deser::__derive`.
use std::borrow::Cow;

use crate::State;
use crate::de::duplicates::{duplicate_field, mark_seen};
use crate::de::unknown::{unknown_field, wants_unknown_fields};
use crate::de::{Sink, SinkHandle};
use crate::error::Error;
use crate::event::Atom;

/// The index of a key that is not a field.
const UNKNOWN: usize = usize::MAX;

/// A function that returns the index of the field for a key.
pub type FieldLookup = fn(&str) -> Option<usize>;

/// The sink for the keys of a derived struct.
///
/// Keys are resolved to the index of their field with a function generated
/// by the derive.  The names of keys that are not fields are only retained
/// if they are needed: always if the sink was created with `retain` (for
/// structs with flattened fields which look them up on the flattened fields
/// and structs that reject unknown keys), otherwise only if the
/// [`UnknownFields`](crate::de::UnknownFields) policy wants them.
pub struct FieldKeySink {
    index: usize,
    other: Option<String>,
    // the position of the key in the input if it's retained in `other`
    offset: Option<usize>,
    lookup: FieldLookup,
    retain: bool,
}

/// What a derived struct with flattened fields does with the next value.
pub enum NextField {
    /// The value is ignored (a duplicate that is ignored).
    Ignore,
    /// The value is for the field with the index.
    Field(usize),
    /// The value is for a key that is not a field.
    Other(String),
}

impl FieldKeySink {
    /// Creates the key sink.
    #[inline]
    pub fn new(lookup: FieldLookup, retain: bool) -> FieldKeySink {
        FieldKeySink {
            index: UNKNOWN,
            other: None,
            offset: None,
            lookup,
            retain,
        }
    }

    /// Resets the key before the next key is deserialized.
    #[inline]
    pub fn reset(&mut self) {
        self.index = UNKNOWN;
        self.other = None;
    }

    /// Sets the field of the next value.
    #[inline]
    pub fn set_index(&mut self, index: usize) {
        self.reset();
        self.index = index;
    }

    /// Returns the position of the last key that is not a field.
    #[inline]
    pub fn offset(&self) -> Option<usize> {
        self.offset
    }

    /// Deserializes a key from an atom.
    ///
    /// `lookup` must be the function the sink was created with, it's passed
    /// again so that it can be called directly (this is always inlined).
    #[inline(always)]
    pub fn key_atom(
        &mut self,
        atom: Atom,
        lookup: FieldLookup,
        state: &mut State,
    ) -> Result<(), Error> {
        match atom {
            // the string is moved out so that only it needs to be dropped
            Atom::Str(key) | Atom::Lexical(key) => {
                match lookup(&key) {
                    Some(index) => self.index = index,
                    None => self.other_key(key, state),
                }
                Ok(())
            }
            other => self.key_atom_slow(other, state),
        }
    }

    #[inline(never)]
    fn key_atom_slow(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.reset();
        self.atom(atom, state)
    }

    /// Records a key that is not a field.
    #[inline(never)]
    fn other_key(&mut self, key: Cow<'_, str>, state: &State) {
        self.index = UNKNOWN;
        self.other = if self.retain || wants_unknown_fields(state) {
            self.offset = state.input_range().map(|range| range.start);
            Some(key.into_owned())
        } else {
            None
        };
    }

    /// Takes the field index of the next value of a struct without
    /// flattened fields.
    ///
    /// Fields that were already seen are resolved with the duplicate key
    /// policy, keys that are not fields with the unknown fields policy (or
    /// rejected if `deny` is set).  Returns `None` if the value is ignored.
    #[inline]
    pub fn next_index(
        &mut self,
        seen: &mut [u64],
        fields: &[&str],
        deny: bool,
        state: &mut State,
    ) -> Result<Option<usize>, Error> {
        let index = std::mem::replace(&mut self.index, UNKNOWN);
        if index != UNKNOWN {
            Ok(
                if !mark_seen(seen, index) || duplicate_field(fields[index], state)? {
                    Some(index)
                } else {
                    None
                },
            )
        } else {
            if self.other.is_some() {
                self.unknown_key(fields, deny, state)?;
            }
            Ok(None)
        }
    }

    #[inline(never)]
    fn unknown_key(&mut self, fields: &[&str], deny: bool, state: &mut State) -> Result<(), Error> {
        match self.other.take() {
            Some(key) => unknown_field(&key, self.offset, fields, deny, state),
            None => Ok(()),
        }
    }

    /// Takes the key of the next value of a struct with flattened fields.
    ///
    /// Fields that were already seen are resolved with the duplicate key
    /// policy.  Keys that are not fields are returned, they are unknown if
    /// no flattened field takes them.
    #[inline(never)]
    pub fn next_field(
        &mut self,
        seen: &mut [u64],
        fields: &[&str],
        state: &State,
    ) -> Result<NextField, Error> {
        let index = std::mem::replace(&mut self.index, UNKNOWN);
        Ok(if index != UNKNOWN {
            if !mark_seen(seen, index) || duplicate_field(fields[index], state)? {
                NextField::Field(index)
            } else {
                NextField::Ignore
            }
        } else {
            match self.other.take() {
                Some(key) => NextField::Other(key),
                None => NextField::Ignore,
            }
        })
    }
}

impl<'de> Sink<'de> for FieldKeySink {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(key) | Atom::Lexical(key) => {
                match (self.lookup)(&key) {
                    Some(index) => self.index = index,
                    None => self.other_key(key, state),
                }
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("string")
    }
}

/// The fields of a derived struct that is updated (see
/// [`Deserialize::deserialize_update`](crate::de::Deserialize::deserialize_update)).
///
/// The derive implements this for structs, [`StructUpdateSink`] does
/// everything else, it exists once for all structs.
pub trait UpdateFields<'de>: Send {
    /// Returns the sink that updates the field with the index.
    fn update_field(&mut self, index: usize) -> SinkHandle<'_, 'de>;
}

/// The sink that updates a derived struct.
///
/// The fields that are given are updated, the others are kept.
pub struct StructUpdateSink<'a, 'de> {
    value: &'a mut (dyn UpdateFields<'de> + 'a),
    key: FieldKeySink,
    seen: Vec<u64>,
    fields: &'static [&'static str],
    name: &'static str,
    deny: bool,
}

impl<'a, 'de> StructUpdateSink<'a, 'de> {
    /// Creates the sink that updates a struct.
    ///
    /// The arguments are the ones of [`FieldKeySink::new`] and
    /// [`FieldKeySink::next_index`] and the name of the struct for errors.
    pub fn handle(
        value: &'a mut (dyn UpdateFields<'de> + 'a),
        lookup: FieldLookup,
        retain: bool,
        fields: &'static [&'static str],
        name: &'static str,
        deny: bool,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(StructUpdateSink {
            value,
            key: FieldKeySink::new(lookup, retain),
            seen: vec![0; fields.len().div_ceil(64)],
            fields,
            name,
            deny,
        })
    }
}

impl<'a, 'de> Sink<'de> for StructUpdateSink<'a, 'de> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.key.reset();
        Ok(SinkHandle::to(&mut self.key))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(
            match self
                .key
                .next_index(&mut self.seen, self.fields, self.deny, state)?
            {
                Some(index) => self.value.update_field(index),
                None => SinkHandle::null(),
            },
        )
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        // the value is deserialized like the value of a key (so that the
        // struct can be flattened)
        match (self.key.lookup)(key) {
            Some(index) => {
                self.key.set_index(index);
                self.next_value(state).map(Some)
            }
            None => Ok(None),
        }
    }
}
