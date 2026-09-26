# deser-path

Experimental extension library to `deser` that provides a layer which
tracks the path of the current value (for instance `servers[1].port`)
during serialization and deserialization and adds it to errors.

```rust
use deser::de::Format;
use deser_path::PathLayer;

let err = deser_json::Deserializer::from_str(r#"{"ports": [80, "x"]}"#)
    .deserialize_with::<std::collections::BTreeMap<String, Vec<u16>>, _>(|driver| {
        driver.push_layer(PathLayer::new());
    })
    .unwrap_err();
assert_eq!(err.path(), Some("ports[1]"));
```
