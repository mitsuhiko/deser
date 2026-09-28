//! Serialization support for enums with data.
//!
//! These are used by the derive.
use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;

use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ContainerShape};
use crate::ser::begin::Begin;
use crate::ser::flatten::FlattenedStruct;
use crate::ser::{
    Chunk, Describe, MapEmitter, SeqEmitter, Serialize, SerializeHandle, StructEmitter, Variant,
    VariantKind, VariantRepr,
};
use crate::{State, Text};

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
    pub fn into_chunk(self, state: &mut State) -> Chunk<'a> {
        Chunk::map(
            EntryEmitter {
                entry: self,
                index: 0,
            },
            state,
        )
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
    pub fn into_chunk(self, state: &mut State) -> Chunk<'a> {
        Chunk::structure(
            FieldsEmitter {
                fields: self.0,
                index: 0,
            },
            state,
        )
    }
}

impl<'a> Serialize for FieldsSer<'a> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::structure(
            FieldsEmitter {
                fields: self
                    .0
                    .iter()
                    .map(|(name, value)| (*name, SerializeHandle::Borrowed(&**value)))
                    .collect(),
                index: 0,
            },
            state,
        ))
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

/// A field of a [`FlatFieldsSer`].
pub enum FieldSer<'a> {
    /// A field with its name.
    Field(&'static str, SerializeHandle<'a>),
    /// A value whose fields are merged into the struct.
    Flatten(&'a dyn Serialize),
}

/// Serializes a list of fields as a struct, some of which are flattened.
///
/// This is used for struct variants with flattened fields.  The fields of
/// flattened values that are optional are skipped if `skip_optionals` is
/// set (the other fields are skipped by the derive).
pub struct FlatFieldsSer<'a> {
    pub fields: Vec<FieldSer<'a>>,
    pub skip_optionals: bool,
}

impl<'a> FlatFieldsSer<'a> {
    /// Converts the fields into a chunk.
    pub fn into_chunk(self, state: &mut State) -> Chunk<'a> {
        Chunk::structure(
            FlatFieldsEmitter {
                fields: self.fields,
                skip_optionals: self.skip_optionals,
                index: 0,
                nested: None,
            },
            state,
        )
    }
}

impl<'a> Serialize for FlatFieldsSer<'a> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::structure(
            FlatFieldsEmitter {
                fields: self
                    .fields
                    .iter()
                    .map(|field| match *field {
                        FieldSer::Field(name, ref value) => {
                            FieldSer::Field(name, SerializeHandle::Borrowed(&**value))
                        }
                        FieldSer::Flatten(value) => FieldSer::Flatten(value),
                    })
                    .collect(),
                skip_optionals: self.skip_optionals,
                index: 0,
                nested: None,
            },
            state,
        ))
    }
}

struct FlatFieldsEmitter<'a> {
    fields: Vec<FieldSer<'a>>,
    skip_optionals: bool,
    index: usize,
    // the fields of the flattened value at `index`
    nested: Option<FlattenedStruct<'a>>,
}

impl<'a> StructEmitter for FlatFieldsEmitter<'a> {
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        loop {
            if let Some(ref mut nested) = self.nested {
                let item = nested.next(state)?;
                // SAFETY: the item borrows from `self.nested`.  If it's
                // returned, `self.nested` is not touched again in this call,
                // otherwise it's dropped before `self.nested` is replaced.
                // The borrow checker does not understand that the borrow
                // does not continue into the next loop iteration (this can
                // be validated with `-Zpolonius`).
                let item = unsafe {
                    core::mem::transmute::<
                        Option<(Cow<'_, str>, SerializeHandle<'_>)>,
                        Option<(Cow<'a, str>, SerializeHandle<'a>)>,
                    >(item)
                };
                match item {
                    Some((_, ref handle)) if self.skip_optionals && handle.is_optional() => {
                        continue;
                    }
                    Some(item) => return Ok(Some(item)),
                    None => {
                        self.nested = None;
                        if let Some(FieldSer::Flatten(value)) = self.fields.get(self.index) {
                            // the values it forwarded to were finished with
                            // the last field, now the value itself
                            value.finish(state)?;
                        }
                        self.index += 1;
                        continue;
                    }
                }
            }
            match self.fields.get(self.index) {
                None => return Ok(None),
                Some(FieldSer::Field(name, value)) => {
                    self.index += 1;
                    return Ok(Some((
                        Cow::Borrowed(*name),
                        SerializeHandle::Borrowed(&**value),
                    )));
                }
                Some(FieldSer::Flatten(value)) => {
                    self.nested = Some(FlattenedStruct::new(*value, state)?);
                }
            }
        }
    }
}

