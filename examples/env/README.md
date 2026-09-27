# env

```
cargo run -p env
```

## Why

Twelve-factor style services are configured through environment
variables. These are flat, all text, and prone to typos. `deser-env` maps
them onto normal derived types: `SHOP_SERVER__MAX_CONNECTIONS` becomes
`server.max_connections`, and each target type parses its text. Errors
name the variable, so the user knows what to fix.

## What it shows

- Naming rules: a prefix (`SHOP_`), `__` for nesting, and lowercased
  names. Variables without the prefix (`PATH`, `HOME`) are ignored.
- Nested structs and a `BTreeMap<String, bool>` (`SHOP_FEATURES__NEW_CHECKOUT=on`).
- Lists:
  - in a single variable with `Separated<',', TrimWhitespace>`
  - as indexed variables (`SHOP_BACKENDS__0__HOST`)
- An internally tagged enum with a flattened struct (`Storage::S3`). The
  `KIND` tag comes last, and the numbers still parse.
- Empty values: `SHOP_SERVER__MAX_CONNECTIONS=` becomes `None`, and
  `SHOP_DEBUG=` becomes `true` with the `Flag` adapter.
- Warnings for unknown variables (`SHOP_SERVER__PROT`) through
  `UnknownFields::Collect`, with both an `EnvVar` and a `Path` attachment.
- `deser_env::to_vars` to write a config back into variables, for example
  for a child process.

The example passes variables in with `from_vars`. A real program would use
`deser_env::from_env("SHOP_")`.

## What you should see

1. The full `Config` debug dump.
2. `warning: ... unknown field `prot` ... (environment variable SHOP_SERVER__PROT) (path: server.prot)`.
3. Two errors: `"three"` is not a u32 (it names `SHOP_STORAGE__ATTEMPTS`),
   and a variable that has both a value and nested variables.
4. The config written back as `SHOP_...=...` lines.

## How to read it

The `ENV` constant is the input, and the `Config` types are the schema.
Put them next to the debug dump.

Related: `config` (env as one layer of several), `query-strings` (the same
"everything is text" problem).
