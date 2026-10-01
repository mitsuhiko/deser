//! Support for the sinks of derived structs.
//!
//! The logic that does not depend on the types of the fields lives here so
//! that it exists once instead of once per derived struct.  This is not part
//! of the public API, it's used by the derive through `deser::__derive`.
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::State;
use crate::Text;
use crate::adapters::Same;
use crate::de::CollectedErrors;
use crate::de::atoms::{atom_into_handle, borrowed_atom_into_handle};
use crate::de::duplicates::{duplicate_field, is_seen, mark_seen};
use crate::de::sinkbox::ArenaStruct;
use crate::de::unknown::{unknown_field, wants_unknown_fields};
use crate::de::{Deserialize, Sink, SinkHandle, default_atom};
use crate::error::{Error, ErrorKind, discarded_error};
use crate::event::Atom;

/// The index of a key that is not a field.
const UNKNOWN: usize = usize::MAX;

/// A function that returns the index of the field for a key.
pub(crate) type FieldLookup = fn(&str) -> Option<usize>;

/// A function that returns `true` if the field with the index collects the
/// values of a repeated key (see
/// [`Deserialize::__private_collects`]).
pub(crate) type FieldCollects = fn(usize) -> bool;

/// How the value of a field is deserialized.
///
/// In a multimap (see
/// [`ContainerShape::with_multimap`](crate::ContainerShape::with_multimap))
/// fields that are collections collect the values of all occurrences of
/// their key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Collect {
    /// The value is the value of the field.
    No,
    /// The value is the first one that the field collects.
    First,
    /// The value is added to the values the field collected.
    Next,
}

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
    collects: FieldCollects,
    // `true` if the field is a collection whose key was given before
    repeated: bool,
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
    pub fn new(lookup: FieldLookup, collects: FieldCollects, retain: bool) -> FieldKeySink {
        FieldKeySink {
            index: UNKNOWN,
            other: None,
            offset: None,
            lookup,
            collects,
            repeated: false,
            retain,
        }
    }

    /// Returns how the value of the field with the index is deserialized.
    ///
    /// This is only needed for updates: collections whose key is given
    /// the first time are replaced, later values are added to them.  It
    /// must be called once per value.
    #[inline]
    pub fn collect(&mut self, index: usize, state: &State) -> Collect {
        match (state.is_multimap() && (self.collects)(index), self.repeated) {
            (false, _) => Collect::No,
            (true, false) => Collect::First,
            (true, true) => {
                self.repeated = false;
                Collect::Next
            }
        }
    }

    /// Decides if the value of a field that was given before is used.
    ///
    /// Collections in multimaps take all values, for other fields the
    /// duplicate key policy decides.
    #[cold]
    #[inline(never)]
    fn duplicate(&mut self, index: usize, fields: &[&str], state: &State) -> Result<bool, Error> {
        if state.is_multimap() && (self.collects)(index) {
            self.repeated = true;
            return Ok(true);
        }
        duplicate_field(fields[index], state)
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
                if !mark_seen(seen, index) || self.duplicate(index, fields, state)? {
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
            if !mark_seen(seen, index) || self.duplicate(index, fields, state)? {
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
            other => default_atom(self, other, state),
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
    /// [`FieldSlot::borrowed_atom`].
    pub borrows: bool,
}

/// The fields of a derived struct without flattened fields.
///
/// The derive implements this for a struct that holds the slot of the
/// struct and a [`FieldValue`] for every field.  [`StructSink`] does
/// everything that does not depend on the types of the fields, it exists
/// once for all structs, and what depends on the type of a field is done
/// by the [`FieldSlot`] of the field which exists once per type of field.
/// The derive only returns the slot of a field by its index and builds the
/// struct.
pub trait StructFields<'de>: Send {
    /// Returns the slot of the field with the index.
    fn field(&mut self, index: usize) -> &mut dyn FieldSlot<'de>;

    /// Returns the fields that want raw values as bits by index (see
    /// [`Deserialize::__private_raw`]).
    ///
    /// The last bit is set if a field from the 64th on wants a raw value,
    /// these are asked for it (see [`FieldSlot::raw`]).
    #[inline(always)]
    fn raw_fields() -> u64
    where
        Self: Sized,
    {
        0
    }

    /// Builds the struct from the fields and places it in the slot.
    ///
    /// If [`StructFinish::ok`] returns `false` or required fields are
    /// missing, this fails with [`StructFinish::missing`].  The fields are
    /// not dropped afterwards, so all values have to be taken out.
    fn finish(&mut self, finish: &mut StructFinish<'_>, state: &mut State) -> Result<(), Error>;
}

/// The value of a field of a derived struct while it's deserialized.
///
/// This is implemented by [`FieldValue`] for all types of fields (and
/// adapters), so the code exists once per type of field instead of once per
/// struct (see [`StructFields`]).
pub trait FieldSlot<'de>: Send {
    /// Returns `true` if the field collects the values of a repeated key
    /// (see [`Deserialize::__private_collects`]).
    fn collects(&self) -> bool;

    /// Returns the format of the raw value the field wants (see
    /// [`Deserialize::__private_raw`]).
    fn raw(&self) -> Option<&'static crate::ext::RawFormatInfo>;

    /// Returns the sink of the field, optionally collecting its value.
    fn sink(&mut self, collect: Collect, state: &mut State) -> SinkHandle<'_, 'de>;

    /// Deserializes an atom into the field.
    fn atom(&mut self, collect: Collect, atom: Atom, state: &mut State) -> Result<(), Error>;

    /// Deserializes a borrowed atom into the field.
    ///
    /// This is only invoked if [`StructInfo::borrows`] is set, otherwise
    /// borrowed atoms are passed to [`atom`](Self::atom).
    fn borrowed_atom(
        &mut self,
        collect: Collect,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error>;
}

/// The value of a field of type `T` that is deserialized with the adapter
/// `A` (see [`FieldSlot`]).
///
/// This is unrelated to the public [`Slot`](crate::de::Slot), the slot of a
/// value that is deserialized from an atom.
pub struct FieldValue<T, A = Same> {
    value: Option<T>,
    _adapter: PhantomData<fn() -> A>,
}

impl<T, A> FieldValue<T, A> {
    /// Creates the slot with an initial value.
    #[inline(always)]
    pub fn new(value: Option<T>) -> FieldValue<T, A> {
        FieldValue {
            value,
            _adapter: PhantomData,
        }
    }

    /// Returns `true` if the field has no value.
    #[inline(always)]
    pub fn is_none(&self) -> bool {
        self.value.is_none()
    }

    /// Sets the value of the field.
    #[inline(always)]
    pub fn set(&mut self, value: Option<T>) {
        self.value = value;
    }

    /// Takes the value of the field.
    #[inline(always)]
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }
}

/// The slot of a struct without fields, it's never used.
struct NoField;

impl<'de> FieldSlot<'de> for NoField {
    fn collects(&self) -> bool {
        false
    }

    fn raw(&self) -> Option<&'static crate::ext::RawFormatInfo> {
        None
    }

    fn sink(&mut self, _collect: Collect, _state: &mut State) -> SinkHandle<'_, 'de> {
        SinkHandle::null()
    }

    fn atom(&mut self, _collect: Collect, _atom: Atom, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn borrowed_atom(
        &mut self,
        _collect: Collect,
        _atom: Atom<'de>,
        _state: &mut State,
    ) -> Result<(), Error> {
        Ok(())
    }
}

/// Returns the slot of a field of a struct without fields.
///
/// The fields of a struct are only looked up by the index of a field, so
/// this is never used.
pub fn no_field_slot<'x, 'de>() -> &'x mut dyn FieldSlot<'de> {
    // boxes of zero sized types do not allocate
    Box::leak(Box::new(NoField))
}

impl<'de, T: Send, A: Deserialize<'de, T>> FieldSlot<'de> for FieldValue<T, A> {
    fn collects(&self) -> bool {
        A::__private_collects()
    }

    fn raw(&self) -> Option<&'static crate::ext::RawFormatInfo> {
        A::__private_raw()
    }

    fn sink(&mut self, collect: Collect, state: &mut State) -> SinkHandle<'_, 'de> {
        // `collect` is only set for fields that collect, the check of the
        // type removes the branch for the others
        if A::__private_collects() && collect != Collect::No {
            A::__private_collect_into(&mut self.value, state)
        } else {
            A::deserialize_into(&mut self.value, state)
        }
    }

    fn atom(&mut self, collect: Collect, atom: Atom, state: &mut State) -> Result<(), Error> {
        if A::__private_collects() && collect != Collect::No {
            atom_into_handle(
                A::__private_collect_into(&mut self.value, state),
                atom,
                state,
            )
        } else {
            A::__private_atom_into(&mut self.value, atom, state)
        }
    }

    fn borrowed_atom(
        &mut self,
        collect: Collect,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        if A::__private_collects() && collect != Collect::No {
            borrowed_atom_into_handle(
                A::__private_collect_into(&mut self.value, state),
                atom,
                state,
            )
        } else {
            A::__private_borrowed_atom_into(&mut self.value, atom, state)
        }
    }
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
            return missing_field(missing, self.fields, state);
        }
        for (index, (missing, name)) in missing.iter().zip(self.fields).enumerate() {
            if *missing && !self.seen.contains(index) {
                self.errors
                    .push(new_missing_field_error(name, state), state);
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
    // the fields are in the same block as the sink (see `ArenaStruct`) which
    // drops them
    fields: NonNull<dyn StructFields<'de> + 'a>,
    key: FieldKeySink,
    seen: Seen,
    errors: CollectedErrors,
    info: &'static StructInfo,
    // the fields that want raw values as bits by index (see
    // `StructFields::raw_fields`)
    raw: u64,
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
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::from_arena_struct(ArenaStruct::new(
            fields,
            info,
            F::raw_fields(),
            &mut state.arena,
        ))
    }

    /// Creates the sink for fields, see [`ArenaStruct`].
    #[inline(always)]
    pub(crate) fn new(
        fields: NonNull<dyn StructFields<'de> + 'a>,
        info: &'static StructInfo,
        raw: u64,
    ) -> StructSink<'a, 'de> {
        StructSink {
            fields,
            key: FieldKeySink::new(info.lookup, |_| false, info.deny),
            seen: Seen {
                small: 0,
                large: None,
            },
            errors: CollectedErrors::new(),
            info,
            raw,
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

    /// Requests the value of the key as raw value if its field wants one.
    #[inline(never)]
    fn request_raw(&mut self, state: &mut State) -> Result<(), Error> {
        let index = self.key.index;
        // the last bit stands for all fields from the 64th on
        if self.raw & (1 << index.min(63)) != 0
            && index < self.info.fields.len()
            && let Some(format) = self.fields().field(index).raw()
        {
            return state.__private_request_raw(format);
        }
        Ok(())
    }

    /// Returns how this occurrence of a field is deserialized.
    fn collect(&mut self, index: usize, state: &State) -> Collect {
        let collects = state.is_multimap() && self.fields().field(index).collects();
        match (collects, core::mem::replace(&mut self.key.repeated, false)) {
            (false, _) => Collect::No,
            (true, false) => Collect::First,
            (true, true) => Collect::Next,
        }
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
        if !seen_before || (state.is_multimap() && self.fields().field(index).collects()) {
            if seen_before {
                self.key.repeated = true;
            }
            Ok(Some(index))
        } else if duplicate_field(self.info.fields[index], state)? {
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
            Some(index) => {
                let collect = self.collect(index, state);
                self.fields().field(index).sink(collect, state)
            }
            None => SinkHandle::null(),
        })
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.key.key_atom(atom, self.key.lookup, state)?;
        if self.raw != 0 {
            return self.request_raw(state);
        }
        Ok(())
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.next_index(state)? {
            Some(index) => {
                let collect = self.collect(index, state);
                self.fields().field(index).atom(collect, atom, state)
            }
            None => Ok(()),
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        // keys are only matched, they do not need to be borrowed
        self.key.key_atom(atom, self.key.lookup, state)?;
        if self.raw != 0 {
            return self.request_raw(state);
        }
        Ok(())
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match self.next_index(state)? {
            Some(index) if self.info.borrows => {
                let collect = self.collect(index, state);
                self.fields()
                    .field(index)
                    .borrowed_atom(collect, atom, state)
            }
            Some(index) => {
                let collect = self.collect(index, state);
                self.fields().field(index).atom(collect, atom, state)
            }
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
/// [`Deserialize::deserialize_update`]).
///
/// The derive implements this for structs, [`StructUpdateSink`] does
/// everything else, it exists once for all structs.
pub trait UpdateFields<'de>: Send {
    /// Returns the sink that updates the field with the index.
    ///
    /// `collect` says if the value is collected (see [`Collect`]).
    fn update_field(
        &mut self,
        index: usize,
        collect: Collect,
        state: &mut State,
    ) -> SinkHandle<'_, 'de>;

    /// Returns whether the field collects repeated values.
    fn collects(&self, index: usize) -> bool;
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
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::arena(
            StructUpdateSink {
                value,
                key: FieldKeySink::new(info.lookup, |_| false, info.deny),
                seen: vec![0; info.fields.len().div_ceil(64)],
                info,
            },
            state,
        )
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
        let index = core::mem::replace(&mut self.key.index, UNKNOWN);
        if index == UNKNOWN {
            if self.key.other.is_some() {
                self.key
                    .unknown_key(self.info.fields, self.info.deny, state)?;
            }
            return Ok(SinkHandle::null());
        }
        let repeated = mark_seen(&mut self.seen, index);
        let collects = state.is_multimap() && self.value.collects(index);
        if repeated && !collects && !duplicate_field(self.info.fields[index], state)? {
            return Ok(SinkHandle::null());
        }
        let collect = if !collects {
            Collect::No
        } else if repeated {
            Collect::Next
        } else {
            Collect::First
        };
        Ok(self.value.update_field(index, collect, state))
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

/// Returns the errors a struct collected together with the fields that
/// are missing.
///
/// `missing` tells for the required fields (with the indexes and names
/// given) if they have no value.  Fields that were seen but have no
/// value failed, they are not missing.
#[cold]
#[inline(never)]
pub fn collected_errors(
    errors: &mut CollectedErrors,
    seen: &[u64],
    missing: &[bool],
    indexes: &[usize],
    names: &[&str],
    state: &State,
) -> Error {
    for ((missing, index), name) in missing.iter().zip(indexes).zip(names) {
        if *missing && !is_seen(seen, *index) {
            errors.push(new_missing_field_error(name, state), state);
        }
    }
    match errors.take() {
        Some(err) => err,
        None => unreachable!(),
    }
}

/// Creates the error for the first missing field.
///
/// If errors are collected, the error holds all missing fields.
#[cold]
pub fn missing_field(missing: &[bool], names: &[&str], state: &State) -> Error {
    if state.collects_errors() {
        let errors = missing
            .iter()
            .zip(names)
            .filter(|(missing, _)| **missing)
            .map(|(_, name)| state.attach_error_context(new_missing_field_error(name, state)));
        if let Some(err) = Error::from_errors(errors) {
            return err;
        }
    }
    let index = missing.iter().position(|x| *x).unwrap_or_default();
    new_missing_field_error(names[index], state)
}

/// Creates the error for a missing field.
#[cold]
pub fn new_missing_field_error(name: &str, state: &State) -> Error {
    if state.discards_errors {
        return discarded_error(ErrorKind::MissingField);
    }
    Error::new(ErrorKind::MissingField, format!("missing field `{}`", name))
}
