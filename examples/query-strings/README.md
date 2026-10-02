# query-strings

```
cargo run -p query-strings
```

## Why

Everything in a query string or HTML form is text. What `limit=10`
means is only known to the type the value ends up in. With serde,
numbers stop parsing once they pass through `flatten` or an
internally tagged or untagged enum, because the value is buffered as a
string (serde issue #1183). deser passes such text on as *lexical
atoms*. They stay lexical while buffered, so the target type can still
parse them.

## What it shows

- `deser_urlencoded::from_str` into a `Search` struct with:
  - a flattened `Page` (`limit: u32`, `offset: Option<u32>`)
  - a flattened, internally tagged `Option<Filter>` whose tag `kind=games`
    comes last in the query, while `players=4` and nested
    `price[min]=10&price[max]=50` still parse as numbers
  - repeated keys for a `Vec` (`tags=family&tags=strategy`)
  - `offset=`, an empty value, which becomes `None`
  - `?exact`, a key without a value, which is `true` with `Flag`
- No `kind` means the flattened `Option<Filter>` is `None`.
- Serializing back to a query string. Repeated keys are the default;
  `ArrayFormat::Brackets` writes `tags[]=` (percent-encoded like browsers
  do).
- An HTML form (`SignUp`): the checkbox value `on` becomes `bool`, and a
  `<select multiple>` sends repeated keys that go into a `Vec<Interest>`.
- Errors inside the flattened enum carry the path (`year`) and a byte
  offset.

## What you should see

The parsed `Search` debug dump, two serialized query strings, the parsed
`SignUp` form, and:

```
error: InvalidValue: invalid value "soon", expected u16 at line 1 column 30
  (path: year)
```

## How to read it

Look at the first query string in `main` and trace each key to a field.
The comment at the top of `main.rs` explains why this would fail with
serde.

Related: `env` (the same "everything is text" idea), `config` (query
strings as `--set` overrides).
