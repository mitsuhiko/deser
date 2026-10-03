# dynamic-values

```
cargo run -p dynamic-values
```

## Why

Sometimes the structure of the data is not known up front. Examples are
generic tooling, format conversion, passing through unknown fields, and
merging config files before picking a type. `deser_value::Value` holds
anything the deser data model can express, including extension values
such as date-times and the source location of each value.

## What it shows

The example has four functions, each run from `main`:

1. `inspect`: walks a `Value` with `kind()` / `Kind::Map` / `Kind::Seq`,
   indexes into it (`value["user"]["roles"][0]`), edits it with the
   `value!` macro, and redacts all `password` keys recursively.
2. `convert`: TOML → `Value` → YAML/JSON/TOML. A TOML date-time stays a
   date-time (`Kind::Ext`), so YAML and TOML write it natively and JSON
   writes it as a string.
3. `unknown_fields`: a struct with `#[deser(flatten)] extra: Map` keeps
   the fields it does not know and writes them back unchanged. `kind:
   &'a str` borrows from the `Value`.
4. `merge`: two JSON files, parsed with `TrackLocations(true)` in the
   context, are
   merged with `update`. Then a typed `Config` is deserialized with
   `deser_value::Deserializer` + `PathLayer`. The resulting error points
   to line 4, column 13 **of the second file**, where the bad value came
   from.

## What you should see

- A list of paths and their kinds (`.user.roles[1]: string`, `.score: float`,
  ...), followed by the edited and redacted JSON.
- `published is a datetime`, then the same data as YAML and as JSON.
- The `Event` with the extra fields in a `Map`, and the unchanged
  round-tripped JSON.
- `error: ... expected u16 at line 4 column 13 (path: server.port)`.

## How to read it

Read one function at a time. `merge` is the most unusual part:
`merged["server"]["port"].span()` returns the span in the source it came
from, and the assert checks that it is the text `"eighty"` in `local`.

Related: `config` (typed layering with `update`), `located`, `formats`.
