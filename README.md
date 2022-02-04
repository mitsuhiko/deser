# deser

Deser is an experimental serialization system for Rust.  It wants to explore the
possibilities of serialization and deserialization of structural formats such as
JSON or msgpack.  It intentionally does not desire to support non self describing
formats such as bincode.

## Design Goals

* **Fast Compile Times:** deser avoids excessive monomorphization by encouraging dynamic dispatch.
* **Unlimited Recursion:** the real world is nasty and incoming data might be badly nested.
  Do not exhaust the call stack no matter how deep your data is.
* **Simple Data Model:** deser simplifies the data model on the serialization and deserialization
  interface but compensates by providing descriptors that provide auxiliary information for when
  a serializer wants to process it.
* **Extensible Data Model:** deser wants to make it possible to extend the data model with types
  that are not native to the serialization interface.  For instance if a data format wants to
  support arbitrarily sized integers this should be possible without falling back to in-band
  signalling.
* **Meta Information:** deser compensates the simplified data model with providing a space to
  hold meta information.
* **Native Byte Serialization:** deser has built-in specialization for serializing bytes and
  byte vectors as distinct formats from slices and vectors.

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

Deser (currently) uses excessive amounts of unsafe code internally.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)

