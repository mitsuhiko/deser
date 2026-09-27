# config

```
cargo run -p config
```

## Why

This is the most complete "real application" example. Programs usually
build their configuration from several layers: built-in defaults, a
system file, a user file, environment variables and command-line
overrides. Each layer should only change what it mentions. Typos should
produce warnings instead of being silently ignored, and every error should
point at its source.

## What it shows

- `update_with(&mut config, ...)` instead of deserializing a new value.
  Each layer merges into the existing `Config`. Nested structs and maps
  are merged, and missing keys keep their previous value.
- Sources, applied in this order:
  1. `Config::default()`
  2. two TOML files (`SYSTEM`, `USER`)
  3. environment variables via `deser-env` (`SHOP_SERVER__PORT=9090`)
  4. `--set` style overrides parsed as a query string with dotted keys
     (`deser-urlencoded` with `Nesting::Dots`).
- Unknown keys are collected as warnings through the `UnknownFields`
  state set to `UnknownFields::Collect(IgnoredFields)`. Each warning has
  a path plus a line/column or an environment variable name.
- `#[deser(deny_unknown_fields)]` on `Timeouts` turns typos into errors
  for that one type.
- Validation with `deser-validate`: `#[deser(as = Check<NonZero>)]` on a
  field and `#[deser(deserialize_as = Check<ConfigRules, _>)]` on the
  whole struct, with validators made by `validator!`. The struct-level
  check runs after the update is complete.
- A small `Report` type that formats `Path`, `line`/`column` and `EnvVar`
  attachments.

## What you should see

1. The final merged config printed as TOML. For example, `port = 9090`
   comes from the environment, `workers = 16` from an override,
   `connect_secs = 2` from the system file and `read_secs = 30` from the
   defaults.
2. Two warnings: the `workres` typo in the user file (with a line number)
   and `SHOP_SERVER__TIMEUOTS` (with the variable name).
3. A series of `error:` lines: port 0 rejected by validation, the read
   timeout shorter than the connect timeout, the `read_sec` typo in a
   `deny_unknown_fields` type, `"many"` in an environment variable, and a
   bad or unknown `--set` override.

## How to read it

Start at `main`. The asserts after the four `apply_*` calls show which
layer each value came from. `setup()` is the part you would copy: it
pushes the `PathLayer` and sets up warning collection.

Related: `env`, `query-strings`, `config-errors`, `dynamic-values`
(merging untyped values).
