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
//! the following formats are supported:
//!
//! * [`deser-json`](https://docs.rs/deser-json): implements JSON serialization and
//!   deserialization.
//! * [`deser-cbor`](https://docs.rs/deser-cbor): implements CBOR serialization and
//!   deserialization.
//!
//! The data model can be extended with types that are not native to it.  For
//! more information see [`ext`].
//!
//! How individual values are serialized and deserialized can be customized
//! with adapters, which compose with containers.  For more information see
//! [`adapters`].
//!
//! # Features
//!
//! * `derive` turns on basic derive support for [`Serialize`] and [`Deserialize`].  For more
//!   information see [`derive`](crate::derive).  This feature is enabled by default.
//! * `jiff`, `chrono`, `time`, `uuid`, `rust_decimal`, `bigdecimal` and `num-bigint`
//!   implement [`Serialize`] and [`Deserialize`] for the types of these crates.  They
//!   are serialized as [well-known types](crate::ext#well-known-types) which data
//!   formats can support natively.

#[macro_use]
mod macros;
mod event;

pub mod adapters;
pub mod de;
mod error;
pub mod ext;
pub mod ser;

mod descriptors;
mod extensions;
mod state;

#[cfg(doctest)]
mod soundness;

#[cfg(all(doctest, feature = "derive"))]
mod derive_errors;

pub use self::descriptors::Descriptor;
pub use self::error::{Error, ErrorKind};
pub use self::event::{Atom, Event};
pub use self::state::State;

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
    pub use std::convert::Into;
    pub use std::default::Default;
    pub use std::marker::PhantomData;
    pub use std::mem::replace;
    pub use std::option::Option::{self, None, Some};
    pub use std::result::Result::{Err, Ok};
    pub use std::string::String;
    pub type Result<T> = std::result::Result<T, super::Error>;
    pub type StrCow<'a> = Cow<'a, str>;

    pub use crate::de::enums::{
        untagged_handle, AdjacentlyTaggedSink, BoxedVariant, ExternallyTaggedSink, IgnoredContent,
        IgnoredVariant, InternallyTaggedSink, OtherVariant, Variant, VariantBuilder, VariantMaker,
        Variants,
    };
    pub use crate::ser::enums::{EntrySer, FieldsSer, SeqSer, TaggedNewtype};
    pub use std::vec::Vec;

    pub use crate::de::{
        atom_into, atom_into_handle, borrowed_atom_into, borrowed_atom_into_handle,
    };

    #[cold]
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
