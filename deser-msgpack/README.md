# deser-msgpack

[MessagePack](https://msgpack.org/) support for
[deser](https://github.com/mitsuhiko/deser).  MessagePack is a compact
binary format with the data model of JSON plus binary data and
extensions.  Use this crate when you want smaller payloads than JSON
without giving up a self describing format, or when you need to talk to
MessagePack based systems (Redis, Neovim, fluentd, RPC protocols, ...).

The same types you use with JSON work unchanged, but bytes become native
binary data instead of base64 and timestamps use the timestamp extension:

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    name: String,
    data: Vec<u8>,
}

let blob = Blob {
    name: "logo".into(),
    data: vec![0xde, 0xad, 0xbe, 0xef],
};

// MessagePack has native binary data
let msgpack = deser_msgpack::to_vec(&blob).unwrap();
assert_eq!(msgpack.len(), 22);
assert_eq!(deser_msgpack::from_slice::<Blob>(&msgpack).unwrap(), blob);

// JSON falls back to base64 for the very same type
assert_eq!(
    deser_json::to_string(&blob).unwrap(),
    r#"{"name":"logo","data":"3q2+7w=="}"#
);
```

* Reads all well-formed MessagePack and passes the
  [msgpack-test-suite](https://github.com/kawanet/msgpack-test-suite).
* Writes integers and lengths in their shortest form, floats keep their
  precision (`f32` is float 32, `f64` is float 64).
  `SerializerConfig::canonical` additionally sorts map entries for a
  deterministic encoding (for instance for hashing or signing).
* Timestamps (extension type `-1`) map onto deser's well-known
  `Timestamp`, so `std::time::SystemTime` and the timestamp types of
  `jiff`, `chrono` and `time` work with the respective features of deser.
* Other extensions are read and written with `Ext`.  Their fallback is the
  binary data.
* Strings and binary data are borrowed from the input.
* Deeply nested input does not overflow the stack.
* Items that follow each other are read with a `Deserializer` and written
  with a `Serializer`.
* `from_reader` and `to_writer` work with `std::io`, and the
  configurations read and write streams of items with `deser::io` or async
  runtimes (`deser-tokio`).  Items are parsed while their input arrives,
  so only incomplete atoms (like strings) are buffered.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-msgpack)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
