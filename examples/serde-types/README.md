# serde-types

```
cargo run -p serde-types
```

## Why

Most of the ecosystem implements serde, not deser. You shouldn't have to
wait for `semver`, `serde_json` or your own older types to add deser
support. `deser-serde`'s `Serde` adapter uses a type's serde impl inside
any deser format. Errors from serde impls still get deser's paths and
locations.

## What it shows

- `#[deser(as = Serde)]` on `semver::Version`.
- Composition with containers:
  - `Vec<Serde>` for a list of a serde-derived `Person` (a stand-in for a
    type from another crate, including its `#[serde(skip_serializing_if)]`)
  - `BTreeMap<_, Serde>` for `semver::VersionReq` values
  - `Option<Serde>` for `serde_json::Value` (free-form data)
- The manifest is read from TOML and written as pretty JSON and as YAML.
  All of it goes through deser formats.
- Errors from serde impls get deser's line and path:
  `semver`'s own parse error for `"1.4"`, and serde's `missing field
  name` inside `authors[1]`.
- `As<Version, Serde>` for use outside a derive.

## What you should see

```
shop 1.4.0-beta.2
  depends on deser ^0.8
  depends on semver >=1.0.20, <2
```

After that, the manifest as pretty JSON and as YAML (John has no `email`,
because serde's `skip_serializing_if` is honored), and:

```
error: Unexpected: unexpected end of input while parsing minor version
  number at line 3 column 11 (path: version)
error: MissingField: missing field `name` at line 4 column 59
  (path: authors[1])
```

## How to read it

The `Manifest` struct is the only place that mentions the adapter.
Everything else is normal deser usage.

Related: `adapters` (how `as = ...` composes).
