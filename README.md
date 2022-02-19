<div align="center">
 <img src="https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg" width="250" height="250">
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

To see some practical examples of this have a look at the
[examples](https://github.com/mitsuhiko/deser/tree/main/examples).

## Design Goals

* **Fast Compile Times:** deser avoids excessive monomorphization by encouraging
  dynamic dispatch.  The goal is to avoid generating a lot of duplicate code that
  produces bloat the compiler needs to churn through.
* **Simple Data Model:** deser simplifies the data model on the serialization
  and deserialization interface.  For instance instead of making a distinction
  between `u8` and `u64` they are represented the same in the model.  To compensate
  for this, it provides type descriptors that provide auxiliary information for
  when a serializer wants to process it.  This helps with compile times and makes
  using the crate easier.
* **Native Bytes Support:** deser has built-in specialization for serializing
  bytes and byte vectors.  A `Vec<u8>` is serialized as bytes and does not need
  special handling for text-only formats such as JSON.
* **Unlimited Recursion:** the real world is nasty and incoming data might be
  badly nested.  Deser does not exhaust the call stack no matter how deep your
  data is.  It accomplishes this by an alternative trait design to serde where
  handles to "sinks" or "serializable" objects are returned.  This means that
  it's up to the caller to manage the recursion.
* **Native Optionals:** the serialization system has a built-in understanding of
  the concept of optional data.  This means that with a single attribute a struct
  serializer can skip over all fields currently set to null.
* **Native Flattening Support:** deser's serialization and deserialization support
  has native support for flattening of structs.  This means no internal buffering
  is required for `#[deser(flatten)]`.
* **Stateful Processing:** deser compensates the simplified data model with providing
  a space to hold meta information.  Out of the box it provides information
  about the types that are being serialized.  The additional space can be used
  to keep track of the "path" to the current structure during serialization and
  deserialization.  (See [deser-path](https://docs.rs/deser-path/) for a
  practical example)

Deser does not intend on replacing serde but it attempts to address some if it's
shortcomings.  For more information there is a document about [Serde
Learnings](https://github.com/mitsuhiko/deser/blob/main/SERDE.md) with
more details.

## Future Plans

* **Extensible Data Model:** deser wants to make it possible to extend the data
  model with types that are not native to the serialization interface.  For
  instance if a data format wants to support arbitrarily sized integers this
  should be possible without falling back to in-band
  signalling.

## Known Limitations

The current design of this system is very allocation heavy.  This is the consequence
of a certain level of flexibility paired with the dynamic dispatch nature.  For instance
for JSON parsing, Serde is more than 3 times faster than Deser and for deserialization
2.5 times.

## Crates

* [deser](https://github.com/mitsuhiko/deser/tree/main/deser): the core crate
  providing the base functionality
* [deser-json](https://github.com/mitsuhiko/deser/tree/main/deser-json): basic
  JSON implementation for deser
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

