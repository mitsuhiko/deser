//! This crate provides a [`PathLayer`] that tracks the path of the current
//! value during serialization and deserialization in the
//! [`State`] (see [`Path`]).
//!
//! The layer can be added to both a
//! [`DeserializeDriver`](deser::de::DeserializeDriver) and a
//! [`SerializeDriver`](deser::ser::SerializeDriver).  Types can retrieve
//! the current [`Path`] from the state and errors get the path of the value
//! they refer to attached (see [`Error::attachment`]):
//!
//! ```rust
//! use deser_path::{Path, PathLayer, PathSegment};
//!
//! #[derive(deser::Deserialize, Debug)]
//! struct Server {
//!     host: String,
//!     port: u16,
//! }
//!
//! let mut de = deser_json::Deserializer::from_str(r#"[{"host": "a", "port": "80"}]"#);
//! let err = de
//!     .deserialize_with::<Vec<Server>, _>(|driver| driver.push_layer(PathLayer::new()))
//!     .unwrap_err();
//! let path = err.attachment::<Path>().unwrap();
//! assert_eq!(path.to_string(), "[0].port");
//! assert_eq!(path.segments()[0], PathSegment::Index(0));
//! assert_eq!(
//!     err.to_string(),
//!     "Unexpected: unexpected string, expected u16 at line 1 column 24 (path: [0].port)"
//! );
//! ```
//!
//! During serialization, the path is available to the
//! [`Serialize`](deser::Serialize) implementations:
//!
//! ```rust
//! use deser_path::{Path, PathLayer};
//! use deser::ser::{Serialize, SerializeDriver, Chunk};
//! use deser::State;
//! use deser::Error;
//!
//! struct MyInt(u32);
//!
//! impl Serialize for MyInt {
//!     fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
//!         // for as long as the `PathLayer` is added we can at any point
//!         // request the current path from the state.
//!         println!("{}", state.get::<Path>().unwrap());
//!         self.0.serialize(state)
//!     }
//! }
//!
//! let serializable = vec![MyInt(42), MyInt(23)];
//! let mut driver = SerializeDriver::new(&serializable);
//! driver.push_layer(PathLayer::new());
//! driver.drive(|_event, _state| Ok(())).unwrap();
//! ```
use std::fmt;

use deser::{Atom, Error, ErrorAttachment, ErrorContext, State};

mod de;
mod ser;

/// A single segment in the path.
#[derive(Debug, PartialEq, Eq)]
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

impl Clone for PathSegment {
    fn clone(&self) -> PathSegment {
        match self {
            PathSegment::Unknown => PathSegment::Unknown,
            PathSegment::Index(index) => PathSegment::Index(*index),
            PathSegment::Key(key) => PathSegment::Key(key.clone()),
        }
    }

    fn clone_from(&mut self, source: &PathSegment) {
        // reuse the buffer of keys
        match (self, source) {
            (PathSegment::Key(buf), PathSegment::Key(key)) => buf.clone_from(key),
            (this, source) => *this = source.clone(),
        }
    }
}

/// The current path of the serialization or deserialization.
///
/// This type is stored in the state and can be retrieved at any point.  By
/// inspecting the [`segments`](Self::segments) a type can figure out where
/// it's invoked from.  It formats as `servers[1].host`.
///
/// The [`PathLayer`] also attaches the path to errors (see
/// [`Error::attachment`]) where it shows up as `(path: servers[1].host)` in
/// the error message.
#[derive(Default)]
pub struct Path {
    segments: Vec<PathSegment>,
    // buffers of popped keys that are reused for new keys
    spare_keys: Vec<String>,
}

/// The maximum number of key buffers retained for reuse.
const MAX_SPARE_KEYS: usize = 32;

impl Path {
    /// Returns the segments.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Pushes a segment.
    fn push(&mut self, segment: PathSegment) {
        self.segments.push(segment);
    }

    /// Pops a segment and retains the buffer of keys.
    fn pop(&mut self) {
        if let Some(PathSegment::Key(buf)) = self.segments.pop() {
            self.recycle(buf);
        }
    }

    /// Sets the last segment.
    fn set_last(&mut self, segment: PathSegment) {
        if let Some(last) = self.segments.last_mut() {
            *last = segment;
        }
    }

