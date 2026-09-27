# deser-json5

[JSON5](https://json5.org/) support for
[deser](https://github.com/mitsuhiko/deser).  JSON5 extends JSON with
comments, trailing commas, identifiers as map keys, single quoted strings,
hexadecimal numbers, `Infinity`, `NaN` and more.

```rust
#[derive(deser::Deserialize)]
struct Config<'a> {
    name: &'a str,
    ports: Vec<u16>,
    timeout: f64,
}

let config: Config = deser_json5::from_str(r#"{
    // the name of the service
    name: 'api',
    ports: [0x50, 443,],
    timeout: .5,
}"#).unwrap();
assert_eq!(config.name, "api");
assert_eq!(config.ports, [80, 443]);
```

Otherwise it works like [`deser-json`](https://docs.rs/deser-json), which
is also used to serialize (JSON is valid JSON5).  The parser is generated
from the one of `deser-json` (see `deser-private-jsontemplate` in the
repository).

