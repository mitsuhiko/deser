# csv

```
cargo run -p csv
```

## Why

CSV files are tables of text: what `25.5` or an empty field means is only
known to the type a field ends up in, and every tool writes a slightly
different dialect.  `deser-csv` passes fields on as *lexical atoms* which
the types parse, also in flattened structs and internally tagged enums,
and the dialect is configurable (delimiters, quotes, escapes, line endings,
null values).

## What it shows

- Reading a CSV export row by row with `deser::io::Reader`: the byte
  order mark, `\r\n`, quoted fields with commas, quotes and line breaks.
- An internally tagged enum (`kind`) flattened into the row, so the
  columns that matter depend on the kind.
- `Nulls::Empty`: empty fields are missing values (`copies` of a gift).
- A list in a single field with `Separated<';'>`, composed with
  `Option` for empty fields.
- A row with an error (`copies` is `many`) is reported with its line,
  column and path and skipped, the other rows are read.
- The names of the columns in the state of the stream.
- Writing the rows with the given columns (the first row is a book and
  does not have all of them) as CSV (quoted where necessary) and as TSV with
  backslash escapes and `\N` for null (`SerializerConfig::tsv`), and
  reading both back.

## What you should see

```
skipped: Unexpected: invalid value "many", expected u32 at line 4 column 30 (path: copies)
```

followed by the column names, the three orders, and the orders as CSV and
as TSV.

## How to read it

Start with `INPUT` and the `Order` type, then follow the loop in `main`.

Related: `query-strings` and `env` (the same "everything is text" idea),
`json-lines` (per-line error recovery).
