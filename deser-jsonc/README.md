# deser-jsonc

JSONC (JSON with comments) support for
[deser](https://github.com/mitsuhiko/deser).  JSONC is JSON with `//` and
`/* */` comments and commas after the last element of sequences and maps,
as used by `tsconfig.json` or the settings of VS Code.

```rust
#[derive(deser::Deserialize)]
struct Config<'a> {
    name: &'a str,
    ports: Vec<u16>,
}

let config: Config = deser_jsonc::from_str(r#"{
    // the name of the service
    "name": "api",
    "ports": [80, 443,],
}"#).unwrap();
assert_eq!(config.name, "api");
```

Otherwise it works like [`deser-json`](https://docs.rs/deser-json), which
is also used to serialize (JSON is valid JSONC).  The parser is generated
from the one of `deser-json` (see `deser-template-json` in the
repository).

