# deser-serde

Adapters to use types that only implement [serde](https://serde.rs/)'s
traits with [deser](https://github.com/mitsuhiko/deser).  Most of the
ecosystem implements serde but not deser.  With this crate you do not need
to wait for that: mark a field with `#[deser(as = Serde)]` and deser uses
its serde implementation, in any deser format.

```rust
use deser::{Deserialize, Serialize};
use deser_serde::Serde;

// a type from some crate that only knows serde
#[derive(serde::Serialize, serde::Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize)]
struct Shape {
    name: String,
    // adapters compose with containers
    #[deser(as = Vec<Serde>)]
    points: Vec<Point>,
    #[deser(as = Option<Serde>)]
    extra: Option<serde_json::Value>,
}

let shape: Shape = deser_yaml::from_str("
name: line
points:
  - {x: 1, y: 2}
  - {x: 3, y: 4}
extra: {color: red}
").unwrap();
assert_eq!(shape.points[1].y, 4);
assert_eq!(shape.extra, Some(serde_json::json!({"color": "red"})));

assert_eq!(
    deser_json::to_string(&shape).unwrap(),
    r#"{"name":"line","points":[{"x":1,"y":2},{"x":3,"y":4}],"extra":{"color":"red"}}"#
);
```

serde and deser drive values in opposite directions, so compound values are
buffered by `Serde`.  With the `coroutine` feature the `SerdeCoroutine`
adapter streams them instead by running serde on a stackful coroutine.
