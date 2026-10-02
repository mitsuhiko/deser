//! Support for merging the fields of a value into a struct.
//!
//! This is used by the derive for flattened fields and by internally tagged
//! enums for the content of newtype variants.
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::State;
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ser::driver::Held;
use crate::ser::{Boxed, Emit, MapEmitter, SerializeHandle, SerializeRef, StructEmitter};

/// Holds the values a value forwarded to (see [`Emit::Forward`]).
///
/// The `Emit`s of values that forward to other values borrow from the
/// forwarded values which is why they are held here.
pub(crate) struct Forwarded(Vec<Held>);

impl Forwarded {
    /// Creates an empty list of forwarded values.
    pub(crate) fn new() -> Forwarded {
        Forwarded(Vec::new())
    }

    /// Serializes the value and follows forwarding `Emit`s.
    ///
    /// Returns the first `Emit` that does not forward.
    ///
    /// # Safety
    ///
    /// The returned `Emit` (and everything created from it) can borrow from
    /// the values held here and must be dropped before `self`.
    pub(crate) unsafe fn serialize<'a>(
        &mut self,
        value: SerializeRef<'a>,
        state: &mut State,
    ) -> Result<Emit<'a>, Error> {
        let mut emit = value.serialize(state)?;
        loop {
            match emit {
                Emit::Forward(handle) => {
                    // SAFETY: the caller guarantees that the `Emit` which
                    // borrows from the held value is dropped first.
                    let held = unsafe { Held::new(handle) };
                    let value: SerializeRef<'a> = unsafe { held.get() };
                    self.0.push(held);
                    emit = value.serialize(state)?;
                }
                emit => return Ok(emit),
            }
        }
    }

    /// Invokes [`finish`](crate::ser::Serialize::finish) on the forwarded values.
    ///
    /// The values are finished in the inverse order, the innermost first.
    /// The value that forwarded is not finished.
    pub(crate) fn finish(&self, state: &mut State) -> Result<(), Error> {
        for held in self.0.iter().rev() {
            // SAFETY: the value is held by `self`
            unsafe { held.get() }.finish(state)?;
        }
        Ok(())
    }
}

/// The emitter of a value that is flattened.
enum Content<'a> {
    Struct(Boxed<dyn StructEmitter + 'a>),
    Map(Boxed<dyn MapEmitter + 'a>),
    // null (like `None`) has no fields
    Empty,
}

/// The fields of a value that is flattened into a struct.
///
/// The value has to serialize as a struct or as a map (whose keys become
/// field names, they have to be strings, integers, booleans or chars).  Null
/// (like `None`) has no fields.  If
/// it forwards to another value (as values serialized with
/// [`FromInto`](crate::adapters::FromInto) do) the fields of the value it
/// forwards to are used.  After the last field the values it forwarded to
/// are finished, the flattened value itself has to be finished by the
/// caller.
pub struct FlattenedStruct<'a> {
    // `content` must be declared (and thus dropped) before `forwarded` as it
    // can borrow from the forwarded values.
    content: Content<'a>,
    forwarded: Forwarded,
    done: bool,
}

impl<'a> FlattenedStruct<'a> {
    /// Serializes the value that is flattened.
    pub fn new(value: SerializeRef<'a>, state: &mut State) -> Result<FlattenedStruct<'a>, Error> {
        let mut forwarded = Forwarded::new();
        // SAFETY: the `Emit` is declared after `forwarded` and dropped before
        // it, the emitter is moved into a struct which drops it first.
        let emit = unsafe { forwarded.serialize(value, state)? };
        let content = match emit {
            Emit::Struct(emitter) => Content::Struct(emitter),
            Emit::Map(emitter) => Content::Map(emitter),
            Emit::Atom(Atom::Null) => Content::Empty,
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "only structs and maps can be flattened",
                ));
            }
        };
        Ok(FlattenedStruct {
            content,
            forwarded,
            done: false,
        })
    }

    /// Returns `true` if the value was null.
    pub(crate) fn is_null(&self) -> bool {
        matches!(self.content, Content::Empty)
    }

    /// Produces the next field.
    ///
    /// After the last field the values the flattened value forwarded to are
    /// finished.
    #[inline]
    pub fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        if self.done {
            return Ok(None);
        }
        match self.content {
            Content::Struct(ref mut emitter) => {
                if let Some(item) = emitter.next(state)? {
                    return Ok(Some(item));
                }
            }
            Content::Map(ref mut emitter) => {
                let key = match emitter.next_key(state)? {
                    Some(key) => Some(map_key_string(key.get(), state)?),
                    None => None,
                };
                if let Some(key) = key {
                    return Ok(Some((Cow::Owned(key), emitter.next_value(state)?)));
                }
            }
            Content::Empty => {}
        }
        self.done = true;
        self.forwarded.finish(state)?;
        Ok(None)
    }
}

/// Returns the field name for the key of a flattened map.
fn map_key_string(key: SerializeRef<'_>, state: &mut State) -> Result<String, Error> {
    let rv = match key.serialize(state)? {
        Emit::Atom(Atom::Str(key) | Atom::Lexical(key)) => key.into_owned(),
        Emit::Atom(Atom::U64(value)) => value.to_string(),
        Emit::Atom(Atom::I64(value)) => value.to_string(),
        Emit::Atom(Atom::Bool(value)) => value.to_string(),
        Emit::Atom(Atom::Char(value)) => value.to_string(),
        _ => {
            return Err(Error::new(
                ErrorKind::UnsupportedType,
                "the keys of flattened maps must be strings, integers, booleans or chars",
            ));
        }
    };
    key.finish(state)?;
    Ok(rv)
}
