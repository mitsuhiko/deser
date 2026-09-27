# input-contracts

```
cargo run -p input-contracts
```

## Why

For a public API, the Rust types *are* the contract. This example models
the requests of an issue tracker API. It shows how to state exactly what
input is accepted, and how to make rejections read in the API's own
terms instead of Rust type names.

## What it shows

- `#[deser(transparent)]` on a struct with named fields (`Email`). On the
  wire it is just a string. The other field (`verified`) is
  `#[deser(skip)]`, so clients can't set it.
- `#[deser(deserialize_as = Check<ValidEmail, _>)]` with `deser-validate`:
  the derived implementation parses the value, then the validator checks
  it and its error points at the value.
- `#[deser(expecting = "a label")]` / `"a request"`: errors say "expected
  a label" instead of naming the Rust type.
- `#[deser(required)]` on an `Option` field (`parent`): the key must be
  present, but `null` is allowed. "No parent" becomes an explicit choice.
- `#[deser(deny_unknown_fields)]` on one variant (`Create`) only. Typos
  there are errors because they would lose data. `Search` still ignores
  extra keys such as `utm_source`.
- `PathLayer` to put the path into every error.

## What you should see

A valid `Create` request (debug and JSON), a `Search` that ignores
`utm_source`, and then one labelled error per rule:

```
the parent is required, even if it's an option:
  MissingField: missing field `parent` ...
typos are errors when creating issues:
  Unexpected: unknown field `lables`, expected one of ... (path: lables)
...
labels are described in the words of the API:
  Unexpected: unexpected string, expected a label ... (path: labels[0])
```

## How to read it

The `for (what, json) in [...]` loop at the end is a table of
contract-violating inputs, each paired with a description of what it
demonstrates. Add your own entries to probe the contract.

Related: `config-errors`, `renames`, `enums`.
