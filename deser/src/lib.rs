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
//! With the `derive` feature it supports deriving structures that can be
//! serialized and deserialized automatically:
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
//! * [`deser-jsonc`](https://docs.rs/deser-jsonc): implements deserialization of
//!   JSONC (JSON with comments, as used by configuration files) and serialization
//!   as JSON.
//! * [`deser-json5`](https://docs.rs/deser-json5): implements deserialization of
//!   [JSON5](https://json5.org/) and serialization as JSON.
//! * [`deser-hj`](https://docs.rs/deser-hj): implements deserialization of
//!   [Hjson](https://hjson.github.io/) and serialization as JSON.
//! * [`deser-cbor`](https://docs.rs/deser-cbor): implements CBOR serialization and
//!   deserialization.
//! * [`deser-msgpack`](https://docs.rs/deser-msgpack): implements MessagePack
//!   serialization and deserialization.
//! * [`deser-toml`](https://docs.rs/deser-toml): implements TOML serialization and
//!   deserialization.
//! * [`deser-yaml`](https://docs.rs/deser-yaml): implements YAML serialization and
//!   deserialization.
//! * [`deser-urlencoded`](https://docs.rs/deser-urlencoded): implements query string
//!   and form data (`application/x-www-form-urlencoded`) serialization and
//!   deserialization.
//! * [`deser-xml`](https://docs.rs/deser-xml): implements XML serialization and
//!   deserialization.
//! * [`deser-plist`](https://docs.rs/deser-plist): implements property list
//!   (XML, binary and OpenStep) serialization and deserialization.
//! * [`deser-csv`](https://docs.rs/deser-csv): implements CSV, TSV and other
//!   delimited text serialization and deserialization.
//! * [`deser-env`](https://docs.rs/deser-env): implements reading
//!   configuration from environment variables (and writing values into
//!   them).
//!
//! The data formats have a deserializer (which deserializes values from a
//! slice) and a serializer (which serializes values into a buffer).  Values
//! can also be read from and written to streams (such as files or sockets),
//! see [`io`][io-module].  The stream serializers and deserializers of the
//! formats do not do IO themselves (see [`stream`]), so they also work with
//! other kinds of IO and without the standard library.
//!
//! Configuration that is given to a serialization or deserialization from
//! the outside, like how bytes are represented in formats without native
//! bytes ([`BytesFormat`]), what happens with repeated keys
//! ([`DuplicateKeys`](de::DuplicateKeys)) or unknown fields
//! ([`UnknownFields`](de::UnknownFields)) and whether errors are collected
//! ([`CollectErrors`](de::CollectErrors)), is held in a [`Context`].  It's
//! created once and given to the deserializers and serializers of the
//! formats (and the drivers, readers and writers) with their `set_context`
//! methods.
//!
//! The data model can be extended with types that are not native to it.  For
//! more information see [`ext`].
//!
//! How individual values are serialized and deserialized can be customized
//! with adapters, which compose with containers.  For more information see
//! [`adapters`].  Types which only implement serde's traits can be used with
//! the adapters of [`deser-serde`](https://docs.rs/deser-serde).  Bytes are
//! base64 strings in formats without native bytes, more encodings (such as
//! hexadecimal and base32) are provided by
//! [`deser-encoding`](https://docs.rs/deser-encoding).
//!
//! Further functionality is provided by these crates:
//!
//! * [`deser-value`](https://docs.rs/deser-value): a dynamic value type which can
//!   hold any value of the data model, to inspect or transform data or to
//!   convert between formats.
//! * [`deser-path`](https://docs.rs/deser-path): a layer that tracks the path of
//!   the current value (like `servers[1].timeout`) and attaches it to errors.
//! * [`deser-validate`](https://docs.rs/deser-validate): validates values while
//!   they are deserialized.
//! * [`deser-location`](https://docs.rs/deser-location): resolves the source
//!   locations (line and column) of values while they are deserialized.
//! * [`deser-debug`](https://docs.rs/deser-debug): formats serializable values
//!   like their [`Debug`](core::fmt::Debug) implementation would.
//! * [`deser-tokio`](https://docs.rs/deser-tokio): reads and writes values with
//!   tokio's asynchronous streams.
//! * [`deser-serde`](https://docs.rs/deser-serde): adapters to use serde types.
//! * [`deser-encoding`](https://docs.rs/deser-encoding): hexadecimal and base32
//!   encodings of bytes.
//!
//! # Features
//!
//! * `derive` turns on basic derive support for [`Serialize`] and [`Deserialize`].  For more
//!   information see [`derive`][derive-module].
//! * `open-enums` adds open enums: traits whose implementations (in any crate) are the
//!   variants of their trait objects, see [open enums][open-enums].
//! * `jiff`, `chrono`, `time`, `uuid`, `rust_decimal`, `bigdecimal` and `num-bigint`
//!   implement [`Serialize`] and [`Deserialize`] for the types of these crates.  They
//!   are serialized as [well-known types](crate::ext#well-known-types) which data
//!   formats can support natively.
//! * `indexmap`, `hashbrown`, `smallvec`, `arrayvec`, `bytes` and `bstr` implement
//!   [`Serialize`] and [`Deserialize`] for the collections and byte buffers of these
//!   crates.  They behave like their counterparts in the standard library, including
//!   the [adapters](crate::adapters) (for instance `IndexMap<_, DisplayFromStr>`).
//!   Without `std` the adapters of `IndexMap` and `IndexSet` are not available.
//! * `io` (enabled by default) adds [`io`][io-module] to read values from and
//!   write values to streams of the standard library (`std::io`).  It
//!   requires `std`.  Reading and writing streams without IO (see
//!   [`stream`]) does not need it.
//! * `std` (enabled by default) uses the standard library, see below.
//!
//! # `no_std`
//!
//! Without the `std` feature deser only needs `alloc` (a global allocator)
//! and works on targets without an operating system.  Disable the default
//! features of deser and of the formats (`deser-json`, `deser-jsonc`,
//! `deser-json5`, `deser-hj`, `deser-cbor`, `deser-msgpack`,
//! `deser-plist` and `deser-csv` support this):
//!
//! ```toml
//! [dependencies]
//! deser-cbor = { version = "0.9", default-features = false }
//!
//! [dependencies.deser]
//! version = "0.9"
//! default-features = false
//! features = ["derive"]
//! ```
//!
//! Everything that is not in `core` and `alloc` is not available: `io`,
//! the implementations for `HashMap` and `HashSet` (use the `hashbrown`
//! feature instead), `Path`, `OsStr`, `SystemTime`, `Mutex`, `RwLock` and
//! `OnceLock`.
//!
#![cfg_attr(feature = "derive", doc = "[derive-module]: crate::derive")]
#![cfg_attr(feature = "derive", doc = "[open-enums]: crate::derive#open-enums")]
#![cfg_attr(
    not(feature = "derive"),
    doc = "[open-enums]: https://docs.rs/deser/latest/deser/derive/index.html#open-enums"
)]
#![cfg_attr(feature = "io", doc = "[io-module]: crate::io")]
#![cfg_attr(
    not(feature = "io"),
    doc = "[io-module]: https://docs.rs/deser/latest/deser/io/"
)]
#![cfg_attr(
    not(feature = "derive"),
    doc = "[derive-module]: https://docs.rs/deser/latest/deser/derive/"
)]
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![no_std]

