# json-numbers

```
cargo run -p json-numbers
```

## Why

JSON numbers can have any precision, but most parsers squeeze them into
`f64` and silently lose digits. That is a problem for money, scientific
data or large IDs. deser keeps the original text of such numbers. Using
`deser::ext::Number` as the field type preserves it exactly, while `f64`
fields still get the nearest value.

## What it shows

- `Number<'static>` fields keep the exact text of the JSON number (a
  202-digit pi, a 60-digit integer, `0.10000000000000000001`). Because
  the text is copied, the values outlive the input (the input `String` is
  dropped right after parsing).
- `Number` methods: `as_str()`, `is_integer()`, and `value()` (the f64
  approximation).
- The same numbers read into `f64` fields (`approximate`) lose precision.
- `jiff::Timestamp` fields parsed from RFC 3339 strings with different
  offsets, all normalized to UTC. Enabled with the `jiff` feature.
- On serialization, `Number` is written back verbatim and `f64` as its
  shortest representation.

## What you should see

A debug dump, then one block per measurement comparing `value` (exact)
with `approximate` (f64), plus the timestamp in UTC and in Vienna. Last
comes the re-serialized JSON, where `value` keeps all digits and
`approximate` shows the rounded ones.

## How to read it

Compare the `value:` and `approximate:` lines in each block. This example
does not assert anything; it is meant to be read.

Related: `formats` (timestamps in other formats).
