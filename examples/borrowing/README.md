# borrowing

```
cargo run -p borrowing
```

## Why

Zero-copy deserialization avoids allocating a new string for every value
in the input. This example shows when deser can hand out slices of the
input and when it has to fall back to owned data. It also shows that
borrowing keeps working in situations where values have to be buffered,
such as internally tagged enums whose tag comes last.

## What it shows

- `&'a str` / `&'a [u8]`: these always borrow. If the input cannot be
  borrowed (for example, a JSON string with escapes), you get an error.
- `Cow<'a, str>` with `#[deser(as = Borrowed)]`: borrows when possible
  and becomes owned otherwise. It also works inside containers:
  `Vec<Borrowed>`.
- An internally tagged enum (`Token`) with `&str` fields, where the
  `"type"` tag can come after the fields. The buffered fields still point
  into the input.
- CBOR byte strings borrowed as `&[u8]`.
- `is_within` checks pointer ranges to show that a value really points
  into the input buffer.

## What you should see

1. The debug output of a `LogLine`.
2. Which `Cow` values were borrowed or owned:
   - `message: owned`, because `\"` escapes need unescaping.
   - `tags[0]: borrowed`.
   - `tags[1]: owned`, because of the `\u00e9` escape.
3. An error for `"\u0069nfo"` in a `&str` field: "unexpected owned string,
   expected a borrowed string".
4. The parsed token list and the round-tripped CBOR `Packet`.

The asserts check that the borrowed values really point into the input
buffer.

## How to read it

Compare the `LogLine` field types with the printed "borrowed"/"owned"
lines. In short: use `&str` if the input can never contain escapes, and
`Cow` + `Borrowed` if it might.

Related: `protocol` (borrowing in a real message type), `generic-types`
(enums with lifetimes).
