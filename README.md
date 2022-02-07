<div align="center">
 <img src="https://github.com/mitsuhiko/deser/raw/main/deser/artwork/logo.svg?raw=true" width="250" height="250">
 <p><strong>deser: an experimental serialization and deserialization library for Rust</strong></p>
</div>

[![Crates.io](https://img.shields.io/crates/d/deser.svg)](https://crates.io/crates/deser)
[![License](https://img.shields.io/github/license/mitsuhiko/deser)](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
[![Documentation](https://docs.rs/deser/badge.svg)](https://docs.rs/deser)

Deser is an experimental serialization system for Rust.  It wants to explore the
possibilities of serialization and deserialization of structural formats such as
JSON or msgpack.  It intentionally does not desire to support non self
describing formats such as bincode.

**This is not a production ready yet.**

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

This generates out the necessary
[`Serialize`](https://docs.rs/deser/latest/deser/ser/trait.Serialize.html) and
[`Deserialize`](https://docs.rs/deser/latest/deser/de/trait.Deserialize.html)
implementations.

To see what this looks like behind the scenes there are two examples
that show how structs are implemented:

* [derive](https://github.com/mitsuhiko/deser/tree/main/examples/derive): shows an example using automatic deriving
* [manual-struct](https://github.com/mitsuhiko/deser/tree/main/examples/manual-struct): shows the same example with a manual implementation

## Design Goals

* **Fast Compile Times:** deser avoids excessive monomorphization by encouraging dynamic dispatch.
* **Unlimited Recursion:** the real world is nasty and incoming data might be badly nested.
  Do not exhaust the call stack no matter how deep your data is.
  It accomplishes this by an alternative trait design to serde where
  handles to "sinks" or "serializable" objects are returned.  This
  means that it's up to the caller to manage the recursion.
* **Simple Data Model:** deser simplifies the data model on the serialization
  and deserialization interface.  For instance instead of making a distinction
  between `u8` and `u64` they are represented the same in the model.  To compensate
  for this, it provides type descriptors that provide auxiliary information for
  when a serializer wants to process it.
* **Meta Information:** deser compensates the simplified data model with providing
  a space to hold meta information.  This for instance can be used to automatically
  keep track of the "path" to the current structure during serialization and
  deserialization.
* **Native Byte Serialization:** deser has built-in specialization for serializing
  bytes and byte vectors as distinct formats from slices and vectors.

## Future Plans

* **Extensible Data Model:** deser wants to make it possible to extend the data
  model with types that are not native to the serialization interface.  For
  instance if a data format wants to support arbitrarily sized integers this
  should be possible without falling back to in-band
  signalling.

## Crates

* [deser](https://github.com/mitsuhiko/deser/tree/main/deser): the core crate
  providing the base functionality
* [deser-path](https://github.com/mitsuhiko/deser/tree/main/deser-path): a crate
  that extends deser to track the path during serialization
* [deser-debug](https://github.com/mitsuhiko/deser/tree/main/deser-debug): formats
  a serializable to the `std::fmt` debug format

## Inspiration

This crate heavily borrows from
[`miniserde`](https://github.com/dtolnay/miniserde),
[`serde`](https://serde.rs/) and [Sentry Relay's meta
system](https://github.com/getsentry/relay).  The general trait design was
modelled after `miniserde`.

## Safety

Deser (currently) uses excessive amounts of unsafe code internally.  It is not vetted and
it is likely completely wrong.  If this design turns out to be useful there will be need
to be a re-design of the internals.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)

