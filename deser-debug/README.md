# deser-debug

Formats any value that implements deser's `Serialize` like
`#[derive(Debug)]` would.  This is useful when a type is serializable but
does not implement `Debug` (for instance because you want to keep the
compile time and binary size of a second derive off your types), when you
are generic over `Serialize` and want to log values, or when you want to
implement `Debug` in terms of `Serialize`.

The Rust shape of values (struct and variant names, `Option`, tuples, ...)
is taken from their description (see `deser::ser::Describe`), so for
derived types the output matches what `#[derive(Debug)]` produces,
including pretty printing with `{:#?}`:

```rust
use deser::Serialize;
use deser_debug::ToDebug;

#[derive(Serialize)]
struct Point {
    x: i32,
    y: Option<i32>,
}

#[derive(Serialize)]
#[deser(tag = "type")]
enum Shape {
    Circle { center: Point, radius: f64 },
}

let shape = Shape::Circle {
    center: Point { x: 1, y: None },
    radius: 2.5,
};
assert_eq!(
    format!("{:?}", ToDebug::new(&shape)),
    "Circle { center: Point { x: 1, y: None }, radius: 2.5 }"
);
```

Note that the output shows the Rust value, not the serialized form: the
internally tagged enum above is still shown as a `Circle` variant rather
than a map with a `type` key.

Names are the ones used when serializing: renamed types, variants and
fields (for instance with `rename_all = "camelCase"`) are shown with their
new names, variants named by integers are shown as numbers and the fields
of flattened structs are shown in the struct they are flattened into.  Only
the shape is taken from the description, which keeps it cheap: types do
not carry any extra information for formatting.
