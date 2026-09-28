//! Support for the sinks of derived structs.
//!
//! The logic that does not depend on the types of the fields lives here so
//! that it exists once instead of once per derived struct.  This is not part
//! of the public API, it's used by the derive through `deser::__derive`.
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::NonNull;

use crate::State;
use crate::Text;
use crate::de::CollectedErrors;
use crate::de::duplicates::{duplicate_field, is_seen, mark_seen};
use crate::de::sinkbox::StructBox;
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
    fn other_key(&mut self, key: Text<'_>, state: &State) {
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
        let index = core::mem::replace(&mut self.index, UNKNOWN);
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
        let index = core::mem::replace(&mut self.index, UNKNOWN);
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

/// What a derived struct (without flattened fields) is, independent of the
/// types of its fields.
pub struct StructInfo {
    /// The name of the struct for errors.
    pub name: &'static str,
    /// The names of the fields by index.
    pub fields: &'static [&'static str],
    /// Returns the index of the field for a key.
    pub lookup: FieldLookup,
    /// Rejects keys that are not fields.
    pub deny: bool,
    /// The fields can borrow from the data (the struct has generics), see
    /// [`StructFields::field_borrowed_atom`].
    pub borrows: bool,
}

/// The fields of a derived struct while it's deserialized.
///
/// The derive implements this for a struct that holds the slot of the
/// struct and a slot for every field.  [`StructSink`] does everything that
/// does not depend on the types of the fields, it exists once for all
/// structs.
pub trait StructFields<'de>: Send {
    /// Returns the sink of the field with the index.
    fn field_sink(&mut self, index: usize) -> SinkHandle<'_, 'de>;

    /// Deserializes an atom into the field with the index.
    fn field_atom(&mut self, index: usize, atom: Atom, state: &mut State) -> Result<(), Error>;

    /// Deserializes a borrowed atom into the field with the index.
    ///
    /// This is only invoked if [`StructInfo::borrows`] is set, otherwise
    /// borrowed atoms are passed to [`field_atom`](Self::field_atom).
    fn field_borrowed_atom(
        &mut self,
        index: usize,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        let _ = (index, atom, state);
        unreachable!()
    }

    /// Builds the struct from the fields and places it in the slot.
    ///
    /// If [`StructFinish::ok`] returns `false` or required fields are
    /// missing, this fails with [`StructFinish::missing`].  The fields are
    /// not dropped afterwards, so all values have to be taken out.
    fn finish(&mut self, finish: &mut StructFinish<'_>, state: &mut State) -> Result<(), Error>;
}

/// Passed to [`StructFields::finish`].
pub struct StructFinish<'a> {
    seen: &'a Seen,
    errors: &'a mut CollectedErrors,
    fields: &'static [&'static str],
}

impl StructFinish<'_> {
    /// Returns `true` if no errors were collected, the struct is only built
    /// then.
    #[inline(always)]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the error if the struct is not built.
    ///
    /// `missing` tells for every field (by index) if it's required and has
    /// no value.  The error holds the errors that were collected and the
    /// fields that are missing.  Fields that were seen but have no value
    /// failed, they are only missing if no errors were collected.
    #[cold]
    #[inline(never)]
    pub fn missing(&mut self, missing: &[bool], state: &State) -> Error {
        if self.errors.is_empty() {
            return crate::__derive::missing_field(missing, self.fields, state);
        }
        for (index, (missing, name)) in missing.iter().zip(self.fields).enumerate() {
            if *missing && !self.seen.contains(index) {
                self.errors
                    .push(crate::__derive::new_missing_field_error(name, state), state);
            }
        }
        match self.errors.take() {
            Some(err) => err,
            None => unreachable!(),
        }
    }
}

/// The fields of a struct that were seen.
///
/// The first 64 fields are tracked inline, the others (of large structs)
/// in words that are allocated once they are needed.
struct Seen {
    small: u64,
    large: Option<Box<[u64]>>,
}

impl Seen {
    /// Returns `true` if the field with the index was seen.
    fn contains(&self, index: usize) -> bool {
        if index < 64 {
            self.small & (1 << index) != 0
        } else {
            match self.large {
                Some(ref large) => is_seen(large, index),
                None => false,
            }
        }
    }
}

/// The sink of a derived struct without flattened fields.
///
/// It exists once for all structs, the fields are behind
/// [`StructFields`].
pub struct StructSink<'a, 'de> {
    // the fields are in the same block as the sink (see `StructBox`) which
    // drops them
    fields: NonNull<dyn StructFields<'de> + 'a>,
    key: FieldKeySink,
    seen: Seen,
    errors: CollectedErrors,
    info: &'static StructInfo,
    // the fields are empty once they are finished, they are not dropped
    finished: bool,
}

