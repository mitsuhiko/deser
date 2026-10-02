# layers

```
cargo run -p layers
```

## Why

Layers are middleware that sits between the values and the format. A
layer sees every event and can change, drop or add events. With layers
you can make cross-cutting changes (renaming keys, redacting secrets,
limits, error paths) without touching the types. This example implements
three serialization layers from scratch and uses the two built-in
deserialization layers.

## What it shows

Custom `deser::ser::Layer` implementations (all in `main.rs`):

- `RenameKeys(fn)` rewrites map keys. It checks `next.state().is_map_key()`
  to know when an event is a key.
- `SkipNulls` drops map entries whose value is null. It holds back the key
  until it has seen the value, then emits both with `next.emit_key` or
  neither.
- `Redact` replaces the values of the listed keys with `"[redacted]"`.
  When such a value is a map or sequence, it drops that whole subtree by
  tracking the nesting depth.

These are pushed with `SerializerConfig::to_string_with(&v, |driver| ...)`.

Built-in deserialization layers:

- `deser::de::Limits` (here `max_items(5)`)
- `deser_path::PathLayer`

The order of the layers matters: `PathLayer` is pushed first, so errors
raised by `Limits` also get a path.

## What you should see

```
plain:
{"user_name":"jdoe","email_address":null,
  "password_hash":"$argon2id$...", ...}

with layers:
{"user-name":"jdoe","password-hash":"[redacted]",
  "api-tokens":"[redacted]","settings":{"dark-mode":true}}

deserialization errors:
LimitExceeded: too many items at line 5 column 49 (path: api_tokens[5])
InvalidType: unexpected string, expected bool at line 6 column 35
  (path: settings.dark_mode)
```

## How to read it

Put the "plain" and "with layers" lines next to each other. Then read
each `impl Layer`: they are small state machines over `Event`s. `Redact`
is the reference for handling nested values in a layer.

Related: `located` (a deserialization layer that rewrites events),
`deep-nesting` (`Limits`).
