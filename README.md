<div align="center">
 <picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo-dark.svg">
  <img src="https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg" alt="deser" width="250">
 </picture>
 <p><strong>deser: an experimental serialization and deserialization library for Rust</strong></p>
</div>

[![Crates.io](https://img.shields.io/crates/d/deser.svg)](https://crates.io/crates/deser)
[![License](https://img.shields.io/github/license/mitsuhiko/deser)](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
[![Documentation](https://docs.rs/deser/badge.svg)](https://docs.rs/deser)

Deser is a serialization library for Rust for self describing formats such as
JSON, YAML, TOML, CBOR, MessagePack, XML, property lists, CSV and query
strings.  It takes the user experience of serde, the problems that years of
running serde in production turned up and the Rust of today, and tries to
solve them with a different architecture.  If you know serde you will feel at
home: you derive `Serialize` and `Deserialize`, pick a format crate, and most
attributes have the names you already know.

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

Deriving requires the `derive` feature, which is not enabled by default:

```sh
cargo add deser --features derive
cargo add deser-json
```

## Crates

The same type works unchanged with every format (CSV as long as it's flat).

* **Core:** [deser](https://github.com/mitsuhiko/deser/tree/main/deser) (the crate you depend on),
  [deser-derive](https://github.com/mitsuhiko/deser/tree/main/deser-derive) (the `derive` feature) and
  [deser-core](https://github.com/mitsuhiko/deser/tree/main/deser-core) (internal, what format crates
  depend on)
* **Formats:**
  [deser-json](https://github.com/mitsuhiko/deser/tree/main/deser-json),
  [deser-jsonc](https://github.com/mitsuhiko/deser/tree/main/deser-jsonc),
  [deser-json5](https://github.com/mitsuhiko/deser/tree/main/deser-json5),
  [deser-hj](https://github.com/mitsuhiko/deser/tree/main/deser-hj),
  [deser-yaml](https://github.com/mitsuhiko/deser/tree/main/deser-yaml),
  [deser-toml](https://github.com/mitsuhiko/deser/tree/main/deser-toml),
  [deser-cbor](https://github.com/mitsuhiko/deser/tree/main/deser-cbor),
  [deser-msgpack](https://github.com/mitsuhiko/deser/tree/main/deser-msgpack),
  [deser-xml](https://github.com/mitsuhiko/deser/tree/main/deser-xml),
  [deser-plist](https://github.com/mitsuhiko/deser/tree/main/deser-plist) (XML, binary and OpenStep),
  [deser-csv](https://github.com/mitsuhiko/deser/tree/main/deser-csv) (CSV and TSV),
  [deser-urlencoded](https://github.com/mitsuhiko/deser/tree/main/deser-urlencoded) (query strings and forms),
  [deser-env](https://github.com/mitsuhiko/deser/tree/main/deser-env) (environment variables),
  [deser-debug](https://github.com/mitsuhiko/deser/tree/main/deser-debug) (debug formatting)
* **Layers and adapters:**
  [deser-path](https://github.com/mitsuhiko/deser/tree/main/deser-path) (paths in errors),
  [deser-location](https://github.com/mitsuhiko/deser/tree/main/deser-location) (line and column of values),
  [deser-validate](https://github.com/mitsuhiko/deser/tree/main/deser-validate) (validation),
  [deser-encoding](https://github.com/mitsuhiko/deser/tree/main/deser-encoding) (hex and base32)
* **Integrations:**
  [deser-value](https://github.com/mitsuhiko/deser/tree/main/deser-value) (dynamic values),
  [deser-transcode](https://github.com/mitsuhiko/deser/tree/main/deser-transcode) (converting between formats),
  [deser-tokio](https://github.com/mitsuhiko/deser/tree/main/deser-tokio) (async IO),
  [deser-serde](https://github.com/mitsuhiko/deser/tree/main/deser-serde) (serde interop)

## Why Deser?

Serde is one of the most important crates in the Rust ecosystem and its
stability is a big part of why.  That same stability also means that some of
its problems cannot be fixed without breaking every format and every hand
written implementation.  Deser is an experiment to see what a serialization
system looks like that is allowed to start over:

* **Buffering does not lose information.**  Internally tagged and untagged
  enums record values as events together with everything the format knows
  about them, so `u128`, exact numbers, numeric keys and error locations
  survive.
* **Flattening is native** and does not buffer at all, also with
  `deny_unknown_fields`.
* **No stack overflows.**  Deser does not recurse on the call stack, so deep
  nesting needs no recursion limit.
* **Enums are more capable.**  Catch-all variants can hold data and round
  trip, a default variant can be picked if the tag is missing and tags can
  be integers and booleans.
* **Customizations compose.**  Adapters nest
  (`as = Option<Vec<DisplayFromStr>>`) and validation is an adapter too.
* **Attributes are Rust, not strings.**  Defaults, names, adapters and bounds
  are expressions and types the compiler checks.
* **Ready for async.**  An ongoing deserialization is `Send` and can be fed
  while the input arrives.
* **Errors point at the problem** with line, column and the path to the
  value, also inside buffered values.  Their category tells malformed input
  apart from input that does not fit your types (HTTP 400 vs. 422).
* **Layers** sit between the format and your types and can track paths,
  rename keys or reject input.
* **Safe defaults for untrusted input:** duplicate keys are an error by
  default, limits for depth, size and length are configured once in a
  context.

Many of these came up while building
[Sentry Relay](https://github.com/getsentry/relay), which processes untrusted
JSON at scale.  [SERDE.md](https://github.com/mitsuhiko/deser/blob/main/SERDE.md)
goes through them in detail and links the serde issues they correspond to.

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
    "InvalidValue: invalid value: must not be zero at line 1 column 25"
);
```

## Errors That Help

Here the tag of an internally tagged enum comes last, so the values have to be
buffered until it is known.  In serde this is where locations and paths get
lost, in deser they are retained:

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
```

More practical examples are in the
[examples](https://github.com/mitsuhiko/deser/tree/main/examples) folder.

## How It Works

Instead of visitors calling into each other recursively, deserializing a type
creates a sink that receives events and serializing it produces emitters that
hand out nested values.  Nested sinks and emitters are returned to a driver
which keeps them in an arena instead of on the call stack, which is why
nesting cannot overflow the stack and a deserialization can be suspended
between events.  The data model is
small (atoms, maps and sequences) and can be extended with
[extension atoms](https://docs.rs/deser/latest/deser/ext/) that carry a
fallback for formats which do not understand them.  Where buffering cannot be
avoided, events are recorded together with the state the format published
for them and replayed as such.

## Limitations

Deser only supports self describing formats, so bincode, postcard and
similar formats are out of scope.  Known limitations, performance numbers and
notes on unsafe code are in
[LIMITATIONS.md](https://github.com/mitsuhiko/deser/blob/main/LIMITATIONS.md).

## Inspiration

This crate heavily borrows from
[`miniserde`](https://github.com/dtolnay/miniserde),
[`serde`](https://serde.rs/) and [Sentry Relay's meta
system](https://github.com/getsentry/relay).  The general trait design was
modelled after `miniserde`.

## License and Links

- [GitHub Repository](https://github.com/mitsuhiko/deser)
- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser)
- [Development Documentation](https://mitsuhiko.github.io/deser/)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