// SAFETY: the sink owns the fields which are `Send`
unsafe impl Send for StructSink<'_, '_> {}

impl<'a, 'de> StructSink<'a, 'de> {
    /// Creates the sink of a struct.
    #[inline]
    pub fn handle<F: StructFields<'de> + 'a>(
        fields: F,
        info: &'static StructInfo,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::from_struct_box(StructBox::new(fields, info))
    }

    /// Creates the sink for fields, see [`StructBox`].
    #[inline(always)]
    pub(crate) fn new(
        fields: NonNull<dyn StructFields<'de> + 'a>,
        info: &'static StructInfo,
    ) -> StructSink<'a, 'de> {
        StructSink {
            fields,
            key: FieldKeySink::new(info.lookup, info.deny),
            seen: Seen {
                small: 0,
                large: None,
            },
            errors: CollectedErrors::new(),
            info,
            finished: false,
        }
    }

    /// Returns the pointer to the fields and if they need to be dropped.
    #[inline(always)]
    pub(crate) fn fields_ptr(&self) -> (NonNull<dyn StructFields<'de> + 'a>, bool) {
        (self.fields, !self.finished)
    }

    #[inline(always)]
    fn fields(&mut self) -> &mut (dyn StructFields<'de> + 'a) {
        // SAFETY: the fields are valid while the sink exists and only
        // borrowed through it
        unsafe { self.fields.as_mut() }
    }

    /// Takes the field index of the next value.
    ///
    /// Returns `None` if the value is ignored, see
    /// [`FieldKeySink::next_index`].
    #[inline(always)]
    fn next_index(&mut self, state: &mut State) -> Result<Option<usize>, Error> {
        let index = core::mem::replace(&mut self.key.index, UNKNOWN);
        if index < 64 && self.seen.small & (1 << index) == 0 {
            self.seen.small |= 1 << index;
            return Ok(Some(index));
        }
        self.next_index_slow(index, state)
    }

    /// Handles keys that are not fields, duplicate fields and the fields
    /// of large structs.
    #[inline(never)]
    fn next_index_slow(&mut self, index: usize, state: &mut State) -> Result<Option<usize>, Error> {
        if index == UNKNOWN {
            if self.key.other.is_some() {
                self.key
                    .unknown_key(self.info.fields, self.info.deny, state)?;
            }
            return Ok(None);
        }
        let seen_before = if index < 64 {
            true
        } else {
            let words = self.info.fields.len().div_ceil(64);
            let large = self
                .seen
                .large
                .get_or_insert_with(|| vec![0; words].into_boxed_slice());
            mark_seen(large, index)
        };
        if !seen_before || duplicate_field(self.info.fields[index], state)? {
            Ok(Some(index))
        } else {
            Ok(None)
        }
    }
}

impl<'a, 'de> Sink<'de> for StructSink<'a, 'de> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.info.name)
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.key.reset();
        Ok(SinkHandle::to(&mut self.key))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(match self.next_index(state)? {
            Some(index) => self.fields().field_sink(index),
            None => SinkHandle::null(),
        })
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.key.key_atom(atom, self.key.lookup, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.next_index(state)? {
            Some(index) => self.fields().field_atom(index, atom, state),
            None => Ok(()),
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        // keys are only matched, they do not need to be borrowed
        self.key.key_atom(atom, self.key.lookup, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match self.next_index(state)? {
            Some(index) if self.info.borrows => {
                self.fields().field_borrowed_atom(index, atom, state)
            }
            Some(index) => self.fields().field_atom(index, atom, state),
            None => Ok(()),
        }
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        // the value is deserialized like the value of a key (so that the
        // struct can be flattened)
        match (self.info.lookup)(key) {
            Some(index) => {
                self.key.set_index(index);
                self.next_value(state).map(Some)
            }
            None => Ok(None),
        }
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.errors.collect(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.finished = true;
        let mut finish = StructFinish {
            seen: &self.seen,
            errors: &mut self.errors,
            fields: self.info.fields,
        };
        // SAFETY: see `fields`
        unsafe { self.fields.as_mut() }.finish(&mut finish, state)
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
    info: &'static StructInfo,
}

impl<'a, 'de> StructUpdateSink<'a, 'de> {
    /// Creates the sink that updates a struct.
    pub fn handle(
        value: &'a mut (dyn UpdateFields<'de> + 'a),
        info: &'static StructInfo,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(StructUpdateSink {
            value,
            key: FieldKeySink::new(info.lookup, info.deny),
            seen: vec![0; info.fields.len().div_ceil(64)],
            info,
        })
    }
}

impl<'a, 'de> Sink<'de> for StructUpdateSink<'a, 'de> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.info.name)
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
                .next_index(&mut self.seen, self.info.fields, self.info.deny, state)?
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