    /// Sets the last segment to the key in the atom.
    ///
    /// This reuses the allocation of the previous key if possible.
    fn set_last_key(&mut self, atom: &Atom) {
        if let (Some(PathSegment::Key(buf)), Atom::Str(key)) = (self.segments.last_mut(), atom) {
            buf.clear();
            buf.push_str(key);
            return;
        }
        let segment = self.key_segment(atom);
        if let Some(last) = self.segments.last_mut()
            && let PathSegment::Key(buf) = std::mem::replace(last, segment)
        {
            self.recycle(buf);
        }
    }

    /// Returns the segment for a key.
    fn key_segment(&mut self, atom: &Atom) -> PathSegment {
        match *atom {
            Atom::Str(ref key) => {
                let mut buf = self.spare_keys.pop().unwrap_or_default();
                buf.clear();
                buf.push_str(key);
                PathSegment::Key(buf)
            }
            Atom::U64(value) => PathSegment::Index(value as usize),
            Atom::I64(value) if value >= 0 => PathSegment::Index(value as usize),
            Atom::Ext(ref ext) => {
                // extension values (like annotated keys) use their fallback
                match ext.fallback() {
                    Atom::Ext(_) => PathSegment::Unknown,
                    fallback => self.key_segment(&fallback),
                }
            }
            _ => PathSegment::Unknown,
        }
    }

    /// Retains the buffer of a key for reuse.
    fn recycle(&mut self, buf: String) {
        if self.spare_keys.len() < MAX_SPARE_KEYS {
            self.spare_keys.push(buf);
        }
    }
}

impl Clone for Path {
    fn clone(&self) -> Path {
        Path {
            segments: self.segments.clone(),
            spare_keys: Vec::new(),
        }
    }

    fn clone_from(&mut self, source: &Path) {
        // this is invoked for every replayed event, reuse the memory
        self.segments.clone_from(&source.segments);
    }
}

impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Path")
            .field("segments", &self.segments)
            .finish()
    }
}

/// Formats the path as `servers[1].host`.
///
/// The root path is formatted as an empty string and unknown segments as
/// `?`.
impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, segment) in self.segments.iter().enumerate() {
            match segment {
                PathSegment::Key(key) => {
                    if idx > 0 {
                        f.write_str(".")?;
                    }
                    f.write_str(key)?;
                }
                PathSegment::Index(index) => write!(f, "[{}]", index)?,
                PathSegment::Unknown => {
                    if idx > 0 {
                        f.write_str(".")?;
                    }
                    f.write_str("?")?;
                }
            }
        }
        Ok(())
    }
}

impl ErrorAttachment for Path {
    fn fmt_context(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, " (path: {})", self)
    }
}

/// A layer that tracks the current [`Path`] in the state.
///
/// The layer works for serialization (it implements
/// [`deser::ser::Layer`]) and deserialization (it implements
/// [`deser::de::Layer`]).  It attaches the path to errors which do not have
/// one (see [`Error::attachment`]).
///
/// During deserialization the path is also correct for values which are
/// buffered and replayed (for instance by internally tagged enums).
///
/// Layers see the path of the current event if they are added after the
/// path layer.  This also means that the errors of such layers get the path
/// of the event they reject attached, so the path layer should typically be
/// added first.
#[derive(Debug, Default)]
pub struct PathLayer {
    frames: Vec<Frame>,
    registered: bool,
}

/// A container that is open.
#[derive(Debug)]
enum Frame {
    /// A map, the flag is `true` if a value is expected next (only used
    /// during serialization).
    Map(bool),
    /// A sequence with the index of the next item.
    Seq(usize),
}

impl PathLayer {
    /// Creates a new path layer.
    pub fn new() -> PathLayer {
        PathLayer::default()
    }

    /// Registers the path with the state.
    #[cold]
    fn register(&mut self, state: &mut State, replayable: bool) {
        self.registered = true;
        state.get_mut::<Path>();
        if replayable {
            state.set_replayable::<Path>();
        }
        state.add_error_context::<PathContext>();
    }
}

/// Attaches the current path to errors.
struct PathContext;

impl ErrorContext for PathContext {
    fn add_context(err: Error, state: &State) -> Error {
        if err.attachment::<Path>().is_some() {
            return err;
        }
        match state.get::<Path>() {
            Some(path) if !path.segments.is_empty() => err.with_attachment(path.clone()),
            _ => err,
        }
    }
}