/// Serializes a list of values as a sequence.
pub struct SeqSer<'a>(pub Vec<SerializeHandle<'a>>);

impl<'a> SeqSer<'a> {
    /// Converts the values into a chunk.
    pub fn into_chunk(self, state: &mut State) -> Chunk<'a> {
        Chunk::seq(
            SeqValuesEmitter {
                values: self.0,
                index: 0,
            },
            state,
        )
    }
}

impl<'a> Serialize for SeqSer<'a> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::seq(
            SeqValuesEmitter {
                values: self
                    .0
                    .iter()
                    .map(|value| SerializeHandle::Borrowed(&**value))
                    .collect(),
                index: 0,
            },
            state,
        ))
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
    pub fn into_chunk(self, state: &mut State) -> Chunk<'a> {
        Chunk::structure(
            TaggedNewtypeEmitter {
                value: self,
                content: None,
                started: false,
                done: false,
            },
            state,
        )
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
        let tag = self.value.tag;
        match self.content.as_mut().unwrap().next(state)? {
            // the tag would be given twice (for instance by an internally
            // tagged enum with the same tag) and most parsers use the last.
            Some((name, _)) if name == tag => Err(Error::new(
                ErrorKind::Unexpected,
                format!(
                    "the content of the variant has a field `{}` like the tag",
                    tag
                ),
            )),
            Some(item) => Ok(Some(item)),
            None => {
                self.done = true;
                self.value.inner.finish(state)?;
                Ok(None)
            }
        }
    }
}

/// Creates the error for a variant that is skipped when serializing.
#[cold]
pub fn skipped_variant(type_name: &str, variant: &str) -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        format!(
            "the variant `{}` of {} cannot be serialized",
            variant, type_name
        ),
    )
}

/// The name of a variant of a unit enum (see [`UnitVariants`]).
pub enum UnitName {
    Str(&'static str),
    U64(u64),
    I64(i64),
    Bool(bool),
    /// The variant cannot be serialized, this is its name in Rust.
    Skipped(&'static str),
}

/// The variants of a unit enum of the derive (an enum with only unit
/// variants).
///
/// The derive generates this as a constant together with a function that
/// returns the index of a variant, everything else exists once for all
/// unit enums.
pub struct UnitVariants {
    /// The name of the enum.
    pub type_name: &'static str,
    /// The names of the variants as they are described.
    pub names: &'static [&'static str],
    /// The names of the variants as they are serialized.
    pub atoms: &'static [UnitName],
}

/// Serializes the variant of a unit enum by index.
#[inline]
pub fn serialize_unit(variants: &UnitVariants, index: usize) -> Result<Chunk<'static>, Error> {
    Ok(Chunk::Atom(match variants.atoms[index] {
        UnitName::Str(name) => Atom::Str(Text::borrowed(name)),
        UnitName::U64(value) => Atom::U64(value),
        UnitName::I64(value) => Atom::I64(value),
        UnitName::Bool(value) => Atom::Bool(value),
        UnitName::Skipped(variant) => return Err(skipped_variant(variants.type_name, variant)),
    }))
}

/// Begins the serialization of the variant of a unit enum by index.
pub fn begin_unit(variants: &UnitVariants, index: usize) -> Result<Begin<'static>, Error> {
    Ok(Begin::chunk(
        serialize_unit(variants, index)?,
        ContainerShape::new(),
        false,
    ))
}

/// Describes the variant of a unit enum by index.
pub fn describe_unit(d: &mut dyn Describe, variants: &UnitVariants, index: usize) {
    d.variant(&Variant::new(
        variants.type_name,
        variants.names[index],
        VariantKind::Unit,
        VariantRepr::External,
    ));
}
