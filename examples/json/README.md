# json

```
cargo run -p json
```

## Why

This is the starting point: the smallest useful program with deser.
Derive `Serialize` and `Deserialize`, read JSON, print the value and
write it back. If you know serde, this shows that the attributes look
the same (`#[deser(...)]` instead of `#[serde(...)]`).

## What it shows

- `#[deser(rename_all = "camelCase")]` on a struct (fields) and
  `"snake_case"` on an enum (variants).
- `#[deser(flatten)]`: the fields of `UserAttributes` appear in the
  parent JSON object. This happens without buffering.
- `Option<String>` fields are optional when reading. They are written as
  `null` unless skipped.
- `#[deser(skip_serializing_if = Vec::is_empty, default)]`: `tags` is
  optional and left out of the output when empty.
- `deser_json::from_str` / `deser_json::to_string`.
- `deser_debug::ToDebug`: the types don't derive `Debug`, but can still
  be printed with `{:#?}`.

## What you should see

```
User {
    id: 23,
    emailAddress: "jane@example.com",
    kind: regular_user,
    isSpecial: true,
    displayName: None,
}
{"id":23,"emailAddress":"jane@example.com","kind":"regular_user","isSpecial":true,"displayName":null}
```

The debug output shows serialized names (`emailAddress`,
`regular_user`), because `ToDebug` shows the value the way it is
serialized. `tags` is missing because it was empty, and `displayName`
is `null` because it was not skipped.

## How to read it

Compare the input JSON in `main` with the output line. The differences
come from `default`, `skip_serializing_if` and `Option`.

Next: `enums`, `optionals`, `formats`, `debug`.
