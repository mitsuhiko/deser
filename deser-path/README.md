# deser-path

A layer for [deser](https://github.com/mitsuhiko/deser) that tracks the
path of the current value (for instance `servers[1].port`) during
serialization and deserialization and attaches it to errors.

"invalid type: string, expected u16" is not a useful error message when the
input has hundreds of values.  Add the `PathLayer` and errors tell you
which value is wrong.  It works with every format and does not require any
changes to your types:

```rust
use deser::Deserialize;
use deser::de::Format;
use deser_path::{Path, PathLayer};

#[derive(Debug, Deserialize)]
struct Config {
    servers: Vec<Server>,
}

#[derive(Debug, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

let json = r#"{"servers": [
    {"host": "a.example.com", "port": 80},
    {"host": "b.example.com", "port": "http"}
]}"#;

let err = deser_json::Deserializer::from_str(json)
    .deserialize_with::<Config, _>(|driver| driver.push_layer(PathLayer::new()))
    .unwrap_err();
assert_eq!(err.attachment::<Path>().unwrap().to_string(), "servers[1].port");
assert_eq!((err.line(), err.column()), (Some(3), Some(39)));
```

Unlike wrapping the deserializer (as `serde_path_to_error` does) the path
is also correct for values that had to be buffered, such as the content of
internally tagged and untagged enums.  During serialization and
deserialization the current `Path` is available in the state, so custom
`Serialize` and `Deserialize` implementations and other layers can use it
too (for instance to redact values at certain paths).
