# generic-types

```
cargo run -p generic-types
```

## Why

The derive adds a `T: Serialize` / `T: Deserialize<'de>` bound for every
type parameter. That is wrong when a parameter is only a marker. Here,
`Record<B: Backend, T>` only serializes `B::Id`, and `Postgres`/`Couch`
never implement `Serialize`. Without custom bounds such types can't be
used. This example shows how to replace the inferred bounds. It also
shows enums with lifetime and const generic parameters.

## What it shows

- Field-level `serialize_bound(...)` / `deserialize_bound(...)` on `id` in
  `Record`. Only that field's bounds are replaced, and `T: Serialize` is
  still inferred from `data`.
- Type-level `bound(B::Id: Serialize + DeserializeOwned)` on `Cursor`.
  One bound replaces the inferred ones for both derives.
- Type-level `serialize_bound` / `deserialize_bound` on an enum with a
  lifetime (`Change<'a, B>`). The borrowed `table: &'a str` points into
  the input, which an assert checks.
- A const generic enum `Shape<const D: usize>` with `[f64; D]`. An array
  of the wrong length fails with `WrongLength`.
- `deser-debug`'s `ToDebug` is used instead of `#[derive(Debug)]`, because
  that derive has the same bound problem.

## What you should see

- The same `Record` type with an `i64` id (Postgres) and a `String` id
  (Couch), then an error when a string id is given to the Postgres
  record.
- A `Cursor` round-trip.
- A list of `Change`s and `Shape<2>` / `Shape<3>` values.
- `error: WrongLength: too many elements in array ...` for a 3-element
  point in `Shape<2>`.

Variant names are shown in their renamed form (`insert`, `point`),
because `ToDebug` shows what the type reports about itself.

## How to read it

Start at the `Backend` trait and the two marker types. Then look at the
bounds on each type definition. The runtime output is simple on purpose;
the point is that this compiles.

Related: `debug`, `borrowing`.
