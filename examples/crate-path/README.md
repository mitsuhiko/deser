# crate-path

```
cargo run -p crate-path
```

## Why

By default, the derive generates code that refers to `::deser`. That path
breaks in two cases: when a crate renames the dependency in `Cargo.toml`,
or when a framework re-exports deser and its users don't depend on deser
directly. `#[deser(crate = path)]` tells the derive where to find it.

## What it shows

- `Cargo.toml` renames the dependency:
  `serialization = { package = "deser", ... }`. So `::deser` does not
  exist in this crate.
- `User` uses `#[deser(crate = serialization)]`.
- `Event<T>` uses a re-export, `#[deser(crate = crate::framework::ser)]`.
  It is an internally tagged enum with struct variants, whose generated
  helper types also need the path.

## What you should see

```
Login { user: User { name: "Peter", age: 42 }, extra: true }
{"type":"Login","user":{"name":"Peter","age":42},"extra":true}
```

`age` is `42` because of `#[deser(default = 42)]`. The main point is that
the example compiles at all.

## How to read it

The interesting part is at compile time, not at runtime: look at
`Cargo.toml` and the `#[deser(crate = ...)]` attributes. The file also
contains a `#[test]` with round-trip checks. `test = false` is set on the
bin target, so `cargo test -p crate-path` does not run it by default.
