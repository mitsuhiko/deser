//! The fast path of the serialize driver.
//!
//! These types are not part of the public API.  They are used by the
//! implementations in this crate and by the derive (through
//! `deser::__derive`).
#[cfg(feature = "derive")]
use alloc::borrow::Cow;

use crate::State;
use crate::error::Error;
use crate::event::{Atom, ContainerShape};

/// How much a driver that pauses emits at once with the plain fast paths.
///
/// The budget is counted in atoms and containers, long text and bytes
/// count more (see [`atom_cost`]).  A driver that can pause (see
/// [`SerializeDriver::drive_until`](crate::ser::SerializeDriver::drive_until))
/// only emits plain values at once which fit into it, larger ones are
/// emitted in pieces so that the driver can pause in between.
pub(crate) const PLAIN_BUDGET: usize = 256;

/// Returns what emitting an atom costs (see [`PLAIN_BUDGET`]).
#[inline]
pub(crate) fn atom_cost(atom: &Atom<'_>) -> usize {
    match atom {
        Atom::Str(text) | Atom::Lexical(text) => 1 + text.len() / 32,
        Atom::Bytes(bytes) => 1 + bytes.len() / 32,
        _ => 1,
    }
}
#[cfg(feature = "derive")]
use crate::ser::StructEmitter;
use crate::ser::{Chunk, SeqEmitter, Serialize, SerializeHandle};

/// The result of [`Serialize::__private_begin`](crate::ser::Serialize::__private_begin).
pub struct Begin<'a> {
    pub(crate) kind: BeginKind<'a>,
    pub(crate) shape: ContainerShape,
    pub(crate) needs_finish: bool,
}

