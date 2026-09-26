//! Support for merging the fields of a value into a struct.
//!
//! This is used by the derive for flattened fields and by internally tagged
//! enums for the content of newtype variants.
use std::borrow::Cow;

use crate::State;
use crate::error::{Error, ErrorKind};
use crate::ser::driver::Held;
use crate::ser::{Chunk, Serialize, SerializeHandle, StructEmitter};

/// Holds the values a value forwarded to (see [`Chunk::Forward`]).
///
/// The chunks of values that forward to other values borrow from the
/// forwarded values which is why they are held here.
pub(crate) struct Forwarded(Vec<Held>);

impl Forwarded {
    /// Creates an empty list of forwarded values.
    pub(crate) fn new() -> Forwarded {
        Forwarded(Vec::new())
    }

    /// Serializes the value and follows forwarding chunks.
    ///
    /// Returns the first chunk that does not forward.
    ///
    /// # Safety
    ///
    /// The returned chunk (and everything created from it) can borrow from
    /// the values held here and must be dropped before `self`.
    pub(crate) unsafe fn serialize<'a>(
        &mut self,
        value: &'a dyn Serialize,
        state: &mut State,
    ) -> Result<Chunk<'a>, Error> {
        let mut chunk = value.serialize(state)?;
        loop {
            match chunk {
                Chunk::Forward(handle) => {
                    // SAFETY: the caller guarantees that the chunk which
                    // borrows from the held value is dropped first.
                    let held = unsafe { Held::new(handle) };
                    let value: &'a dyn Serialize = unsafe { held.get() };
                    self.0.push(held);
                    chunk = value.serialize(state)?;
                }
                chunk => return Ok(chunk),
            }
        }
    }

    /// Invokes [`finish`](Serialize::finish) on the forwarded values.
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

/// The fields of a value that is flattened into a struct.
///
/// The value has to serialize as a struct.  If it forwards to another value
/// (as values serialized with [`FromInto`](crate::adapters::FromInto) do)
/// the fields of the value it forwards to are used.  After the last field
/// the values it forwarded to are finished, the flattened value itself has
/// to be finished by the caller.
pub struct FlattenedStruct<'a> {
    // `emitter` must be declared (and thus dropped) before `forwarded` as it
    // can borrow from the forwarded values.
    emitter: Box<dyn StructEmitter + 'a>,
    forwarded: Forwarded,
    done: bool,
}

impl<'a> FlattenedStruct<'a> {
    /// Serializes the value that is flattened.
    pub fn new(value: &'a dyn Serialize, state: &mut State) -> Result<FlattenedStruct<'a>, Error> {
        let mut forwarded = Forwarded::new();
        // SAFETY: the chunk is declared after `forwarded` and dropped before
        // it, the emitter is moved into a struct which drops it first.
        let chunk = unsafe { forwarded.serialize(value, state)? };
        match chunk {
            Chunk::Struct(emitter) => Ok(FlattenedStruct {
                emitter,
                forwarded,
                done: false,
            }),
            _ => Err(Error::new(
                ErrorKind::Unexpected,
                "unable to flatten on struct into struct",
            )),
        }
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
        match self.emitter.next(state)? {
            Some(item) => Ok(Some(item)),
            None => {
                self.done = true;
                self.forwarded.finish(state)?;
                Ok(None)
            }
        }
    }
}
