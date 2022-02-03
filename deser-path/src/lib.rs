//! This crate provides a wrapper type that observes the serialization to communicate
//! the current path of the serialization into the [`SerializerState`].
//!
//! ```rust
//! use deser_path::{Path, PathSerializable};
//! use deser::ser::{Serializable, SerializerState, Chunk};
//! use deser::Error;
//!
//! struct MyInt(u32);
//!
//! impl Serializable for MyInt {
//!     fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
//!         // for as long as we're wrapped with the `PathSerializable` we can at
//!         // any point request the current path from the state.
//!         let path = state.get::<Path>();
//!         println!("{:?}", path.segments());
//!         Ok(Chunk::U64(self.0 as u64))
//!     }
//! }
//!
//! let serializable = vec![MyInt(42), MyInt(23)];
//! let path_serializable = PathSerializable::wrap(&serializable);
//! // now serialize path_serializable instead
//! ```
use std::borrow::Cow;
use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serializable, SerializerState, StructEmitter};
use deser::Error;

macro_rules! extend_lifetime {
    ($expr:expr, $t:ty) => {
        std::mem::transmute::<$t, $t>($expr)
    };
}

/// A single segment in the path.
#[derive(Debug, Clone)]
pub enum PathSegment {
    /// An unknown path segment.
    ///
    /// This can happen if the key was not a string or unsigned integer.
    Unknown,
    /// An unsigned index.
    Index(usize),
    /// A string key.
    Key(String),
}

/// The current path of the serialization.
///
/// This type is stored in the [`SerializerState`] and can be retrieved at any point.
/// By inspecting the [`segments`](Self::segments) a serializer can figure out where
/// it's invoked from.
#[derive(Debug, Default, Clone)]
pub struct Path {
    segments: Vec<PathSegment>,
}

impl Path {
    /// Returns the segments.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }
}

/// Wraps a serializable so that it tracks the current path.
pub struct PathSerializable<'a> {
    serializable: &'a dyn Serializable,
}

impl<'a> PathSerializable<'a> {
    /// Wraps another serializable.
    pub fn wrap(serializable: &'a dyn Serializable) -> PathSerializable<'a> {
        PathSerializable { serializable }
    }
}

impl<'a> Serializable for PathSerializable<'a> {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        match self.serializable.serialize(state)? {
            Chunk::Struct(emitter) => Ok(Chunk::Struct(Box::new(PathStructEmitter {
                emitter,
                stashed_serializable: None,
            }))),
            Chunk::Map(emitter) => Ok(Chunk::Map(Box::new(PathMapEmitter {
                emitter,
                key_serializable: None,
                value_serializable: None,
                path_segment: Rc::default(),
            }))),
            Chunk::Seq(emitter) => Ok(Chunk::Seq(Box::new(PathSeqEmitter {
                emitter,
                stashed_serializable: None,
                index: 0,
            }))),
            other => Ok(other),
        }
    }

    fn done(&self, state: &SerializerState) -> Result<(), Error> {
        self.serializable.done(state)
    }
}

struct PathStructEmitter<'a> {
    emitter: Box<dyn StructEmitter + 'a>,
    stashed_serializable: Option<SegmentPushingSerializable>,
}

impl<'a> StructEmitter for PathStructEmitter<'a> {
    fn next(&mut self) -> Option<(Cow<'_, str>, &dyn Serializable)> {
        let (key, value) = self.emitter.next()?;
        let new_segment = PathSegment::Key(key.to_string());
        unsafe {
            self.stashed_serializable = Some(SegmentPushingSerializable {
                real: NonNull::from(extend_lifetime!(value, &dyn Serializable)),
                segment: RefCell::new(Some(new_segment)),
            });
        }
        Some((key, self.stashed_serializable.as_ref().unwrap()))
    }
}

struct PathMapEmitter<'a> {
    emitter: Box<dyn MapEmitter + 'a>,
    key_serializable: Option<SegmentCollectingSerializable>,
    value_serializable: Option<SegmentPushingSerializable>,
    path_segment: Rc<RefCell<Option<PathSegment>>>,
}

impl<'a> MapEmitter for PathMapEmitter<'a> {
    fn next_key(&mut self) -> Option<&dyn Serializable> {
        unsafe {
            self.key_serializable = Some(SegmentCollectingSerializable {
                real: NonNull::from(extend_lifetime!(
                    self.emitter.next_key()?,
                    &dyn Serializable
                )),
                segment: self.path_segment.clone(),
            });
        }
        Some(self.key_serializable.as_ref().unwrap())
    }

    fn next_value(&mut self) -> &dyn Serializable {
        let new_segment = self
            .path_segment
            .borrow_mut()
            .take()
            .unwrap_or(PathSegment::Unknown);
        unsafe {
            self.value_serializable = Some(SegmentPushingSerializable {
                real: NonNull::from(extend_lifetime!(
                    self.emitter.next_value(),
                    &dyn Serializable
                )),
                segment: RefCell::new(Some(new_segment)),
            });
        }
        self.value_serializable.as_ref().unwrap()
    }
}

struct PathSeqEmitter<'a> {
    emitter: Box<dyn SeqEmitter + 'a>,
    stashed_serializable: Option<SegmentPushingSerializable>,
    index: usize,
}

impl<'a> SeqEmitter for PathSeqEmitter<'a> {
    fn next(&mut self) -> Option<&dyn Serializable> {
        let index = self.index;
        self.index += 1;
        let value = self.emitter.next()?;
        let new_segment = PathSegment::Index(index);
        unsafe {
            self.stashed_serializable = Some(SegmentPushingSerializable {
                real: NonNull::from(extend_lifetime!(value, &dyn Serializable)),
                segment: RefCell::new(Some(new_segment)),
            });
        }
        Some(self.stashed_serializable.as_mut().unwrap())
    }
}

struct SegmentPushingSerializable {
    real: NonNull<dyn Serializable>,
    segment: RefCell<Option<PathSegment>>,
}

impl Serializable for SegmentPushingSerializable {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        {
            let mut path = state.get_mut::<Path>();
            path.segments.push(self.segment.take().unwrap());
        }
        match unsafe { self.real.as_ref().serialize(state)? } {
            Chunk::Struct(emitter) => Ok(Chunk::Struct(Box::new(PathStructEmitter {
                emitter,
                stashed_serializable: None,
            }))),
            Chunk::Map(emitter) => Ok(Chunk::Map(Box::new(PathMapEmitter {
                emitter,
                key_serializable: None,
                value_serializable: None,
                path_segment: Rc::default(),
            }))),
            Chunk::Seq(emitter) => Ok(Chunk::Seq(Box::new(PathSeqEmitter {
                emitter,
                stashed_serializable: None,
                index: 0,
            }))),
            other => Ok(other),
        }
    }

    fn done(&self, state: &SerializerState) -> Result<(), Error> {
        unsafe { self.real.as_ref().done(state)? };
        let mut path = state.get_mut::<Path>();
        path.segments.pop();
        Ok(())
    }
}

struct SegmentCollectingSerializable {
    real: NonNull<dyn Serializable>,
    segment: Rc<RefCell<Option<PathSegment>>>,
}

impl Serializable for SegmentCollectingSerializable {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        match unsafe { self.real.as_ref().serialize(state) }? {
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
        unsafe { self.real.as_ref().done(state) }
    }
}
