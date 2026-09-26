# deser-serde

Experimental extension library to `deser` that provides adapters to
serialize and deserialize values with their [serde](https://serde.rs/)
implementations.  This is useful for types from crates which only support
serde.

```rust
use deser::{Deserialize, Serialize};
use deser_serde::Serde;

#[derive(Serialize, Deserialize)]
struct Config {
    #[deser(as = Serde)]
    extra: serde_json::Value,
    #[deser(as = Option<Vec<Serde>>)]
    points: Option<Vec<SomeSerdeType>>,
}
```

serde and deser drive values in opposite directions, so compound values are
buffered by `Serde`.  With the `coroutine` feature the `SerdeCoroutine`
adapter streams them instead by running serde on a stackful coroutine.
