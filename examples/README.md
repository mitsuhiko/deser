# Examples

Every example is a small binary which can be run with `cargo run -p NAME`
from the root of the repository.

Getting started:

* [`json`](json): deriving `Serialize` and `Deserialize`, renaming,
  flattening, optional fields and debug formatting with `deser-debug`.
* [`enums`](enums): the enum representations (externally, internally and
  adjacently tagged, untagged) and catch-all variants.
* [`formats`](formats): one type written as JSON, YAML, TOML and CBOR,
  with UUIDs, timestamps and dates handled natively where a format
  supports them, and layout hints.
* [`adapters`](adapters): `#[deser(as = ...)]` to serialize types on
  behalf of others, composed with containers, and adapters that tolerate
  errors.
* [`optionals`](optionals): skipping optional fields and telling missing
  values apart from null for partial updates.
* [`debug`](debug): formatting values like `#[derive(Debug)]` with
  `deser-debug`, keeping the Rust shape of options, newtypes and enums.

What sets deser apart:

* [`borrowing`](borrowing): borrowing strings and bytes from the input
  with `&str`, `&[u8]` and `Cow`.
* [`bytes`](bytes): bytes in JSON and TOML (base64, hex, arrays of
  integers) configured per format and per field, and native bytes in CBOR.
* [`deep-nesting`](deep-nesting): a million levels of nesting without
  overflowing the stack, and limits for untrusted input.
* [`config-errors`](config-errors): errors with line, column and path,
  also for buffered values, in TOML and YAML.
* [`json-lines`](json-lines): reading JSON Lines with per-line error
  recovery and writing them.
* [`json-numbers`](json-numbers): exact decimal numbers and timestamps in
  JSON.

Advanced:

* [`layers`](layers): layers that rename keys, skip nulls and redact values
  during serialization, and the built-in limits and path layers.
* [`located`](located): annotating values with their path and source
  location through extension values and the state, surviving buffering.
* [`manual-struct`](manual-struct): implementing `Serialize` and
  `Deserialize` by hand.
* [`crate-path`](crate-path): using the derive when deser is renamed or
  re-exported.
