# bytes

```
cargo run -p bytes
```

## Why

Binary data is represented differently depending on the format. CBOR and
MessagePack have native byte strings. JSON and TOML do not, so bytes have
to be encoded somehow (base64, hex, integer arrays). This example shows
how to control that per format and per field without changing your
types. It also shows that reading is lenient.

## What it shows

The `Blob` struct has four byte fields, each with a different strategy:

| field       | annotation                    | JSON/TOML                  | CBOR/MessagePack |
|-------------|-------------------------------|----------------------------|------------------|
| `data`      | none                          | base64 (configurable)      | native bytes     |
| `digest`    | `as = Hex`                    | hex string                 | hex string       |
| `signature` | `as = BytesFallback<Hex>`     | hex string                 | native bytes     |
| `legacy`    | `as = BytesFallback<IntSeq>`  | array of integers          | native bytes     |

It also shows:

- A `BytesFormat` in the `Context` changes the default for plain bytes,
  for writing and reading. The example uses URL-safe base64 for JSON and
  integer arrays for TOML. Fields with an explicit adapter are not
  affected.
- Reading accepts standard and URL-safe base64 (with or without padding),
  as well as integer arrays, for plain bytes.
- `Hex` comes from `deser-encoding`. The base64 variants and `IntSeq`
  come from `deser::adapters`.

## What you should see

JSON and TOML output with the defaults, then again with the changed
format config (only `data` changes). Then hex dumps of the CBOR and
MessagePack encodings. In those dumps, `44deadbeef` / `c404deadbeef` is
the native byte string for `signature`. `digest` still shows up as a text
string (`7032636632...` / `b032636632...`).

## How to read it

Read the table above next to the output. The asserts in `main` pin the
exact bytes on the wire, and the comments explain the CBOR/MessagePack
type bytes.

Related: `formats` (the same idea for dates/UUIDs), `adapters`.
