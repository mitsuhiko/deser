# debug

```
cargo run -p debug
```

## Why

deser's data model is small. `Some(42)` is just `42`, a tuple struct is a
sequence and a unit struct is null. On top of that, types describe their
Rust shape (`Serialize::describe`). `deser-debug` uses that description to
format any `Serialize` value the way `#[derive(Debug)]` would. So types do
not need a `Debug` derive, which is useful when that derive would add
unwanted bounds (see `generic-types`).

## What it shows

- `ToDebug::new(&value)` with `{:?}` and `{:#?}` on types that only derive
  `Serialize`.
- Shapes that are kept even though the wire format flattens them:
  - `Option`
  - newtypes (`Meters`, `Length<Unit>` with a skipped `PhantomData`)
  - tuple structs (`Rgb`)
  - unit structs (`Unset`)
  - `#[deser(transparent)]` structs (`UserId`, shown like a newtype)
- Enums show their variants whatever the representation: internally
  tagged `Shape`, adjacently tagged `Fill` and untagged `Label`.
- Implementing `fmt::Debug` for a type by delegating to `ToDebug`
  (`Drawing`).
- A hand-written `Serialize` without `describe` (`Anonymous`). It is
  formatted as a plain map `{"answer": 42, "maybe": Some(true)}`.

## What you should see

The output alternates between JSON and debug output, so you can compare
the two. For example, JSON `0.5` becomes `Some(Length(0.5))`, and JSON
`{"t":"Gradient","c":["red","blue"]}` becomes
`Some(Gradient("red", "blue"))`. The big `Drawing { ... }` block is the
`{:#?}` output of the custom `Debug` impl.

## How to read it

Put each JSON line next to the debug line that follows it. Every
`assert_eq!` in `main` shows the exact expected string.

Related: `json` (basic `ToDebug` use), `manual-struct` (how to implement
`describe`).
