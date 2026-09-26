# deser-json

[JSON](https://www.json.org/) support for
[deser](https://github.com/mitsuhiko/deser).  Use it to read and write JSON
for any type that implements deser's `Serialize` and `Deserialize`.

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Event<'a> {
    // borrowed from the input, no allocation
    kind: &'a str,
    id: u128,
    payload: Vec<u8>,
}

let json = r#"{"kind": "upload", "id": 340282366920938463463374607431768211455, "payload": "aGVsbG8="}"#;
let event: Event = deser_json::from_str(json).unwrap();
assert_eq!(event.kind, "upload");
assert_eq!(event.id, u128::MAX);
assert_eq!(event.payload, b"hello");

assert_eq!(
    deser_json::to_string(&event).unwrap(),
    r#"{"kind":"upload","id":340282366920938463463374607431768211455,"payload":"aGVsbG8="}"#
);
```

Why use it:

* **No dependencies** besides `deser` by default.  The `speedups` feature
  pulls in `ryu`, `itoa` and `simdutf8` for faster number formatting and
  UTF-8 validation.
* **Borrowing:** strings without escapes are passed on borrowed so types
  can hold `&str` pointing into the input.
* **Numbers without loss:** 128 bit integers are written and read as
  numbers, and with `DeserializerConfig::exact_numbers` decimals such as
  `0.10` keep their exact text for types like `rust_decimal::Decimal`.
* **Bytes:** `Vec<u8>` is base64 by default and can be configured per
  format or per field (hex, arrays of integers, ...).
* **Unlimited nesting:** a million nested arrays do not overflow the stack.
* **JSON Lines:** a `Deserializer` can read one value per line and
  recovers from errors in individual lines.
* **Streams:** `from_reader` and `to_writer` work with `std::io`, and the
  `Decoder` and `Encoder` read and write streams of values (JSON Lines or
  concatenated JSON) with `deser::io` or async runtimes (`deser-tokio`)
  while only buffering one value at a time.
* **Source locations:** errors carry line and column and with
  `DeserializerConfig::track_locations` values can be wrapped in
  [`deser_location::Spanned`](https://docs.rs/deser-location) to learn
  where they came from.
