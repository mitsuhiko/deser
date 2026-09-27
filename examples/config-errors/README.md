# config-errors

```
cargo run -p config-errors
```

## Why

People fix configuration files by hand, so an error message needs to say
exactly where the problem is: line, column, and the logical path such as
`servers[1].backend.timeout`. This example shows that deser provides both,
even for values that had to be buffered first. The internally tagged
`Backend` enum has its `type` key last, so its fields are recorded before
the variant is known. Their locations survive that.

## What it shows

- `deser_path::PathLayer` pushed onto the driver with `deserialize_with`.
  It attaches a `Path` to errors (`err.attachment::<Path>()`).
- `err.line()` / `err.column()` from the TOML and YAML deserializers.
- Errors inside buffered values (the fields of the internally tagged
  enum) that still point at the right place.
- Syntax errors (not just type errors) that carry a location.

## What you should see

First the parsed config, then four error lines:

```
OutOfRange: invalid value 80810, expected u16 at line 11 column 8 (path: servers[1].port)
Unexpected: unexpected string, expected u32 at line 15 column 11 (path: servers[1].backend.timeout)
Unexpected: unexpected string, expected u32 at line 11 column 16 (path: servers[1].backend.timeout)
Unexpected: unexpected newline, expected a value at line 6 column 8
```

The second and third errors are the interesting ones: the value was
buffered, and the location still points at the offending token.

## How to read it

Each error case is made by `.replace(...)` on the valid TOML/YAML input,
so you can see exactly what was broken. The asserts check both the path
and the line/column.

Related: `config` (layered config with warnings), `located` (getting
locations into your own types), `input-contracts`.
