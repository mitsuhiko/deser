//! Deser is an experimental serialization and deserialization library for Rust.

#[macro_use]
mod macros;
mod event;

pub mod de;
mod error;
pub mod ser;

mod descriptors;
mod extensions;

pub use self::descriptors::Descriptor;
pub use self::error::{Error, ErrorKind};
pub use self::event::Event;