pub(crate) enum BeginKind<'a> {
    Chunk(Chunk<'a>),
    Struct(&'a dyn IndexedStruct),
    Seq(&'a dyn IndexedSeq),
    /// A plain value (see [`PlainSink`]), the driver either emits it or
    /// serializes it into a chunk.
    Plain(&'a dyn Serialize),
}

impl<'a> Begin<'a> {
    /// Begins a value with a chunk.
    #[inline]
    pub fn chunk(chunk: Chunk<'a>, shape: ContainerShape, needs_finish: bool) -> Begin<'a> {
        Begin {
            kind: BeginKind::Chunk(chunk),
            shape,
            needs_finish,
        }
    }

    /// Begins a struct which provides its fields by index.
    ///
    /// This is equivalent to a [`Chunk::Struct`] but does not require an
    /// emitter to be allocated.  `finish` is not invoked.
    #[inline]
    pub fn indexed_struct(value: &'a dyn IndexedStruct, shape: ContainerShape) -> Begin<'a> {
        Begin {
            kind: BeginKind::Struct(value),
            shape,
            needs_finish: false,
        }
    }

    /// Begins a plain value (see [`PlainSink`]).
    ///
    /// The driver emits the value with
    /// [`__private_emit_plain`](Serialize::__private_emit_plain) or if it
    /// needs to drive every value on its own, with
    /// [`serialize`](Serialize::serialize).  `finish` is not invoked.
    #[inline]
    pub fn plain(value: &'a dyn Serialize, shape: ContainerShape) -> Begin<'a> {
        Begin {
            kind: BeginKind::Plain(value),
            shape,
            needs_finish: false,
        }
    }

    /// Begins a sequence which provides its elements by index.
    ///
    /// This is equivalent to a [`Chunk::Seq`] but does not require an
    /// emitter to be allocated.  `finish` is not invoked.
    #[inline]
    pub fn indexed_seq(value: &'a dyn IndexedSeq, shape: ContainerShape) -> Begin<'a> {
        Begin {
            kind: BeginKind::Seq(value),
            shape,
            needs_finish: false,
        }
    }
}

/// A field of an [`IndexedStruct`].
pub enum StructField<'a> {
    /// A field with key and value.
    Field(&'a str, SerializeHandle<'a>),
    /// The field is skipped.
    Skip,
    /// There are no more fields.
    End,
}

/// A struct which provides its fields by index.
///
/// The fields are requested with increasing indexes starting at zero until
/// [`StructField::End`] is returned.
pub trait IndexedStruct: Sync {
    fn field(&self, index: usize) -> StructField<'_>;

    /// Emits the fields from `index` on as long as their values are plain
    /// (see [`PlainSink`]).
    ///
    /// Returns the index of the first field that was not emitted or
    /// [`FIELDS_END`] if all fields were emitted.  Skipped fields count as
    /// emitted.  If `bounded` is set, only values that fit into the budget
    /// are emitted (see `PLAIN_BUDGET`).  See [`emit_plain_field`].
    #[inline]
    fn emit_plain_fields(
        &self,
        index: usize,
        bounded: bool,
        sink: &mut dyn PlainSink,
    ) -> Result<usize, Error> {
        let _ = (bounded, sink);
        Ok(index)
    }
}

/// Emits a field of a struct if its value is plain.
///
/// Returns `false` without emitting anything if the value is not plain.
/// If `bounded` is set, values which do not fit into the budget are not
/// emitted either (see `PLAIN_BUDGET`).  Derived structs implement
/// [`emit_plain_fields`](IndexedStruct::emit_plain_fields) with this, it
/// exists once per type of field rather than once per field.
#[cfg(feature = "derive")]
#[inline]
pub fn emit_plain_field<T: Serialize>(
    value: &T,
    name: &str,
    sink: &mut dyn PlainSink,
    bounded: bool,
) -> Result<bool, Error> {
    if !value.__private_is_plain_value() {
        return Ok(false);
    }
    // large values are emitted in pieces
    if bounded && value.__private_plain_cost(PLAIN_BUDGET).is_none() {
        return Ok(false);
    }
    sink.field(name)?;
    value.__private_emit_plain(sink)?;
    Ok(true)
}

/// A struct emitter for an [`IndexedStruct`].
#[cfg(feature = "derive")]
pub struct IndexedStructEmitter<'a> {
    fields: &'a dyn IndexedStruct,
    index: usize,
}

#[cfg(feature = "derive")]
impl<'a> IndexedStructEmitter<'a> {
    pub fn new(fields: &'a dyn IndexedStruct) -> IndexedStructEmitter<'a> {
        IndexedStructEmitter { fields, index: 0 }
    }
}

#[cfg(feature = "derive")]
impl<'a> StructEmitter for IndexedStructEmitter<'a> {
    fn next(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        loop {
            let field = self.fields.field(self.index);
            self.index += 1;
            match field {
                StructField::Field(key, value) => return Ok(Some((Cow::Borrowed(key), value))),
                StructField::Skip => continue,
                StructField::End => return Ok(None),
            }
        }
    }
}

/// A sequence which provides its elements by index.
///
/// The elements are requested with increasing indexes starting at zero
/// until `None` is returned.
pub trait IndexedSeq: Sync {
    fn element(
        &self,
        index: usize,
        state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error>;

    /// Emits all elements if they are plain (see [`PlainSink`]).
    ///
    /// Returns `false` without emitting anything if they are not.
    #[inline]
    fn emit_plain(&self, sink: &mut dyn PlainSink) -> Result<bool, Error> {
        let _ = sink;
        Ok(false)
    }

    /// Emits the elements from `index` on as long as they are plain and
    /// fit into the budget (see `PLAIN_BUDGET`).
    ///
    /// Returns the index of the first element that was not emitted.  This
    /// is used by drivers that can pause to emit large sequences in
    /// pieces, the elements that are not emitted are driven on their own.
    #[inline]
    fn emit_plain_chunk(
        &self,
        index: usize,
        budget: usize,
        sink: &mut dyn PlainSink,
    ) -> Result<usize, Error> {
        let _ = (budget, sink);
        Ok(index)
    }
}

/// A sequence emitter for an [`IndexedSeq`].
pub struct IndexedSeqEmitter<'a> {
    seq: &'a dyn IndexedSeq,
    index: usize,
}

impl<'a> IndexedSeqEmitter<'a> {
    pub fn new(seq: &'a dyn IndexedSeq) -> IndexedSeqEmitter<'a> {
        IndexedSeqEmitter { seq, index: 0 }
    }
}

impl<'a> SeqEmitter for IndexedSeqEmitter<'a> {
    fn next(&mut self, state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        let index = self.index;
        self.index += 1;
        self.seq.element(index, state)
    }
}

/// Returned by [`IndexedStruct::emit_plain_fields`] if all fields were
/// emitted.
pub const FIELDS_END: usize = usize::MAX;

/// Receives the events of plain values.
///
/// Plain values are atoms and sequences of plain values which do not use
/// the state (they do not read it, attach event data or add error
/// context) and do not need [`finish`](crate::ser::Serialize::finish).
/// The events they produce do not depend on anything but the value, which
/// allows the driver to hand out their events directly instead of driving
/// every value on its own.  See
/// [`Serialize::__private_is_plain`](crate::ser::Serialize::__private_is_plain).
pub trait PlainSink {
    fn atom(&mut self, atom: Atom<'_>) -> Result<(), Error>;
    fn seq_start(&mut self, shape: ContainerShape) -> Result<(), Error>;
    fn seq_end(&mut self) -> Result<(), Error>;
    fn map_start(&mut self, shape: ContainerShape) -> Result<(), Error>;
    fn map_end(&mut self) -> Result<(), Error>;
    /// Marks the next value as map key.
    fn key(&mut self);
    /// Emits the key of a struct field.
    fn field(&mut self, name: &str) -> Result<(), Error>;
}

/// Implements the plain methods of `Serialize` for a value that serializes
/// as a single atom.
macro_rules! plain_atom {
    (|$this:ident| $atom:expr) => {
        #[inline]
        fn __private_is_plain() -> bool
        where
            Self: Sized,
        {
            true
        }

        #[inline]
        fn __private_emit_plain(
            &self,
            sink: &mut dyn crate::ser::PlainSink,
        ) -> Result<(), crate::Error> {
            let $this = self;
            sink.atom($atom)
        }

        #[inline]
        fn __private_plain_cost(&self, budget: usize) -> Option<usize> {
            let $this = self;
            budget.checked_sub(crate::ser::atom_cost(&$atom))
        }
    };
}

pub(crate) use plain_atom;
