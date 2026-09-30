# raw-values

```
cargo run -p raw-values
```

## Why

Sometimes a part of a document should not be deserialized right away:
its type depends on another field (like the `kind` of a message), it's
passed on to someone else, or it should be stored exactly as it came in.
`deser_json::RawJson` holds the JSON text of such a value,
`deser_cbor::RawCbor` its CBOR encoding.

When the value comes from JSON, `RawJson` keeps its text. The value is
only validated, which is faster than parsing it into events, and the text
is written out again unchanged. Values from other formats are encoded as
JSON, so a `RawJson` always holds JSON. serde_json has the same feature,
but it's implemented with a magic struct name that the format has to
recognize. Everything that does not know the name (buffers of untagged
enums and flattened fields, other formats) sees a struct with a strange
field. In deser the input travels as an extension value.

## What it shows

- `#[deser(as = Borrowed)] payload: RawJson<'a>` borrows the text from
  the input. `get()` returns it exactly as it was (whitespace, `\u00fc`
  escapes, `1.50` and `-2e3`).
- `payload.deserialize::<T>()` deserializes the payload once its type is
  known. The message borrows its tags from the raw value.
- Serializing to JSON writes the raw text, serializing to YAML writes
  the value it holds.
- `RawJson<'static>` copies the text and outlives the input.
- A `RawJson` read from YAML holds the value encoded as JSON.
- `RawCbor` keeps the CBOR encoding as it was (an indefinite length array
  with a non-minimal integer), writes it back unchanged and converts to
  JSON like any other value.

## What you should see

For every envelope the raw text of the payload and its value, then the
envelopes as JSON (the payloads unchanged) and as YAML (the payloads as
YAML), then the owned raw value, the one read from YAML and the CBOR
record.

## How to read it

Compare the `raw:` lines with `INPUT`, the JSON output with the YAML
output and the CBOR payload with the input bytes.

Related: `json-numbers` (exact numbers), `borrowing`.
