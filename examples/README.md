# Examples

Every example is a small binary which can be run with `cargo run -p NAME`
from the root of the repository.  Each example directory has a `README.md`
that explains why the example exists, what it shows, what output to
expect and where to start reading the code.  Most examples `assert!` what
they print, so a clean exit means they behaved as documented.

Getting started:

* [`json`](json): deriving `Serialize` and `Deserialize`, renaming,
  flattening, optional fields and debug formatting with `deser-debug`.
* [`enums`](enums): the enum representations (externally, internally and
  adjacently tagged, untagged), catch-all variants, untagged fallback
  variants and flattened fields in variants.
* [`formats`](formats): one type written as JSON, YAML, TOML, CBOR and
  MessagePack, with UUIDs, timestamps and dates handled natively where a
  format supports them, and layout hints.
* [`adapters`](adapters): `#[deser(as = ...)]` to serialize types on
  behalf of others, composed with containers, and adapters that tolerate
  errors.
* [`optionals`](optionals): skipping optional fields and telling missing
  values apart from null for partial updates.
* [`debug`](debug): formatting values like `#[derive(Debug)]` with
  `deser-debug`, keeping the Rust shape of options, tuple structs, unit
  structs, newtypes and enums.
* [`query-strings`](query-strings): query strings and HTML forms with
  `deser-urlencoded`, with numbers that parse in flattened structs and
  tagged enums.
* [`csv`](csv): reading a CSV export row by row with `deser-csv`,
  flattened tagged enums, lists in a field, per-row errors and writing
  CSV and TSV.
* [`xml-namespaces`](xml-namespaces): an Atom feed with Dublin Core,
  Media RSS and XHTML with `deser-xml`: names in namespaces that do not
  depend on the prefixes of the document, mixed content, unknown elements
  kept as dynamic values and writing with chosen or generated prefixes.
* [`xml-pretty`](xml-pretty): an SVG drawing written as indented XML
  with `deser-xml`, with namespaces declared on the root, mixed content
  that stays on a single line and compact sequences.
* [`env`](env): configuration from environment variables with
  `deser-env`: nested keys, lists, flags, tagged enums and errors that name
  the variable.
* [`renames`](renames): renaming keys of a TOML file without breaking old
  files or other programs, with aliases, different names for reading and
  writing and keys that are only written.
* [`input-contracts`](input-contracts): what input is accepted and how
  errors read, with transparent structs, `expecting`, required options
  and unknown fields denied for a single variant.

What sets deser apart:

* [`borrowing`](borrowing): borrowing strings and bytes from the input
  with `&str`, `&[u8]` and `Cow`, in structs and enums.
* [`config`](config): layered configuration (defaults, files,
  environment variables and command line overrides) with updates,
  validation and warnings for unknown keys.
* [`bytes`](bytes): bytes in JSON and TOML (base64, hex, arrays of
  integers) configured per format and per field, and native bytes in CBOR
  and MessagePack.
* [`deep-nesting`](deep-nesting): a million levels of nesting without
  overflowing the stack, and limits for untrusted input.
* [`config-errors`](config-errors): errors with line, column and path,
  also for buffered values, in TOML and YAML.
* [`validation`](validation): validating while deserializing with
  `deser-validate`, forms that keep invalid values with their errors and
  API requests rejected with a report of all problems.
* [`protocol`](protocol): a wire protocol with integer tags, enums named
  by their discriminants, tag aliases and forwarding of unknown messages
  without losing data.
* [`json-lines`](json-lines): reading JSON Lines with per-line error
  recovery and writing them.
* [`json-numbers`](json-numbers): exact decimal numbers and timestamps in
  JSON.
* [`streams`](streams): reading and writing files and streams of values
  (JSON Lines, CBOR sequences, YAML documents) with `std::io`.
* [`tokio-server`](tokio-server): a JSON Lines server and client with
  tokio, using `deser-tokio`'s reader, writer and codec.

Other crates:

* [`dynamic-values`](dynamic-values): inspecting, transforming and
  converting data with `deser-value`, keeping unknown fields and merging
  files with errors that point into the right file.
* [`serde-types`](serde-types): using types that only implement serde
  (like `semver::Version` and `serde_json::Value`) with `deser-serde`.

Advanced:

* [`layers`](layers): layers that rename keys, skip nulls and redact values
  during serialization, and the built-in limits and path layers.
* [`located`](located): annotating values with their path and source
  location through extension values and the state, surviving buffering.
* [`manual-struct`](manual-struct): implementing `Serialize` and
  `Deserialize` by hand.
* [`crate-path`](crate-path): using the derive when deser is renamed or
  re-exported.
* [`no-std`](no-std): a library that does not use the standard library
  (only `alloc`) and builds for targets without an operating system.
* [`generic-types`](generic-types): custom bounds for type parameters
  that are not serialized themselves (on types and fields), and enums with
  lifetime and const parameters.
