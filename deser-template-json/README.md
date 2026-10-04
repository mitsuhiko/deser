# deser-template-json

The source of the parsers and serializers of `deser-json`, `deser-jsonc`,
`deser-json5` and `deser-hj` and their tests.  This crate is not published and
nothing depends on it.  It only exists so that the template is Rust code that
compiles, can be tested and works in editors.

The crates share one parser and one serializer (JSON is valid in every
dialect, each crate writes its own raw values as they are and JSON5 writes
NaN and infinite floats).  Every crate gets its own copy, so the dialects do
not depend on each other.  The code that only some dialects have is marked
with `#[cfg]` attributes on made up *capabilities*:

| capability        | what it adds                                                          | jsonc | json5 | hjson |
|-------------------|-----------------------------------------------------------------------|:-----:|:-----:|:-----:|
| `comments`        | `//` and `/* */` comments                                             |   ✓   |   ✓   |   ✓   |
| `trailing_commas` | a comma after the last element of a sequence or map                   |   ✓   |   ✓   |   ✓   |
| `single_quotes`   | strings in single quotes, control characters in strings               |       |   ✓   |   ✓   |
| `json5`           | the rest of JSON5 (identifier keys, numbers, escapes)                 |       |   ✓   |       |
| `hjson`           | the rest of Hjson (strings and keys without quotes, multiline strings, `#` comments, optional commas, maps without braces) |       |       |   ✓   |

`generate.py` evaluates the capabilities for every dialect and writes the
files in `src/` (except `lib.rs`) into the `src/` directory of the dialect
crates, where they are committed.  Code of capabilities a dialect does not
have is removed, so the generated code is what one would write by hand for
the dialect: `deser-json` knows nothing about comments or JSON5.

```rust
match byte {
    b'"' => string_value!(start),
    #[cfg(json5)]
    b'\'' => string_value!(start),
    // ...
}
```

The build script of this crate enables the capabilities of JSON5, the
dialect with the most of them, so that editors analyze most of the code.
JSON5 and Hjson read unquoted keys and values differently and a dialect
can only be one of them, the code of Hjson is checked by compiling
`deser-hj`.  The code of a dialect is removed from the others, the
generated parsers of JSON, JSONC and JSON5 do not change when Hjson
changes.

## Workflow

* Edit the files in `src/` (never the generated files, they start with an
  `@generated` comment).
* Run `make codegen` (or `python3 deser-template-json/generate.py`).
* Test the generated crates (`cargo test -p deser-template-json`,
  see below).  Unit tests of the parser for a capability go into the
  template with the same `#[cfg]`.

`make format-check` (and CI) fails if the generated files are out of date.

## Tests

The integration tests of this crate (`tests/`) are the tests of reading
JSON, JSONC, JSON5 and Hjson.  Every test file is a module of the dialects
it applies to (see `tests/json.rs`, `tests/jsonc.rs`, `tests/json5.rs` and
`tests/hjson.rs`), so
it's compiled once per dialect and the tests show up as
`jsonc::test_de::test_strings`.  The files import the crate under test as
`dialect` and the capabilities of the dialect as `DIALECT` for the few
tests that differ:

```rust
use super::{DIALECT, dialect};

if !DIALECT.trailing_commas {
    assert!(dialect::from_str::<Vec<u32>>("[1,]").is_err());
}
```

The tests of a capability are in their own file which is only a module of
the dialects with the capability (`test_comments.rs`, `test_json5.rs` and
`test_hjson.rs`).  `generate.py` checks that the capabilities in the tests
match the dialects.  The tests of writing JSON are in `deser-json`, the
JSON5 test suite (with its vendored data) is in `deser-json5` and the
Hjson test suite in `deser-hj`.

## Template Syntax

The template is regular Rust.  The generator understands:

* `#[cfg(...)]` on items, statements, match arms, fields and parameters with
  conditions on capabilities (`all`, `any` and `not` work as usual).  If the
  condition is false, the node is removed together with the comments and
  attributes directly above it.  If it's true, the attribute is removed and
  a block statement is unwrapped:

  ```rust
  #[cfg(not(json5))]
  {
      return Err(...);
  }
  ```

* Conditions that mix capabilities with other conditions are simplified:
  `#[cfg(all(comments, feature = "io"))]` becomes `#[cfg(feature = "io")]`
  for `deser-jsonc` and removes the node for `deser-json`.
* `#[cfg_attr(capability, ...)]` and `cfg!(capability)` (which becomes
  `true` or `false`, prefer `#[cfg]` so no dead code is generated).
* `#![cfg(...)]` at the top of a file only generates the file for the
  dialects that match.
* Comments that start with `//#` are only in the template (for instance to
  explain why code is conditional).  Comments that start with
  `//#(capability)` are only in the dialects with the capability (as
  regular comments), for instance to explain how code that all dialects
  share handles comments.
* `deser_template_json` (in doc tests) becomes the name of the
  dialect crate.

A node ends at the first `;` or `,` outside of brackets (commas do not end
items like `fn`) or after a block which is not followed by `else`, `.` or
`?`.  `#[cfg]` cannot be used within expressions (as in Rust), use a
separate match arm or statement instead.
