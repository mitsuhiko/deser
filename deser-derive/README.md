# deser-derive

The `#[derive(Serialize, Deserialize)]` macros for
[deser](https://github.com/mitsuhiko/deser).  You do not depend on this
crate directly, enable the `derive` feature of `deser` instead:

```toml
[dependencies]
deser = { version = "0.8", features = ["derive"] }
```

The attributes follow serde, so if you are coming from there most of what
you know carries over: renaming, defaults, aliases, flattening, skipping
and all four enum representations.  What you get on top is that the
generated code is small (it relies on dynamic dispatch rather than
monomorphization, which keeps compile times down) and that buffered
representations such as internally tagged enums keep source locations and
paths for errors.

```rust
use deser::{Deserialize, Serialize};
use deser::adapters::DisplayFromStr;
use std::net::IpAddr;

#[derive(Debug, Serialize, Deserialize)]
#[deser(rename_all = "kebab-case", skip_serializing_optionals)]
struct Service {
    name: String,
    #[deser(as = DisplayFromStr)]
    listen: IpAddr,
    #[deser(default = 30)]
    timeout_secs: u32,
    health_check: Option<String>,
    backend: Backend,
}

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "type", rename_all = "lowercase")]
enum Backend {
    Http { url: String },
    Static { root: String },
}

let service: Service = deser_json::from_str(r#"{
    "name": "web",
    "listen": "127.0.0.1",
    "backend": {"root": "/srv/www", "type": "static"}
}"#).unwrap();
assert_eq!(service.timeout_secs, 30);

assert_eq!(
    deser_json::to_string(&service).unwrap(),
    r#"{"name":"web","listen":"127.0.0.1","timeout-secs":30,"backend":{"type":"static","root":"/srv/www"}}"#
);
```

All attributes are documented in the
[`derive`](https://docs.rs/deser/latest/deser/derive/) module of deser.
