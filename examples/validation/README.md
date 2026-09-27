# validation

```
cargo run -p validation
```

## Why

Input that parses can still be wrong: an email address without a domain,
a quantity of zero, an empty street.  Checking that after deserialization
loses where the value was in the input.  `deser-validate` validates while
deserializing, so problems point at the value (path, line and column) like
type errors do.

There are two common situations.  A website shows a form again with the
values the user entered and an error next to each invalid field, so the
form must deserialize even if fields are invalid.  An API rejects a
request with all of its problems at once, not just the first one.

## What it shows

- `Validated<T, V>` fields that keep their errors (and the value, if it
  could be read) so an HTML form submitted as query string deserializes
  and can be shown again.
- Custom validators made with `validator!`: `Slug` from a plain function
  (`validator!(pub Slug(value: &str) = check_slug)`) and `Quantity` from a
  condition and a message.  The name of the validator is the code of its
  violations (`slug`, `quantity`).  They are used next to the built-in
  ones (`Email`, `Len`, `Range`, `NonEmpty`, `MaxLen`, `Each`) and in
  tuples that combine them.
- The `Check` adapter (`#[deser(as = Check<Slug>)]`) which rejects invalid
  values while the field keeps its type, `Checked<T, V>` fields which are
  always valid, and a `Validation` that reports all problems of a JSON
  request with paths, violation codes and lines, including type errors
  and missing fields.
- `Validated<Collect<T>>` to collect all errors of one part of the input.
- `track_locations` on the JSON deserializer, so that the errors values
  keep have lines and columns too (without it they only have offsets,
  `Report::resolve_positions` resolves them).

## What you should see

```
-- HTML form --
username: "Jane Doe" <- invalid value: may only contain lowercase letters, digits and dashes
email: "jane@example.com"
age: "" <- invalid value "eleven", expected u8
-- JSON API --
customer [email]: invalid value: must be an email address (line 2)
lines[1].sku [slug]: invalid value: may only contain lowercase letters, digits and dashes (line 5)
lines[1].quantity [quantity]: invalid value: must be between 1 and 100 (line 5)
lines[2].quantity [invalid_type]: unexpected string, expected u32 (line 6)
shipping.street [non_empty]: invalid value: must not be empty (line 8)
shipping.zip [length]: invalid value: length must be between 4 and 10 (line 8)
shipping [invalid_type]: missing field `city` (line 8)
gift_message [length]: invalid value: item 0: length must be at most 20 (line 10)
shipping has 3 problems
```

## How to read it

Start with `check_slug` and the `validator!` lines at the top, then
`SignupForm` and `field` for the form, then `Order` and `api` for the
report.  The asserts check every line of the output.

Related: `config-errors` (errors with line, column and path),
`input-contracts`, `adapters` (adapters that tolerate errors).
