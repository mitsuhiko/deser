# deser-cbor

[CBOR](https://www.rfc-editor.org/rfc/rfc8949) support for
[deser](https://github.com/mitsuhiko/deser).  CBOR is a compact binary
format with the data model of JSON plus byte strings and tags.  Use this
crate when you want smaller payloads than JSON without giving up a self
describing format, or when you need to talk to CBOR based protocols (COSE,
WebAuthn, ...).

The same types you use with JSON work unchanged, but bytes become native
byte strings instead of base64 and UUIDs, date-times and big numbers use
their standard CBOR tags:

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    name: String,
    data: Vec<u8>,
}

let blob = Blob { name: "logo".into(), data: vec![0xde, 0xad, 0xbe, 0xef] };

// CBOR has native bytes
let cbor = deser_cbor::to_vec(&blob).unwrap();
assert_eq!(cbor.len(), 21);
assert_eq!(deser_cbor::from_slice::<Blob>(&cbor).unwrap(), blob);

// JSON falls back to base64 for the very same type
assert_eq!(
    deser_json::to_string(&blob).unwrap(),
    r#"{"name":"logo","data":"3q2+7w=="}"#
);
```

* Reads all well-formed CBOR including indefinite length strings, arrays
  and maps and CBOR sequences.
* Writes the preferred serialization (shortest integers, lengths and
  lossless floats).  `SerializerConfig::canonical` additionally sorts map
  entries for a deterministic encoding (for instance for signing).
* Date-times (tags 0, 1 and 1004), UUIDs (tag 37), decimal fractions
  (tag 4) and bignums (tags 2 and 3) map onto deser's well-known types, so
  `uuid::Uuid`, `jiff`, `chrono`, `time` and `rust_decimal` types work
  with the respective features of deser.
* Other tags are transparent, `Tagged<T>` reads and writes them.
* Deeply nested input does not overflow the stack.
