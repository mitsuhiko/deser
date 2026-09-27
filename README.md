<div align="center">
 <img src="https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg" width="250" height="233">
 <p><strong>deser: an experimental serialization and deserialization library for Rust</strong></p>
</div>

[![Crates.io](https://img.shields.io/crates/d/deser.svg)](https://crates.io/crates/deser)
[![License](https://img.shields.io/github/license/mitsuhiko/deser)](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
[![Documentation](https://docs.rs/deser/badge.svg)](https://docs.rs/deser)

Deser is a serialization library for Rust for self describing formats such as
JSON, YAML, TOML, CBOR, MessagePack, CSV and query strings.  It takes the user experience of
serde, the problems that years of running serde in production turned up and the
Rust of today, and tries to solve them with a different architecture.  If you
know serde you will feel at home: you derive `Serialize` and `Deserialize` on
your types, pick a format crate, and most attributes have the names you already
know.

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

let json = r#"{"id": 42, "accountHolder": "Jane"}"#;
let account: Account = deser_json::from_str(json).unwrap();
assert_eq!(account.account_holder, "Jane");
assert_eq!(
    deser_json::to_string(&account).unwrap(),
    r#"{"id":42,"accountHolder":"Jane","isDeactivated":false}"#
);
```

The same type works unchanged with
[`deser-yaml`](https://docs.rs/deser-yaml),
[`deser-toml`](https://docs.rs/deser-toml),
[`deser-cbor`](https://docs.rs/deser-cbor),
[`deser-msgpack`](https://docs.rs/deser-msgpack),
[`deser-urlencoded`](https://docs.rs/deser-urlencoded) and (as long as it's
flat) [`deser-csv`](https://docs.rs/deser-csv).  Deriving requires the `derive`
feature, which is not enabled by default:

```sh
cargo add deser --features derive
cargo add deser-json
```

## Why Deser?

Serde is one of the most important crates in the Rust ecosystem and its
stability is a big part of why.  That same stability also means that some of its
problems cannot be fixed: they are a consequence of the data model and the trait
design, and changing those would break every format and every hand written
implementation out there.  Many of the issues below have been open for years.
Deser is an experiment to see what an alternative serialization system looks
like that is allowed to start over.

Things it can fix that is tricky for Serde to address:

* **Buffering does not lose information.**  Internally tagged enums whose
  tag is not first, untagged enums and (in serde) flattened fields have to
  buffer values.  In serde that buffer is a lossy copy of the data model:
  `u128` stops working, `arbitrary_precision` numbers turn into maps,
  numeric map keys and numbers in query strings no longer parse and errors
  lose their line and column.  In deser values are recorded as events
  together with everything the format knows about them, and replayed as
  such.
* **Flattening is native.**  `#[deser(flatten)]` does not buffer at all.
  Flattened fields take the keys they know, a flattened map receives
  exactly the keys nobody else took, `deny_unknown_fields` works with
  flattened structs and internally tagged enums, and errors in a flattened
  `Option` are reported instead of silently becoming `None`.
* **No stack overflows.**  Deser does not recurse on the call stack, so a
  million levels of nesting deserialize, serialize and get skipped without
  a recursion limit.  If you want a limit for untrusted input it's a
  layer, not a hard coded constant.
* **Enums are more capable.**  Catch-all variants can hold data and capture
  the unknown tag so values round trip, a default variant can be picked if
  the tag is missing, tags can be integers and booleans and
  `#[deser(repr)]` uses the discriminants.
* **Customizations compose.**  `#[deser(as = Option<Vec<DisplayFromStr>>)]`
  works without writing another function, missing fields stay optional,
  in-place updates and unknown field collection are built in, and
  validation ([`deser-validate`](https://docs.rs/deser-validate)) is an
  adapter as well: `#[deser(as = Check<NonZero>)]`.

[SERDE.md](https://github.com/mitsuhiko/deser/blob/main/SERDE.md) goes
through these in detail and links the open serde issues they correspond to.

Deser also starts the design against a modern Rust baseline.  Serde 1.0 was
released in 2017 and its design reflects the Rust of that time.  Deser can take
advantage of what the language has gained since:

* **Attributes are Rust, not strings.**  Defaults are expressions
  (`default = 8080`), names can be constants or `concat!(...)`, adapters are
  types (`as = BTreeMap<_, DisplayFromStr>`) and bounds and paths are
  written as code.  The compiler checks them and your
  editor can navigate them.
* **Const generics everywhere.**  Arrays of any length and `NonZero<T>` work
  out of the box, as do newer standard library types like `OnceLock` and
  `Infallible`.
* **Ready for async.**  An ongoing deserialization is `Send`, can be held
  across calls and fed while the input arrives.  JSON, CBOR and MessagePack
  are parsed as the bytes come in and only incomplete tokens are buffered.
  [`deser-tokio`](https://docs.rs/deser-tokio) reads and writes streams of
  values on sockets.

## It Comes From Experience

The problems deser addresses are not hypothetical.  Many of them came up while
building [Sentry Relay](https://github.com/getsentry/relay), which processes
untrusted JSON at scale, and some of the serde issues about them were filed by
the author of this crate ([#1183](https://github.com/serde-rs/serde/issues/1183)
in 2018, [#1463](https://github.com/serde-rs/serde/issues/1463) in 2019).  That
shows in the design:

* **Errors point at the problem:** with line, column and the path to the
  value (`servers[1].timeout`), also inside buffered values.
* **Hooks between the format and your types:** layers see every value and
  can track paths, enforce limits, rename keys, redact values or reject
  input, without support from the format or your types.
* **State flows through the whole process:** formats publish source
  locations and formatting hints, layers and types can read and attach
  information, and it all survives buffering.
* **Safe defaults for untrusted input:** duplicate keys are an error by
  default (different parsers picking different values for the same input
  is a security problem), and limits for depth, size and length are a
  layer away.
* **Fast to compile:** the derive generates comparatively little code and
  relies on dynamic dispatch instead of monomorphizing everything (see
  [compile-times](https://github.com/mitsuhiko/deser/tree/main/compile-times)).

## Foundation For Coding Agents

Deser is designed so that both you and the coding agents you work with can
reason about it, whether they use it in your code or work on deser itself:

* **A small, inspectable data model.**  Everything is a stream of events
  made of a handful of atoms.  You can feed events into a
  `DeserializeDriver` or pull them out of a `SerializeDriver` in a test and
  see exactly what happens, no visitor state machines involved.
* **Mistakes fail at compile time.**  Attributes are type checked Rust, and
  the derive rejects combinations that would silently have no effect (for
  instance `default` on a flattened field or field attributes on a type
  that is serialized through an adapter).
* **Errors an agent can act on:** an error kind, a message that names the
  type (``unknown variant `D` of Kind, expected `A` or `B` ``), a line and
  column and the path of the value.

## A Taste

A single enum that shows a few things that would need hand written code or
extra crates with serde:

```rust
use std::net::IpAddr;
use deser::{Deserialize, Serialize};
use deser::adapters::DisplayFromStr;
use deser::de::Recording;
use deser_validate::{Check, validator};

mod keys {
    pub const KIND: &str = "@type";
}

validator!(NonZero(port: &u16) => *port != 0, "must not be zero");

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = keys::KIND, tag_alias = "type", rename_all = "snake_case")]
pub enum Listener {
    // picked if the tag is missing
    #[deser(default)]
    Tcp {
        #[deser(as = DisplayFromStr)]
        host: IpAddr,
        #[deser(default = 8080, as = Check<NonZero>)]
        port: u16,
    },
    Unix { path: String },
    // kinds this version does not know are kept and written back
    #[deser(other)]
    Other(#[deser(tag)] String, Recording),
}

let listener: Listener =
    deser_json::from_str(r#"{"host": "127.0.0.1"}"#).unwrap();
assert!(matches!(listener, Listener::Tcp { port: 8080, .. }));

let input = r#"{"@type":"quic","host":"::1","alpn":["h3"]}"#;
let listener: Listener = deser_json::from_str(input).unwrap();
assert_eq!(deser_json::to_string(&listener).unwrap(), input);

let input = r#"{"host": "::1", "port": 0}"#;
let err = deser_json::from_str::<Listener>(input).unwrap_err();
assert_eq!(
    err.to_string(),
    "Unexpected: invalid value: must not be zero at line 1 column 25"
);
```

## Errors That Help

Consider a config file with an internally tagged enum where the tag comes last.
To deserialize it the values have to be buffered until the tag is known.  In
serde this is where locations and paths get lost, in deser they are retained:

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
    .deserialize_with::<Config, _>(|driver| {
        driver.push_layer(PathLayer::new())
    })
    .unwrap_err();
let path = err.attachment::<Path>().unwrap();
assert_eq!(path.to_string(), "servers[1].timeout");
assert_eq!((err.line(), err.column()), (Some(8), Some(11)));

// Unexpected: unexpected string, expected u32 at line 8 column 11
// (path: servers[1].timeout)
println!("{err}");
```

Swap `deser_toml` for `deser_yaml` or `deser_json` and you get the same quality
of errors.  To see more practical examples have a look at the
[examples](https://github.com/mitsuhiko/deser/tree/main/examples).

## Reading and Writing

Every format has the same pieces:

* Functions for single values: `from_str`, `from_slice` and `to_string`
  (`to_vec` for CBOR and MessagePack).  Options are set on a `DeserializerConfig` or
  `SerializerConfig`, which have the same methods.
* A `Deserializer` which reads one value after another from a slice, and a
  `Serializer` which writes more than one value (JSON Lines, CBOR
  sequences, MessagePack streams, YAML documents).
* With the `io` feature (enabled by default): `from_reader` and
  `to_writer` for `std::io`, and `deser::io::Reader` and `deser::io::Writer`
  for streams of values.  They only buffer what they need: JSON, CBOR and
  MessagePack are parsed while the input arrives, and a `deser::Streamed<T>` sequence
  hands out its elements one by one.
  [`deser-tokio`](https://docs.rs/deser-tokio) does the same with tokio.

```rust
use deser::io::Reader;
use deser_json::{DeserializerConfig, Trailing};

const LINES: DeserializerConfig =
    DeserializerConfig::new().trailing(Trailing::Newline);

// reads one event per line, a line that fails does not end the stream
let mut events = Reader::new(std::io::stdin(), LINES);
while let Some(event) = events.read::<Event>()? {
    handle(event);
}
```

## How It Works

* **Sinks and emitters instead of visitors:** deserializing a type creates
  a sink for a slot (`Option<T>`) that receives events, serializing it
  produces chunks and emitters that hand out the nested values.  Instead
  of calling into each other recursively, the nested sinks and emitters
  are returned to a driver which keeps them on the heap.  This is why
  nesting cannot overflow the stack, why a deserialization can be
  suspended between events and why updates of existing values
  (`deserialize_update`) and adapters (`SerializeAs` and `DeserializeAs`)
  fit into the same traits.
* **A small data model:** values are atoms (booleans, integers, floats,
  strings, bytes, ...), maps and sequences.  There is no distinction
  between `u8` and `u64` in the model.  Formats that need more information
  can get it: maps and sequences carry their shape (the order and number
  of elements) and values can describe their Rust shape (such as struct
  and variant names, which `deser-debug` uses).
* **Extension atoms:** the data model can be extended with types that are
  not native to it.  Every extension value carries a fallback into the
  core data model.  Serializers and deserializers that understand an
  extension handle it natively (for instance `deser-json` supports 128 bit
  integers and exact numbers this way), everybody else transparently gets
  the fallback.  (See [ext](https://docs.rs/deser/latest/deser/ext/) for
  more information)
* **Lexical atoms:** text whose type the format cannot express (the keys
  of JSON objects, the values of query strings) is passed as a lexical
  atom which the type it is deserialized into parses.  This keeps working
  in flattened structs and tagged enums.
* **Native bytes and optionals:** a `Vec<u8>` is bytes in formats which
  support them (such as CBOR and MessagePack) and base64 elsewhere, other encodings can be
  picked per field or per format.  Values know if they are optional, so a
  struct can skip all unset fields with a single attribute and
  `Option<Option<T>>` tells a missing value apart from null.
* **Borrowing:** types can borrow strings and bytes from the data they are
  deserialized from (for instance `&str` fields), formats pass on slices of
  their input without copying them.
* **Lossless buffering:** where buffering cannot be avoided values are
  recorded as events together with the state the format published for
  each event (such as source locations) and replayed as such.  A
  `Recording` can also be used as a raw value.
* **State and layers:** serialization and deserialization carry a state in
  which formats, layers and types keep information.  Layers sit between
  the data format and the types and see all events.  They can observe,
  reject and rewrite events, for instance to track the path (see
  [deser-path](https://docs.rs/deser-path/)), to limit the size of
  untrusted input or to rename keys in the output.

Deser intentionally does not support non self describing formats such as
bincode.  Supporting both kinds of formats with the same traits is the
source of a whole class of runtime failures and surprises in serde (see
[SERDE.md](https://github.com/mitsuhiko/deser/blob/main/SERDE.md)).

## Known Limitations

The current design of this system relies on dynamic dispatch and heap allocated
sinks and emitters for many compound values.  This is the consequence of a
certain level of flexibility and the desire to not use the call stack for
recursion.  Deser works around most of this overhead (for instance derived
structs and vectors serialize without allocations, and sinks are allocated
from a per thread cache).  Compared to serde based libraries in the
[included benchmark](https://github.com/mitsuhiko/deser/tree/main/benchmark)
YAML and TOML are two to four times as fast and CBOR deserializes faster
but serializes slower.  JSON serializes faster but deserializes slower on
float heavy and deeply nested data, MessagePack is on par on string heavy
data and slower on float heavy and deeply nested data.

Serializables are `Sync`, deserializable types and sinks are `Send` so that
an ongoing serialization or deserialization can move between threads (for
instance while waiting for IO).  This means that types which are not thread
safe (`Rc`, `rc::Weak`, `Cell` and `RefCell`) cannot be serialized or
deserialized.  `Mutex` and `RwLock` can only be deserialized as serializing
them would have to hold the lock guard while the serialization moves between
threads.

## Crates

Core:

* [deser](https://github.com/mitsuhiko/deser/tree/main/deser): the
  crate you depend on, re-exports the core and the derive macros
* [deser-derive](https://github.com/mitsuhiko/deser/tree/main/deser-derive):
  derive macros (enabled with the `derive` feature of `deser`)
* [deser-core](https://github.com/mitsuhiko/deser/tree/main/deser-core): an
  internal crate with everything but the derive macros.  Format crates
  depend on it so they compile in parallel with the derive macros.  Use
  `deser` instead.

Formats:

* [deser-json](https://github.com/mitsuhiko/deser/tree/main/deser-json): JSON
* [deser-yaml](https://github.com/mitsuhiko/deser/tree/main/deser-yaml): YAML
* [deser-toml](https://github.com/mitsuhiko/deser/tree/main/deser-toml): TOML
* [deser-cbor](https://github.com/mitsuhiko/deser/tree/main/deser-cbor): CBOR
* [deser-msgpack](https://github.com/mitsuhiko/deser/tree/main/deser-msgpack): MessagePack
* [deser-csv](https://github.com/mitsuhiko/deser/tree/main/deser-csv): CSV and TSV
* [deser-urlencoded](https://github.com/mitsuhiko/deser/tree/main/deser-urlencoded): query strings and forms
* [deser-env](https://github.com/mitsuhiko/deser/tree/main/deser-env): environment variables
* [deser-debug](https://github.com/mitsuhiko/deser/tree/main/deser-debug): debug formatting

Layers and adapters:

* [deser-path](https://github.com/mitsuhiko/deser/tree/main/deser-path): paths in errors
* [deser-location](https://github.com/mitsuhiko/deser/tree/main/deser-location): line and column of values
* [deser-validate](https://github.com/mitsuhiko/deser/tree/main/deser-validate): validation
* [deser-encoding](https://github.com/mitsuhiko/deser/tree/main/deser-encoding): hex and base32 encodings of bytes

Integrations:

* [deser-value](https://github.com/mitsuhiko/deser/tree/main/deser-value): dynamic values
* [deser-tokio](https://github.com/mitsuhiko/deser/tree/main/deser-tokio): async IO with tokio
* [deser-serde](https://github.com/mitsuhiko/deser/tree/main/deser-serde): serde interop

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
- [Development Documentation](https://mitsuhiko.github.io/deser/) (all crates, built from `main`)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)
