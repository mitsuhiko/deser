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

Otherwise it works like [`deser-json`](https://docs.rs/deser-json) and
serializes JSON (which is valid JSON5, NaN and infinite floats are written
as `NaN`, `Infinity` and `-Infinity`).  The parser and the serializer are
generated from the ones of `deser-json` (see `deser-template-json` in the
repository).

