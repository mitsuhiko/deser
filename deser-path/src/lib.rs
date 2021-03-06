//! This crate provides a wrapper type that observes the serialization to communicate
//! the current path of the serialization into the [`SerializerState`](deser::ser::SerializerState)
//! and [`DeserializerState`](deser::de::DeserializerState).
//!
//! ```rust
//! use deser_path::{Path, PathSerializable};
//! use deser::ser::{Serialize, SerializerState, Chunk};
//! use deser::{Atom, Error};
//!
//! struct MyInt(u32);
//!
//! impl Serialize for MyInt {
//!     fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
//!         // for as long as we're wrapped with the `PathSerializable` we can at
//!         // any point request the current path from the state.
//!         let path = state.get::<Path>();
//!         println!("{:?}", path.segments());
//!         self.0.serialize(state)
//!     }
//! }
//!
//! let serializable = vec![MyInt(42), MyInt(23)];
//! let path_serializable = PathSerializable::wrap(&serializable);
//! // now serialize path_serializable instead
//! ```
mod de;
mod ser;

pub use de::*;
pub use ser::*;

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
/// This type is stored in the state and can be retrieved at any point.  By
/// inspecting the [`segments`](Self::segments) a serializer can figure out
/// where it's invoked from.
#[derive(Debug, Default, Clone)]
pub struct Path {
    pub(crate) segments: Vec<PathSegment>,
}

impl Path {
    /// Returns the segments.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }
}
