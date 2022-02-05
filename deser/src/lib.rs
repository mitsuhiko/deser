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

#[doc(inline)]
pub use self::{de::Deserialize, ser::Serialize};

/// Provides automatic deriving for [`Serialize`].
///
/// At the moment this can only derive structs.  Special attributes
/// can be provided with the `deser` attribute.  This is largely
/// modelled after serde.  At the moment the following attributes
/// exist:
///
/// * `#[deser(rename = "field")]`: renames a struct or field.
/// * `#[deser(rename_all = "...")]`: renames all fields at once to a
///   specific name style.  he possible values are `"lowercase"`, `"UPPERCASE"`,
///   `"PascalCase"`, `"camelCase"`, `"snake_case"`, `"SCREAMING_SNAKE_CASE"`,
///   `"kebab-case"`, and `"SCREAMING-KEBAB-CASE"`.
#[cfg(feature = "derive")]
pub use deser_derive::Serialize;

// These are re-exported fro the derive macro.  There is no good
// reason for this right now as deser does not yet have no-std
// support but this will make it easier later to add support.
#[cfg(feature = "derive")]
#[doc(hidden)]
pub mod __derive {
    pub use std::borrow::Cow;
    pub use std::boxed::Box;
    pub use std::option::Option::{self, None, Some};
    pub use std::result::Result::{Err, Ok};
    pub type Result<T> = std::result::Result<T, super::Error>;
    pub type StrCow<'a> = Cow<'a, str>;

    mod _hack {
        pub type Str = str;
    }
    pub use self::_hack::Str as str;
}
