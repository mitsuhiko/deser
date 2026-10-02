# json-lines

```
cargo run -p json-lines
```

## Why

JSON Lines (NDJSON) is a common format for logs and event streams. A
single corrupt line must not stop the reader. This example reads one
value per line, reports bad lines with their line number and continues
with the next line.

## What it shows

- `DeserializerConfig::builder().trailing(Trailing::Newline).build()` makes a
  `deser_json::Deserializer` read one value per line.
- A `while !de.is_end() { de.deserialize::<Event>() }` loop. An error
  (bad syntax or a type mismatch) only discards its own line. Empty lines
  are skipped.
- Writing JSON Lines: `to_string` never emits newlines, so appending
  `'\n'` after each value is enough.

## What you should see

```
skipped: OutOfRange: invalid value -1, expected u64 at line 4 column 46
skipped: EndOfFile: unexpected end of file at line 5 column 50
```

The first is a type error (negative `bytes`), the second a syntax error
(missing `}`). After that come the three good events and the JSON Lines
output written from them.

## How to read it

Put the `INPUT` constant next to the skipped lines. The line numbers
match lines in `INPUT`.

Related: `streams` (the same over `std::io` with `deser::io::Reader`),
`tokio-server` (async).
