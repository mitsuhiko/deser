# deser-value

A dynamic value type for [deser](https://github.com/mitsuhiko/deser).
`Value` can hold anything the deser data model can express.  Use it for
data whose structure is not known up front, to inspect or transform data
before it's deserialized into a type, or to convert between formats:

```rust
use deser_value::{Value, value};

let mut config: Value = deser_json::from_str(r#"{"name": "app", "port": 8080}"#).unwrap();
config["port"] = value!(9090);
config["tags"] = value!(["web", "prod"]);
assert_eq!(
    deser_json::to_string(&config).unwrap(),
    r#"{"name":"app","port":9090,"tags":["web","prod"]}"#
);
```

Unlike most value types, a `Value` keeps as much of the input as possible.
Deserializing a value and serializing it again gives you the same output:

* **Any keys:** map keys can be any value (for instance integers in CBOR or
  complex keys in YAML).  Maps keep the order of their entries.
* **Extensions:** date-times, UUIDs, exact numbers and other extension
  values keep their type.  A TOML date-time stays a date-time, and JSON
  numbers that don't fit into an `f64` keep their text.
* **Event data:** information that is not part of the data model, such as
  CBOR tags, YAML tags and formatting hints, is kept in the meta data of
  each value.
* **Locations:** if the format tracks locations, values remember where
  they came from.  Types deserialized from such values report errors at
  the original location, even after values were moved around or merged
  from different files:

```rust
use deser::Deserialize;
use deser_value::{Value, from_value};

#[derive(Debug, Deserialize)]
struct Config {
    port: u16,
}

let value: Value = deser_json::DeserializerConfig::new()
    .track_locations(true)
    .from_str("{\n  \"port\": \"80\"\n}")
    .unwrap();
let err = from_value::<Config>(&value).unwrap_err();
assert_eq!((err.line(), err.column()), (Some(2), Some(11)));
```

Like the rest of deser, values do not use recursion.  Deeply nested values
can be deserialized, serialized, cloned, compared, formatted and dropped
without overflowing the stack.
