use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deser::ser::{
    Chunk, MapEmitter, SeqEmitter, Serializable, SerializableRef, SerializerState, StructEmitter,
};
use deser::Error;

use crate::{Path, PathSegment};

/// Wraps a serializable so that it tracks the current path.
pub struct PathSerializable<'a> {
    serializable: SerializableRef<'a>,
}

impl<'a> PathSerializable<'a> {
    /// Wraps another serializable.
    pub fn wrap(serializable: &'a dyn Serializable) -> PathSerializable<'a> {
        PathSerializable::wrap_ref(SerializableRef::Borrowed(serializable))
    }

    /// Wraps another serializable ref.
    pub fn wrap_ref(serializable: SerializableRef<'a>) -> PathSerializable<'a> {
        PathSerializable { serializable }
    }
}

impl<'a> Serializable for PathSerializable<'a> {
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

    fn done(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.done(state)
    }
}

struct PathStructEmitter<'a> {
    emitter: Box<dyn StructEmitter + 'a>,
}

impl<'a> StructEmitter for PathStructEmitter<'a> {
    fn next(&mut self) -> Option<(Cow<'_, str>, SerializableRef)> {
        let (key, value) = self.emitter.next()?;
        let new_segment = PathSegment::Key(key.to_string());
        let value_serializable = SegmentPushingSerializable {
            serializable: value,
            segment: RefCell::new(Some(new_segment)),
        };
        Some((key, SerializableRef::Owned(Box::new(value_serializable))))
    }
}

struct PathMapEmitter<'a> {
    emitter: Box<dyn MapEmitter + 'a>,
    path_segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> MapEmitter for PathMapEmitter<'a> {
    fn next_key(&mut self) -> Option<SerializableRef> {
        let key_serializable = SegmentCollectingSerializable {
            serializable: self.emitter.next_key()?,
            segment: self.path_segment.clone(),
        };
        Some(SerializableRef::Owned(Box::new(key_serializable)))
    }

    fn next_value(&mut self) -> SerializableRef {
        let new_segment = self
            .path_segment
            .borrow_mut()
            .take()
            .unwrap_or(PathSegment::Unknown);
        let value_serializable = SegmentPushingSerializable {
            serializable: self.emitter.next_value(),
            segment: RefCell::new(Some(new_segment)),
        };
        SerializableRef::Owned(Box::new(value_serializable))
    }
}

struct PathSeqEmitter<'a> {
    emitter: Box<dyn SeqEmitter + 'a>,
    index: usize,
}

impl<'a> SeqEmitter for PathSeqEmitter<'a> {
    fn next(&mut self) -> Option<SerializableRef> {
        let index = self.index;
        self.index += 1;
        let value = self.emitter.next()?;
        let new_segment = PathSegment::Index(index);
        let item_serializable = SegmentPushingSerializable {
            serializable: value,
            segment: RefCell::new(Some(new_segment)),
        };
        Some(SerializableRef::Owned(Box::new(item_serializable)))
    }
}

struct SegmentPushingSerializable<'a> {
    serializable: SerializableRef<'a>,
    segment: RefCell<Option<PathSegment>>,
}

impl<'a> Serializable for SegmentPushingSerializable<'a> {
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

    fn done(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.done(state)?;
        let mut path = state.get_mut::<Path>();
        path.segments.pop();
        Ok(())
    }
}

struct SegmentCollectingSerializable<'a> {
    serializable: SerializableRef<'a>,
    segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> Serializable for SegmentCollectingSerializable<'a> {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        match self.serializable.serialize(state)? {
            Chunk::Str(key) => {
                *self.segment.borrow_mut() = Some(PathSegment::Key(key.to_string()));
                Ok(Chunk::Str(key))
            }
            Chunk::U64(val) => {
                *self.segment.borrow_mut() = Some(PathSegment::Index(val as usize));
                Ok(Chunk::U64(val))
            }
            other => Ok(other),
        }
    }

    fn done(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.done(state)
    }
}
