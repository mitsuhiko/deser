//! <div align="center">
//!  <img src="https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg" width="250" height="250">
//!  <p><strong>deser: an experimental serialization and deserialization library for Rust</strong></p>
//! </div>
//!
//! Deser is an experimental serialization system for Rust.  It wants to explore
//! the possibilities of serialization and deserialization of structural formats
//! such as JSON or msgpack.  It intentionally does not desire to support non
//! self describing formats such as bincode.
//!
//! It supports deriving structures that can be serialized and derserialized
//! automatically:
//!
#![cfg_attr(
    feature = "derive",
    doc = r#"
```rust
use deser::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct Account {
    id: usize,
    account_holder: String,
    is_deactivated: bool,
}
```
"#
)]
//!
//! To serialize or deserialize this a data format implementation is needed.  At the moment
//! only JSON is supported:
//!
//! * [`deser-json`](https://docs.rs/deser-json): implements JSON serialization and
//!   deserialization.
//!
//! # Features
//!
//! * `derive` turns on basic derive support for [`Serialize`] and [`Deserialize`].  For more
//!   information see [`derive`](crate::derive).  This feature is enabled by default.

#[macro_use]
mod macros;
mod event;

pub mod de;
mod error;
pub mod ext;
pub mod ser;

mod descriptors;

pub use self::descriptors::Descriptor;
pub use self::error::{Error, ErrorKind};
pub use self::event::{Atom, Event};

// common re-exports

#[doc(no_inline)]
pub use self::{de::Deserialize, ser::Serialize};

#[cfg(feature = "derive")]
#[doc(no_inline)]
pub use self::derive::{Deserialize, Serialize};

#[cfg(feature = "derive")]
pub mod derive;

// These are re-exported fro the derive macro.  There is no good
// reason for this right now as deser does not yet have no-std
// support but this will make it easier later to add support.
#[cfg(feature = "derive")]
#[doc(hidden)]
pub mod __derive {
    pub use std::borrow::Cow;
    pub use std::boxed::Box;
    pub use std::default::Default;
    pub use std::option::Option::{self, None, Some};
    pub use std::result::Result::{Err, Ok};
    pub type Result<T> = std::result::Result<T, super::Error>;
    pub type StrCow<'a> = Cow<'a, str>;

    pub fn new_missing_field_error(name: &str) -> super::Error {
        super::Error::new(
            super::ErrorKind::MissingField,
            format!("Missing field '{}'", name),
        )
    }

    mod _hack {
        pub type Str = str;
    }
    pub use self::_hack::Str as str;
}
