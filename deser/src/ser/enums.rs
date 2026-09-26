//! Serialization support for enums with data.
//!
//! These are used by the derive.
use std::borrow::Cow;

use crate::State;
use crate::error::{Error, ErrorKind};
use crate::ser::flatten::FlattenedStruct;
use crate::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle, StructEmitter};

/// Serializes a map with a single entry.
///
/// This is used for externally tagged variants with a tag that is not a
/// static string.
pub struct EntrySer<'a> {
    key: SerializeHandle<'a>,
    value: SerializeHandle<'a>,
}

impl<'a> EntrySer<'a> {
    /// Creates a new entry.
    pub fn new(key: SerializeHandle<'a>, value: SerializeHandle<'a>) -> EntrySer<'a> {
        EntrySer { key, value }
    }

    /// Converts the entry into a chunk.
    pub fn into_chunk(self) -> Chunk<'a> {
        Chunk::Map(Box::new(EntryEmitter {
            entry: self,
            index: 0,
        }))
    }
}

struct EntryEmitter<'a> {
    entry: EntrySer<'a>,
    index: usize,
}

impl<'a> MapEmitter for EntryEmitter<'a> {
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        let index = self.index;
        self.index += 1;
        Ok(if index == 0 {
            Some(SerializeHandle::Borrowed(&*self.entry.key))
        } else {
            None
        })
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        Ok(SerializeHandle::Borrowed(&*self.entry.value))
    }
}

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
    name: SerializeHandle<'a>,
    inner: &'a dyn Serialize,
}

impl<'a> TaggedNewtype<'a> {
    /// Creates a new tagged newtype.
    ///
    /// `name` is the value of the tag.
    pub fn new(tag: &'static str, name: SerializeHandle<'a>, inner: &'a dyn Serialize) -> Self {
        TaggedNewtype { tag, name, inner }
    }

    /// Converts the value into a chunk.
    pub fn into_chunk(self) -> Chunk<'a> {
        Chunk::Struct(Box::new(TaggedNewtypeEmitter {
            value: self,
            content: None,
            started: false,
            done: false,
        }))
    }
}

struct TaggedNewtypeEmitter<'a> {
    value: TaggedNewtype<'a>,
    // the fields of the inner value, once the tag was emitted
    content: Option<FlattenedStruct<'a>>,
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
                SerializeHandle::Borrowed(&*self.value.name),
            )));
        }
        if self.done {
            return Ok(None);
        }
        if self.content.is_none() {
            let content = FlattenedStruct::new(self.value.inner, state)?;
            // the tag alone would be deserialized as the content (not as
            // null), `()` is handled by the derive.
            if content.is_null() {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "newtype variants of internally tagged enums must contain structs or maps",
                ));
            }
            self.content = Some(content);
        }
        match self.content.as_mut().unwrap().next(state)? {
            Some(item) => Ok(Some(item)),
            None => {
                self.done = true;
                self.value.inner.finish(state)?;
                Ok(None)
            }
        }
    }
}
