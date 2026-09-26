//! The fast path of the serialize driver.
//!
//! These types are not part of the public API.  They are used by the
//! implementations in this crate and by the derive (through
//! `deser::__derive`).
#[cfg(feature = "derive")]
use std::borrow::Cow;

use crate::State;
use crate::error::Error;
use crate::event::ContainerShape;
#[cfg(feature = "derive")]
use crate::ser::StructEmitter;
use crate::ser::{Chunk, SerializeHandle};

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
    fn field(&self, index: usize, state: &mut State) -> Result<StructField<'_>, Error>;
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
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        loop {
            let field = self.fields.field(self.index, state)?;
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
}