// Everything but the derive macros lives in deser-core, so that the crates
// of the data formats (which only depend on deser-core) can be compiled in
// parallel with the derive macros.  The items are inlined so that the
// documentation shows them as part of this crate.  New public items of
// deser-core need to be added here (`tests/test_facade.rs` checks this).

#[doc(inline)]
pub use deser_core::{adapters, de, ext, hints, ser, stream};

#[cfg(feature = "io")]
#[doc(inline)]
pub use deser_core::io;

#[cfg(feature = "derive")]
#[doc(inline)]
pub use deser_core::derive;

#[doc(inline)]
pub use deser_core::{
    Atom, Bytes, BytesFormat, ContainerShape, Context, Error, ErrorAttachment, ErrorCategory,
    ErrorContext, ErrorKind, Event, EventData, Implicit, ImplicitValue, Order, Position, Source,
    State, Text,
};

// common re-exports

#[doc(no_inline)]
pub use crate::{de::Deserialize, ser::Serialize, stream::Streamed};

#[cfg(feature = "derive")]
#[doc(inline)]
pub use deser_derive::{Deserialize, Serialize};

#[cfg(feature = "open-enums")]
#[doc(inline)]
pub use deser_core::{DuplicateVariant, OpenEnum, OpenEnums, OpenVariant};

#[cfg(feature = "open-enums")]
#[doc(inline)]
pub use deser_derive::{open_enum, variant};

// Everything the code generated by the derive refers to.  Not public API.
// The macros of deser-core refer to deser-core itself (`$crate`).
#[cfg(feature = "derive")]
#[doc(hidden)]
pub use deser_core::__derive;

#[cfg(doctest)]
mod soundness;

#[cfg(all(doctest, feature = "derive"))]
mod derive_errors;

#[cfg(all(doctest, feature = "open-enums"))]
mod open_enum_errors;
