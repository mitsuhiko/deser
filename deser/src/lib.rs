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
//! serialized and derserialized automatically:
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
//! * [`deser-msgpack`](https://docs.rs/deser-msgpack): implements MessagePack
//!   serialization and deserialization.
//! * [`deser-toml`](https://docs.rs/deser-toml): implements TOML serialization and
//!   deserialization.
//! * [`deser-yaml`](https://docs.rs/deser-yaml): implements YAML serialization and
//!   deserialization.
//! * [`deser-urlencoded`](https://docs.rs/deser-urlencoded): implements query string
//!   and form data (`application/x-www-form-urlencoded`) serialization and
//!   deserialization.
//! * [`deser-csv`](https://docs.rs/deser-csv): implements CSV, TSV and other
//!   delimited text serialization and deserialization.
//! * [`deser-env`](https://docs.rs/deser-env): implements reading
//!   configuration from environment variables (and writing values into
//!   them).
//!
//! The data formats have a deserializer (which deserializes values from a
//! slice) and a serializer (which serializes values into a buffer).  Values
//! can also be read from and written to streams (such as files or sockets),
//! see [`io`][io-module].
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
//!   like their [`Debug`](std::fmt::Debug) implementation would.
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
//! * `jiff`, `chrono`, `time`, `uuid`, `rust_decimal`, `bigdecimal` and `num-bigint`
//!   implement [`Serialize`] and [`Deserialize`] for the types of these crates.  They
//!   are serialized as [well-known types](crate::ext#well-known-types) which data
//!   formats can support natively.
//! * `indexmap`, `hashbrown`, `smallvec`, `arrayvec`, `bytes` and `bstr` implement
//!   [`Serialize`] and [`Deserialize`] for the collections and byte buffers of these
//!   crates.  They behave like their counterparts in the standard library, including
//!   the [adapters](crate::adapters) (for instance `IndexMap<_, DisplayFromStr>`).
//! * `io` (enabled by default) adds [`io`][io-module] to read values from and
//!   write values to streams.
//!
#![cfg_attr(feature = "derive", doc = "[derive-module]: crate::derive")]
#![cfg_attr(feature = "io", doc = "[io-module]: crate::io")]
#![cfg_attr(
    not(feature = "io"),
    doc = "[io-module]: https://docs.rs/deser/latest/deser/io/"
)]
#![cfg_attr(
    not(feature = "derive"),
    doc = "[derive-module]: https://docs.rs/deser/latest/deser/derive/"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Everything but the derive macros lives in deser-core, so that the crates
// of the data formats (which only depend on deser-core) can be compiled in
// parallel with the derive macros.  The items are inlined so that the
// documentation shows them as part of this crate.  New public items of
// deser-core need to be added here.

#[doc(inline)]
pub use deser_core::{adapters, de, ext, hints, ser};

#[cfg(feature = "io")]
#[doc(inline)]
pub use deser_core::io;

#[cfg(feature = "derive")]
#[doc(inline)]
pub use deser_core::derive;

#[doc(inline)]
pub use deser_core::{
    Atom, Bytes, ContainerShape, Error, ErrorAttachment, ErrorContext, ErrorKind, Event, EventData,
    Order, Position, State, Streamed,
};

#[doc(inline)]
pub use deser_core::make_slot_wrapper;

// common re-exports

#[doc(no_inline)]
pub use crate::{de::Deserialize, ser::Serialize};

#[cfg(feature = "derive")]
#[doc(inline)]
pub use deser_derive::{Deserialize, Serialize};

// Everything the code generated by the derive and the macros refer to.  Not
// public API.
#[cfg(feature = "derive")]
#[doc(hidden)]
pub use deser_core::__derive;
#[doc(hidden)]
pub use deser_core::__make_slot_wrapper;

#[cfg(doctest)]
mod soundness;

#[cfg(all(doctest, feature = "derive"))]
mod derive_errors;
