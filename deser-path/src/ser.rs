use std::borrow::Cow;
use std::cell::RefCell;

use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle, StructEmitter};
use deser::State;
use deser::{Atom, Descriptor, Error};

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
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.serializable.serialize(state)? {
            Chunk::Struct(emitter) => Ok(Chunk::Struct(Box::new(PathStructEmitter { emitter }))),
            Chunk::Map(emitter) => Ok(Chunk::Map(Box::new(PathMapEmitter { emitter }))),
            Chunk::Seq(emitter) => Ok(Chunk::Seq(Box::new(PathSeqEmitter { emitter, index: 0 }))),
            other => Ok(other),
        }
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.serializable.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.serializable.is_optional()
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.serializable.descriptor()
    }
}

struct PathStructEmitter<'a> {
    emitter: Box<dyn StructEmitter + 'a>,
}

impl<'a> StructEmitter for PathStructEmitter<'a> {
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        let (key, value) = match self.emitter.next(state)? {
            Some(result) => result,
            None => return Ok(None),
        };
        // the key is only copied into the path when the value is serialized
        let value_serializable = SegmentPushingSerializable {
            serializable: value,
            segment: RefCell::new(Some(PendingSegment::Key(key.clone()))),
        };
        Ok(Some((key, SerializeHandle::boxed(value_serializable))))
    }
}

struct PathMapEmitter<'a> {
    emitter: Box<dyn MapEmitter + 'a>,
}

impl<'a> MapEmitter for PathMapEmitter<'a> {
    fn next_key(&mut self, state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        state.get_mut::<Path>().pending_key = None;
        let key_serializable = SegmentCollectingSerializable {
            serializable: match self.emitter.next_key(state)? {
                Some(result) => result,
                None => return Ok(None),
            },
        };
        Ok(Some(SerializeHandle::boxed(key_serializable)))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        let new_segment = state
            .get_mut::<Path>()
            .pending_key
            .take()
            .unwrap_or(PathSegment::Unknown);
        let value_serializable = SegmentPushingSerializable {
            serializable: self.emitter.next_value(state)?,
            segment: RefCell::new(Some(PendingSegment::Segment(new_segment))),
        };
        Ok(SerializeHandle::boxed(value_serializable))
    }
}

struct PathSeqEmitter<'a> {
    emitter: Box<dyn SeqEmitter + 'a>,
    index: usize,
}

impl<'a> SeqEmitter for PathSeqEmitter<'a> {
    fn next(&mut self, state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        let index = self.index;
        self.index += 1;
        let value = match self.emitter.next(state)? {
            Some(result) => result,
            None => return Ok(None),
        };
        let new_segment = PathSegment::Index(index);
        let item_serializable = SegmentPushingSerializable {
            serializable: value,
            segment: RefCell::new(Some(PendingSegment::Segment(new_segment))),
        };
        Ok(Some(SerializeHandle::boxed(item_serializable)))
    }
}

/// A segment that is pushed once the value is serialized.
enum PendingSegment<'a> {
    Key(Cow<'a, str>),
    Segment(PathSegment),
}

struct SegmentPushingSerializable<'a> {
    serializable: SerializeHandle<'a>,
    segment: RefCell<Option<PendingSegment<'a>>>,
}

impl<'a> Serialize for SegmentPushingSerializable<'a> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        {
            let path = state.get_mut::<Path>();
            match self.segment.take().unwrap() {
                PendingSegment::Key(key) => path.push_key(&key),
                PendingSegment::Segment(segment) => path.segments.push(segment),
            }
        }
        match self.serializable.serialize(state)? {
            Chunk::Struct(emitter) => Ok(Chunk::Struct(Box::new(PathStructEmitter { emitter }))),
            Chunk::Map(emitter) => Ok(Chunk::Map(Box::new(PathMapEmitter { emitter }))),
            Chunk::Seq(emitter) => Ok(Chunk::Seq(Box::new(PathSeqEmitter { emitter, index: 0 }))),
            other => Ok(other),
        }
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.serializable.finish(state)?;
        state.get_mut::<Path>().pop();
        Ok(())
    }

    fn is_optional(&self) -> bool {
        self.serializable.is_optional()
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.serializable.descriptor()
    }
}

/// Records the serialized key as pending key in the path.
struct SegmentCollectingSerializable<'a> {
    serializable: SerializeHandle<'a>,
}

impl<'a> Serialize for SegmentCollectingSerializable<'a> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        match self.serializable.serialize(state)? {
            Chunk::Atom(Atom::Str(key)) => {
                state.get_mut::<Path>().pending_key = Some(PathSegment::Key(key.to_string()));
                Ok(Chunk::Atom(Atom::Str(key)))
            }
            Chunk::Atom(Atom::U64(val)) => {
                state.get_mut::<Path>().pending_key = Some(PathSegment::Index(val as usize));
                Ok(Chunk::Atom(Atom::U64(val)))
            }
            other => Ok(other),
        }
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.serializable.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.serializable.is_optional()
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.serializable.descriptor()
    }
}
