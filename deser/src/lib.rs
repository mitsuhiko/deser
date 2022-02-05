//! Deser is an experimental serialization and deserialization library for Rust.
//!
//! There is not much in terms of actual serialization and deserialization yet
//! as this library at this point is an exploration in API design for the
//! abstraction layer itself.
//!
//! For more information have a look at the GitHub repository:
//! [mitsuhiko/deser](https://github.com/mitsuhiko/deser).

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
