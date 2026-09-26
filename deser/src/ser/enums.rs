//! Serialization support for enums with data.
//!
//! These are used by the derive.
use std::borrow::Cow;

use crate::State;
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::flatten::Forwarded;
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
            emitter: None,
            forwarded: Forwarded::new(),
            started: false,
            done: false,
        }))
    }
}

/// The content of a newtype variant of an internally tagged enum.
enum TaggedContent<'a> {
    Struct(Box<dyn StructEmitter + 'a>),
    Map(Box<dyn MapEmitter + 'a>),
}

struct TaggedNewtypeEmitter<'a> {
    value: TaggedNewtype<'a>,
    // `emitter` must be declared (and thus dropped) before `forwarded` as it
    // can borrow from the forwarded values.
    emitter: Option<TaggedContent<'a>>,
    // values the inner value forwarded to (see `Chunk::Forward`)
    forwarded: Forwarded,
    started: bool,
    done: bool,
}

/// Returns the string of a map key.
fn map_key_string(key: &dyn Serialize, state: &mut State) -> Result<String, Error> {
    let rv = match key.serialize(state)? {
        Chunk::Atom(Atom::Str(key) | Atom::Lexical(key)) => key.into_owned(),
        _ => {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "newtype variants of internally tagged enums must contain maps with string keys",
            ));
        }
    };
    key.finish(state)?;
    Ok(rv)
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
        if self.emitter.is_none() {
            // SAFETY: the emitter is dropped before the forwarded values.
            let chunk = unsafe { self.forwarded.serialize(self.value.inner, state)? };
            self.emitter = Some(match chunk {
                Chunk::Struct(emitter) => TaggedContent::Struct(emitter),
                Chunk::Map(emitter) => TaggedContent::Map(emitter),
                _ => {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "newtype variants of internally tagged enums must contain structs or maps",
                    ));
                }
            });
        }
        let item = match self.emitter.as_mut().unwrap() {
            TaggedContent::Struct(emitter) => emitter.next(state)?,
            // map keys are converted into strings so that they can be
            // emitted as struct fields.
            TaggedContent::Map(emitter) => {
                let key = match emitter.next_key(state)? {
                    Some(key) => Some(map_key_string(&*key, state)?),
                    None => None,
                };
                match key {
                    Some(key) => Some((Cow::Owned(key), emitter.next_value(state)?)),
                    None => None,
                }
            }
        };
        match item {
            Some(item) => Ok(Some(item)),
            None => {
                self.done = true;
                self.forwarded.finish(state)?;
                self.value.inner.finish(state)?;
                Ok(None)
            }
        }
    }
}
