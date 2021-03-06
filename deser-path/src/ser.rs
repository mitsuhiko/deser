use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deser::ser::{
    Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle, SerializerState, StructEmitter,
};
use deser::{Atom, Error};

use crate::{Path, PathSegment};

/// Wraps a serializable so that it tracks the current path.
pub struct PathSerializable<'a> {
    serializable: SerializeHandle<'a>,
}

impl<'a> PathSerializable<'a> {
    /// Wraps another serializable.
    pub fn wrap(serializable: &'a dyn Serialize) -> PathSerializable<'a> {
        PathSerializable::wrap_ref(SerializeHandle::Borrowed(serializable))
    }

    /// Wraps another serializable ref.
    pub fn wrap_ref(serializable: SerializeHandle<'a>) -> PathSerializable<'a> {
        PathSerializable { serializable }
    }
}

impl<'a> Serialize for PathSerializable<'a> {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        match self.serializable.serialize(state)? {
            Chunk::Struct(emitter) => Ok(Chunk::Struct(Box::new(PathStructEmitter { emitter }))),
            Chunk::Map(emitter) => Ok(Chunk::Map(Box::new(PathMapEmitter {
                emitter,
                path_segment: Rc::default(),
            }))),
            Chunk::Seq(emitter) => Ok(Chunk::Seq(Box::new(PathSeqEmitter { emitter, index: 0 }))),
            other => Ok(other),
        }
    }

    fn finish(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.finish(state)
    }
}

struct PathStructEmitter<'a> {
    emitter: Box<dyn StructEmitter + 'a>,
}

impl<'a> StructEmitter for PathStructEmitter<'a> {
    fn next(
        &mut self,
        state: &SerializerState,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle)>, Error> {
        let (key, value) = match self.emitter.next(state)? {
            Some(result) => result,
            None => return Ok(None),
        };
        let new_segment = PathSegment::Key(key.to_string());
        let value_serializable = SegmentPushingSerializable {
            serializable: value,
            segment: RefCell::new(Some(new_segment)),
        };
        Ok(Some((key, SerializeHandle::boxed(value_serializable))))
    }
}

struct PathMapEmitter<'a> {
    emitter: Box<dyn MapEmitter + 'a>,
    path_segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> MapEmitter for PathMapEmitter<'a> {
    fn next_key(&mut self, state: &SerializerState) -> Result<Option<SerializeHandle>, Error> {
        let key_serializable = SegmentCollectingSerializable {
            serializable: match self.emitter.next_key(state)? {
                Some(result) => result,
                None => return Ok(None),
            },
            segment: self.path_segment.clone(),
        };
        Ok(Some(SerializeHandle::boxed(key_serializable)))
    }

    fn next_value(&mut self, state: &SerializerState) -> Result<SerializeHandle, Error> {
        let new_segment = self
            .path_segment
            .borrow_mut()
            .take()
            .unwrap_or(PathSegment::Unknown);
        let value_serializable = SegmentPushingSerializable {
            serializable: self.emitter.next_value(state)?,
            segment: RefCell::new(Some(new_segment)),
        };
        Ok(SerializeHandle::boxed(value_serializable))
    }
}

struct PathSeqEmitter<'a> {
    emitter: Box<dyn SeqEmitter + 'a>,
    index: usize,
}

impl<'a> SeqEmitter for PathSeqEmitter<'a> {
    fn next(&mut self, state: &SerializerState) -> Result<Option<SerializeHandle>, Error> {
        let index = self.index;
        self.index += 1;
        let value = match self.emitter.next(state)? {
            Some(result) => result,
            None => return Ok(None),
        };
        let new_segment = PathSegment::Index(index);
        let item_serializable = SegmentPushingSerializable {
            serializable: value,
            segment: RefCell::new(Some(new_segment)),
        };
        Ok(Some(SerializeHandle::boxed(item_serializable)))
    }
}

struct SegmentPushingSerializable<'a> {
    serializable: SerializeHandle<'a>,
    segment: RefCell<Option<PathSegment>>,
}

impl<'a> Serialize for SegmentPushingSerializable<'a> {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        {
            let mut path = state.get_mut::<Path>();
            path.segments.push(self.segment.take().unwrap());
        }
        match self.serializable.serialize(state)? {
            Chunk::Struct(emitter) => Ok(Chunk::Struct(Box::new(PathStructEmitter { emitter }))),
            Chunk::Map(emitter) => Ok(Chunk::Map(Box::new(PathMapEmitter {
                emitter,
                path_segment: Rc::default(),
            }))),
            Chunk::Seq(emitter) => Ok(Chunk::Seq(Box::new(PathSeqEmitter { emitter, index: 0 }))),
            other => Ok(other),
        }
    }

    fn finish(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.finish(state)?;
        let mut path = state.get_mut::<Path>();
        path.segments.pop();
        Ok(())
    }
}

struct SegmentCollectingSerializable<'a> {
    serializable: SerializeHandle<'a>,
    segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> Serialize for SegmentCollectingSerializable<'a> {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        match self.serializable.serialize(state)? {
            Chunk::Atom(Atom::Str(key)) => {
                *self.segment.borrow_mut() = Some(PathSegment::Key(key.to_string()));
                Ok(Chunk::Atom(Atom::Str(key)))
            }
            Chunk::Atom(Atom::U64(val)) => {
                *self.segment.borrow_mut() = Some(PathSegment::Index(val as usize));
                Ok(Chunk::Atom(Atom::U64(val)))
            }
            other => Ok(other),
        }
    }

    fn finish(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.finish(state)
    }
}
