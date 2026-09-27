# formats

```
cargo run -p formats
```

## Why

A single derived type should work with every format. This example
serializes one `Release` struct to JSON, YAML, TOML, CBOR and MessagePack.
It focuses on types that are not in the core data model: UUIDs,
timestamps, dates and durations. deser passes these through as
*extension values*. Each format uses a native representation where it has
one and writes a fallback (usually a string) where it does not.

## What it shows

- `uuid::Uuid`, `jiff::Timestamp`, `jiff::civil::Date` and
  `std::time::Duration` in one struct. The `jiff` and `uuid` features of
  `deser` are enabled in `Cargo.toml`.
- Native handling per format:
  - TOML: date-times and dates as TOML literals (not quoted).
  - YAML: plain timestamps.
  - CBOR: tags 37 (UUID), 1 (timestamp) and 1004 (date).
  - MessagePack: the timestamp extension, strings for the rest.
  - JSON: strings, and ISO 8601 `PT12M34S` for the duration.
- Every format round-trips back to an equal `Release` (checked with
  asserts).
- `Plain`: a type that uses `String` instead of jiff types still reads the
  TOML file. It gets the string fallback of the native date-time.
- `#[deser(as = Compact)]` is a presentation hint. It gives an inline
  table in TOML and flow style `{...}` in YAML.

## What you should see

The same data as JSON, YAML and TOML. In the TOML, `published =
2024-06-19T15:22:45Z` is unquoted and `checksums` is inline. After that,
the byte sizes of CBOR and MessagePack, and finally the `Plain` struct
with string dates.

## How to read it

Compare the three text outputs. Differences in quoting show where a
format used a native type. The comments above the CBOR and MessagePack
sections say which native types are used.

Related: `bytes` (the same idea for binary data), `json-numbers`,
`dynamic-values` (conversion that keeps extension types).
