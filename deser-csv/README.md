# deser-csv

CSV, TSV and other delimited text for [deser](https://github.com/mitsuhiko/deser).

A document is a sequence of records.  The first record holds the names of
the columns and the other records are maps of the names to their fields
(or sequences, without names).  Everything in a CSV file is text, only the
type a field is deserialized into knows what it means.  deser passes the
fields on as lexical atoms which are parsed by the type they end up in,
also after buffering, so numbers parse in flattened structs and internally
tagged enums:

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct City {
    name: String,
    country: String,
    population: Option<u64>,
}

let input = "name,country,population\nVienna,Austria,1897000\nAtlantis,,\n";
let cities: Vec<City> = deser_csv::from_str(input).unwrap();
assert_eq!(cities[0].population, Some(1897000));
assert_eq!(cities[1].population, None);

assert_eq!(deser_csv::to_string(&cities).unwrap(), input);
```

What works:

* CSV as described by RFC 4180 and the dialects around it: any delimiter
  (`;`, `\t`, `|`, ASCII separators), any quote character or none, doubled
  or escaped quotes, backslash escapes, `\n`, `\r\n` and `\r` line endings,
  comments, blank lines, trimming whitespace, byte order marks and the
  `sep=;` line of Excel.
* TSV as databases write it (backslash escapes and `\N` for null) with
  `DeserializerConfig::tsv()` and `SerializerConfig::tsv()`.
* Strict by default: quotes that do not follow the rules and records with
  the wrong number of fields are errors (`lenient_quotes` and `flexible`
  accept them).  Errors point at the line and column and (with
  `deser-path`) at the path (`[3].price`).
* Records one at a time from streams (`deser::io::Reader` or
  `deser-tokio`), where errors only discard their record and the names of
  the columns are in the state of the stream.
* Empty fields are `None` for optionals of types that do not accept them,
  which fields are null is configurable.  Lists in a field with the
  `Separated` adapter.
* Serializing with the columns from the first record (or given ones),
  quoting where necessary (or always, or for everything but numbers) and
  optionally escaping formulas for spreadsheets ("CSV injection").

The parser is tested with the test cases of
[PapaParse](https://github.com/mholt/PapaParse) and
[rust-csv](https://github.com/BurntSushi/rust-csv).

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-csv)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)
