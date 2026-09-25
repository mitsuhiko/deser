//! This crate provides wrapper types that observe the serialization and
//! deserialization to communicate the current path into the
//! [`State`](deser::State).
//!
//! ```rust
//! use deser_path::{Path, PathSerializable};
//! use deser::ser::{Serialize, SerializeDriver, Chunk};
//! use deser::State;
//! use deser::Error;
//!
//! struct MyInt(u32);
//!
//! impl Serialize for MyInt {
//!     fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
//!         // for as long as we're wrapped with the `PathSerializable` we can at
//!         // any point request the current path from the state.
//!         println!("{:?}", state.get::<Path>().map(Path::segments));
//!         self.0.serialize(state)
//!     }
//! }
//!
//! let serializable = vec![MyInt(42), MyInt(23)];
//! let path_serializable = PathSerializable::wrap(&serializable);
//!
//! // now serialize path_serializable instead
//! let mut driver = SerializeDriver::new(&path_serializable);
//! while driver.next().unwrap().is_some() {
//!     // ...
//! }
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
#[derive(Default)]
pub struct Path {
    pub(crate) segments: Vec<PathSegment>,
    // buffers of popped keys that are reused for new keys
    spare_keys: Vec<String>,
    // during serialization the segment of the map key that was serialized
    // last, it becomes the segment of the value that follows.
    pub(crate) pending_key: Option<PathSegment>,
}

/// The maximum number of key buffers retained for reuse.
const MAX_SPARE_KEYS: usize = 32;

impl Path {
    /// Returns the segments.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Pushes a key segment, reusing a previously popped key buffer.
    pub(crate) fn push_key(&mut self, key: &str) {
        let mut buf = self.spare_keys.pop().unwrap_or_default();
        buf.clear();
        buf.push_str(key);
        self.segments.push(PathSegment::Key(buf));
    }

    /// Pops a segment and retains the buffer of keys.
    pub(crate) fn pop(&mut self) {
        if let Some(PathSegment::Key(buf)) = self.segments.pop() {
            if self.spare_keys.len() < MAX_SPARE_KEYS {
                self.spare_keys.push(buf);
            }
        }
    }
}

impl Clone for Path {
    fn clone(&self) -> Path {
        Path {
            segments: self.segments.clone(),
            spare_keys: Vec::new(),
            pending_key: None,
        }
    }
}

impl std::fmt::Debug for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Path")
            .field("segments", &self.segments)
            .finish()
    }
}
