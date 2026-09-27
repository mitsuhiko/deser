# optionals

```
cargo run -p optionals
```

## Why

PATCH-style partial updates need three states per field: *missing*
(leave it alone), *null* (clear it) and *a value* (set it). This example
shows how to model that, and how to leave unset fields out of the output
without annotating every field.

## What it shows

- `#[deser(skip_serializing_optionals)]` on the struct: every field whose
  current value reports itself as optional (such as `None`) is skipped.
  Without it, you'd need `skip_serializing_if` on each field.
- `Option<Option<T>>` for a nullable field in a patch:
  - `None`: the key is missing
  - `Some(None)`: the key is present and `null`
  - `Some(Some(v))`: the key has a value
- `Option` fields are optional when deserializing by default.
- `Profile::apply` as a typical patch handler.

## What you should see

```
{"name":"Jane"}
{"nickname":null}
Profile { name: "Jane", nickname: Some("jd"), tags: ["staff"] }
Profile { name: "Jane", nickname: None, tags: ["staff"] }
```

The first two lines show that only the set fields are written, including
an explicit `null` for `Some(None)`. The last two show that a missing
`nickname` keeps `"jd"` while `"nickname": null` clears it.

## How to read it

The comments on the `ProfilePatch` fields describe the three states. The
asserts in `main` check each transition.

Related: `json` (`skip_serializing_if`), `input-contracts` (`required`
options), `config` (`update` for merging whole structs).
