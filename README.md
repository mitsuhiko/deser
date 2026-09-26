<div align="center">
 <img src="https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg" width="250" height="233">
 <p><strong>deser: an experimental serialization and deserialization library for Rust</strong></p>
</div>

[![Crates.io](https://img.shields.io/crates/d/deser.svg)](https://crates.io/crates/deser)
[![License](https://img.shields.io/github/license/mitsuhiko/deser)](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
[![Documentation](https://docs.rs/deser/badge.svg)](https://docs.rs/deser)

Deser is an experimental serialization system for Rust for self describing
formats such as JSON, YAML, TOML, CBOR and query strings.  If you know serde you will feel at
home: you derive `Serialize` and `Deserialize` on your types and pick a format
crate.  What deser does differently is what happens when data gets messy:

* **Errors point at the problem:** with line, column and the path to the
  value (`servers[1].timeout`), also inside internally tagged and untagged
  enums which have to buffer.
* **No stack overflows:** deeply nested (or hostile) input does not recurse
  on the call stack, and limits for untrusted input are a layer away.
* **Bytes, dates, UUIDs and big numbers just work:** they are native where
  the format supports them (CBOR byte strings, TOML date-times) and fall
  back to strings everywhere else, without in-band signalling.
* **Text of unknown type is parsed by its type:** where the format cannot
  say what a value is (query strings, the keys of JSON objects), the type it
  is deserialized into decides, also in flattened structs and tagged enums.
* **Hooks between format and types:** layers see every value and can track
  paths, rename keys, redact values or reject input, without support from
  the format or your types.
* **Fast to compile:** the derive generates little code and relies on
  dynamic dispatch instead of monomorphizing everything.

It intentionally does not support non self describing formats such as
bincode.

**This is not production ready yet.**

```rust
use deser::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct Account {
    id: u64,
    account_holder: String,
    #[deser(default)]
    is_deactivated: bool,
}

let account: Account = deser_json::from_str(r#"{"id": 42, "accountHolder": "Jane"}"#).unwrap();
assert_eq!(account.account_holder, "Jane");
assert_eq!(
    deser_json::to_string(&account).unwrap(),
    r#"{"id":42,"accountHolder":"Jane","isDeactivated":false}"#
);
```

The same type works unchanged with
[`deser-yaml`](https://docs.rs/deser-yaml),
[`deser-toml`](https://docs.rs/deser-toml) and
[`deser-cbor`](https://docs.rs/deser-cbor).  Deriving requires the `derive`
feature, which is not enabled by default:

```toml
[dependencies]
deser = { version = "0.8", features = ["derive"] }
deser-json = "0.8"
```

## Reading and Writing

Every format has the same pieces:

* Functions for single values: `from_str`, `from_slice` and `to_string`
  (`to_vec` for CBOR).  Options are set on a `DeserializerConfig` or
  `SerializerConfig`, which have the same methods.
* A `Deserializer` which reads one value after another from a slice, and a
  `Serializer` which writes more than one value (JSON Lines, CBOR
  sequences, YAML documents).
* With the `io` feature (enabled by default): `from_reader` and
  `to_writer` for `std::io`, and `deser::io::Reader` and `deser::io::Writer`
  for streams of values.  They only buffer what they need: JSON and CBOR
  are parsed while the input arrives, and a `deser::Streamed<T>` sequence
  hands out its elements one by one.
  [`deser-tokio`](https://docs.rs/deser-tokio) does the same with tokio.

```rust
use deser::io::Reader;
use deser_json::{DeserializerConfig, Trailing};

const LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);

// reads one event per line, a line that fails does not end the stream
let mut events = Reader::new(std::io::stdin(), LINES);
while let Some(event) = events.read::<Event>()? {
    handle(event);
}
```

## Errors That Help

Consider a config file with an internally tagged enum where the tag comes
last.  To deserialize it the values have to be buffered until the tag is
known.  In serde this is where locations and paths get lost, in deser they
are retained:

```rust
use deser::Deserialize;
use deser_path::{Path, PathLayer};

#[derive(Debug, Deserialize)]
struct Config {
    servers: Vec<Server>,
}

#[derive(Debug, Deserialize)]
#[deser(tag = "type", rename_all = "lowercase")]
enum Server {
    Http { url: String, timeout: u32 },
    File { path: String },
}

let toml = r#"
[[servers]]
type = "file"
path = "/srv/www"

[[servers]]
url = "https://example.com/"
timeout = "30s"
type = "http"
"#;

let err = deser_toml::Deserializer::from_str(toml)
    .deserialize_with::<Config, _>(|driver| driver.push_layer(PathLayer::new()))
    .unwrap_err();
assert_eq!(err.attachment::<Path>().unwrap().to_string(), "servers[1].timeout");
assert_eq!((err.line(), err.column()), (Some(8), Some(11)));

// Unexpected: unexpected string, expected u32 at line 8 column 11 (path: servers[1].timeout)
println!("{err}");
```

Swap `deser_toml` for `deser_yaml` or `deser_json` and you get the same
quality of errors.  To see more practical examples have a look at the
[examples](https://github.com/mitsuhiko/deser/tree/main/examples).

## Design Goals

* **Fast Compile Times:** deser avoids excessive monomorphization by encouraging
  dynamic dispatch.  The goal is to avoid generating a lot of duplicate code that
  produces bloat the compiler needs to churn through.
* **Simple Data Model:** deser simplifies the data model on the serialization
  and deserialization interface.  For instance instead of making a distinction
  between `u8` and `u64` they are represented the same in the model.  Formats
  that need more information can get it: maps and sequences carry their shape
  (the order and number of elements) and values can describe their Rust shape
  (such as struct and variant names, which `deser-debug` uses).  This helps
  with compile times and makes using the crate easier.
* **Native Bytes Support:** deser has built-in specialization for serializing
  bytes and byte vectors.  A `Vec<u8>` is serialized as bytes in formats which
  support them (such as CBOR) and as base64 in text-only formats such as JSON
  without special handling.  Other encodings (such as hex) can be picked per
  field or per format.
* **Borrowing:** types can borrow strings and bytes from the data they are
  deserialized from (for instance `&str` fields), formats pass on slices of
  their input without copying them.
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
* **Lossless Buffering:** where buffering cannot be avoided (for instance for
  internally tagged enums where the tag does not come first) values are
  recorded as events together with the state the format published for each
  event (such as source locations) and replayed as such.  Format specific
  behavior like integer map keys in JSON, extension values or location
  tracking keeps working for buffered values.
* **Extensible Data Model:** the data model can be extended with types that are
  not native to the serialization interface through extension atoms.  Every
  extension value carries a fallback into the core data model.  Serializers
  and deserializers that understand an extension handle it natively (for
  instance `deser-json` supports 128 bit integers this way), everybody else
  transparently gets the fallback.  This avoids in-band signalling.  (See
  [ext](https://docs.rs/deser/latest/deser/ext/) for more information)
* **Stateful Processing:** serialization and deserialization carry a state
  in which formats, layers and types keep information.  Formats publish the
  location of every event in it, and data attached to events (such as tags or
  formatting hints) travels with the events through the state.  Layers can use
  it to keep track of the "path" to the current value.  (See
  [deser-path](https://docs.rs/deser-path/) for a practical example)
* **Layers:** layers sit between the data format and the types and see all
  events.  They can observe, reject and rewrite events without support by
  the format or the types, for instance to track the path, to limit the size
  of untrusted input or to rename keys in the output.  Errors carry the
  location in the input and (with the path layer) the path of the value
  they refer to, also for values which are buffered.

Deser does not intend on replacing serde but it attempts to address some of its
shortcomings.  For more information there is a document about [Serde
Learnings](https://github.com/mitsuhiko/deser/blob/main/SERDE.md) with
more details.

## Known Limitations

The current design of this system relies on dynamic dispatch and heap allocated
sinks and emitters for many compound values.  This is the consequence of a
certain level of flexibility and the desire to not use the call stack for
recursion.  Deser works around most of this overhead (for instance derived
structs and vectors serialize without allocations, and sinks are allocated
from a per thread cache) and for JSON it is roughly on par with Serde in the
included benchmark.

## Crates

* [deser](https://github.com/mitsuhiko/deser/tree/main/deser): the core crate
  providing the base functionality
* [deser-derive](https://github.com/mitsuhiko/deser/tree/main/deser-derive):
  the derive macros, use them through the `derive` feature of `deser`
* [deser-json](https://github.com/mitsuhiko/deser/tree/main/deser-json): JSON
  implementation for deser
* [deser-cbor](https://github.com/mitsuhiko/deser/tree/main/deser-cbor): CBOR
  implementation for deser with support for tags
* [deser-toml](https://github.com/mitsuhiko/deser/tree/main/deser-toml): TOML 1.1
  implementation for deser
* [deser-yaml](https://github.com/mitsuhiko/deser/tree/main/deser-yaml): YAML
  implementation for deser
* [deser-urlencoded](https://github.com/mitsuhiko/deser/tree/main/deser-urlencoded):
  query strings and form data (`application/x-www-form-urlencoded`) for deser
* [deser-path](https://github.com/mitsuhiko/deser/tree/main/deser-path): a layer
  that tracks the path during serialization and deserialization and adds it
  to errors
* [deser-location](https://github.com/mitsuhiko/deser/tree/main/deser-location): a
  crate that provides source locations (line and column) for formats that
  support them
* [deser-debug](https://github.com/mitsuhiko/deser/tree/main/deser-debug): formats
  a serializable to the `std::fmt` debug format
* [deser-tokio](https://github.com/mitsuhiko/deser/tree/main/deser-tokio): reads
  and writes values of all formats with tokio (for instance JSON Lines or
  CBOR sequences on sockets)
* [deser-value](https://github.com/mitsuhiko/deser/tree/main/deser-value): a
  dynamic value type which retains extension values, tags, formatting hints
  and source locations
* [deser-serde](https://github.com/mitsuhiko/deser/tree/main/deser-serde): adapters
  to serialize and deserialize types with their serde implementations
  (`#[deser(as = Serde)]`)

## Inspiration

This crate heavily borrows from
[`miniserde`](https://github.com/dtolnay/miniserde),
[`serde`](https://serde.rs/) and [Sentry Relay's meta
system](https://github.com/getsentry/relay).  The general trait design was
modelled after `miniserde`.

## Safety

Deser needs unsafe code internally, primarily to erase lifetimes in the drivers
which keep the chain of borrowed sinks and emitters on the heap rather than the
call stack.  The unsafe code is documented, has a dedicated test suite that
exercises it (partial drops, errors, panics, deep nesting) and the test suites of
all crates are run under [miri](https://github.com/rust-lang/miri) with both
stacked and tree borrows (`make miri-test`).  This does not guarantee soundness
but if you find a soundness issue, please report it.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)

