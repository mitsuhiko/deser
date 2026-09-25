//! Serialization support for enums with data.
//!
//! These are used by the derive.
use std::borrow::Cow;

use crate::error::{Error, ErrorKind};
use crate::ser::{Chunk, SeqEmitter, Serialize, SerializeHandle, StructEmitter};
use crate::State;

/// Serializes a list of named fields as a struct.
pub struct FieldsSer<'a>(pub Vec<(&'static str, SerializeHandle<'a>)>);

impl<'a> FieldsSer<'a> {
    /// Converts the fields into a chunk.
    pub fn into_chunk(self) -> Chunk<'a> {
        Chunk::Struct(Box::new(FieldsEmitter {
            fields: self.0,
            index: 0,
        }))
    }
}

impl<'a> Serialize for FieldsSer<'a> {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Struct(Box::new(FieldsEmitter {
            fields: self
                .0
                .iter()
                .map(|(name, value)| (*name, SerializeHandle::Borrowed(&**value)))
                .collect(),
            index: 0,
        })))
    }
}

struct FieldsEmitter<'a> {
    fields: Vec<(&'static str, SerializeHandle<'a>)>,
    index: usize,
}

impl<'a> StructEmitter for FieldsEmitter<'a> {
    fn next(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        let index = self.index;
        self.index += 1;
        Ok(self
            .fields
            .get(index)
            .map(|(name, value)| (Cow::Borrowed(*name), SerializeHandle::Borrowed(&**value))))
    }
}

/// Serializes a list of values as a sequence.
pub struct SeqSer<'a>(pub Vec<SerializeHandle<'a>>);

impl<'a> SeqSer<'a> {
    /// Converts the values into a chunk.
    pub fn into_chunk(self) -> Chunk<'a> {
        Chunk::Seq(Box::new(SeqValuesEmitter {
            values: self.0,
            index: 0,
        }))
    }
}

impl<'a> Serialize for SeqSer<'a> {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Seq(Box::new(SeqValuesEmitter {
            values: self
                .0
                .iter()
                .map(|value| SerializeHandle::Borrowed(&**value))
                .collect(),
            index: 0,
        })))
    }
}

struct SeqValuesEmitter<'a> {
    values: Vec<SerializeHandle<'a>>,
    index: usize,
}

impl<'a> SeqEmitter for SeqValuesEmitter<'a> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        let index = self.index;
        self.index += 1;
        Ok(self
            .values
            .get(index)
            .map(|value| SerializeHandle::Borrowed(&**value)))
    }
}

/// Serializes a newtype variant of an internally tagged enum.
///
/// The tag is emitted first, followed by the fields of the inner value which
/// has to serialize as a struct.
pub struct TaggedNewtype<'a> {
    tag: &'static str,
    name: &'static str,
    inner: &'a dyn Serialize,
}

impl<'a> TaggedNewtype<'a> {
    /// Creates a new tagged newtype.
    pub fn new(tag: &'static str, name: &'static str, inner: &'a dyn Serialize) -> Self {
        TaggedNewtype { tag, name, inner }
    }

    /// Converts the value into a chunk.
    pub fn into_chunk(self) -> Chunk<'a> {
        Chunk::Struct(Box::new(TaggedNewtypeEmitter {
            value: self,
            emitter: None,
            started: false,
            done: false,
        }))
    }
}

struct TaggedNewtypeEmitter<'a> {
    value: TaggedNewtype<'a>,
    emitter: Option<Box<dyn StructEmitter + 'a>>,
    started: bool,
    done: bool,
}

impl<'a> StructEmitter for TaggedNewtypeEmitter<'a> {
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        if !self.started {
            self.started = true;
            return Ok(Some((
                Cow::Borrowed(self.value.tag),
                SerializeHandle::to(&self.value.name),
            )));
        }
        if self.done {
            return Ok(None);
        }
        if self.emitter.is_none() {
            match self.value.inner.serialize(state)? {
                Chunk::Struct(emitter) => self.emitter = Some(emitter),
                _ => {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "newtype variants of internally tagged enums must contain structs",
                    ))
                }
            }
        }
        match self.emitter.as_mut().unwrap().next(state)? {
            Some(item) => Ok(Some(item)),
            None => {
                self.done = true;
                self.value.inner.finish(state)?;
                Ok(None)
            }
        }
    }
}
