//! Deser is an experimental serialization system for Rust.  It wants to explore
//! the possibilities of serialization and deserialization of structural formats
//! such as JSON or msgpack.  It intentionally does not desire to support non
//! self describing formats such as bincode.
//!
//! There is not much in terms of actual serialization and deserialization yet
//! as this library at this point is an exploration in API design for the
//! abstraction layer itself.
//!
//! **This is not a production ready yet.**
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
//! For more information have a look at the GitHub repository:
//! [mitsuhiko/deser](https://github.com/mitsuhiko/deser).
//!
//! ## Features
//!
//! * `derive` turns on basic derive support for [`Serialize`] and [`Deserialize`].  For more
//!   information see [`derive`](crate::derive).

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

    mod _hack {
        pub type Str = str;
    }
    pub use self::_hack::Str as str;
}
