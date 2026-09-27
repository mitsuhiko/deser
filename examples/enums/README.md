# enums

```
cargo run -p enums
```

## Why

Enums are where serialization formats vary the most. This example is a
tour of every enum representation deser supports. The representations
match the ones known from serde, so you can map existing serde code 1:1.
It also shows the extras around them: `other` catch-all variants,
`untagged` fallback variants inside a tagged enum, and flattened shared
fields in variants.

## What it shows

| enum      | attributes                                          | JSON shape                                  |
|-----------|-----------------------------------------------------|---------------------------------------------|
| `Command` | default (externally tagged)                         | `"quit"`, `{"move": {...}}`                 |
| `Event`   | `tag = "type"`, `rename_all_fields = "camelCase"`   | `{"type": "login", "userName": ...}`        |
| `Message` | `tag = "kind", content = "data"`                    | `{"kind": "text", "data": "hi"}`            |
| `Shape`   | `tag = "shape"` + `flatten` + `#[deser(untagged)]`  | tagged objects, or a bare number (legacy)   |
| `Value<T>`| `untagged`                                          | whatever shape matches first                |

Also shown:

- `#[deser(other)] Unknown` in `Event` catches unknown tags such as
  `"something_new"`.
- `Event::Purchase(Purchase)` is a newtype variant whose inner struct
  fields sit next to the tag.
- The tag does not have to come first in the input. deser buffers the
  content until it knows the variant.

## What you should see

For each representation there is an `input:`, `parsed:` (Rust `Debug`)
and `output:` (re-serialized JSON) block. Things to notice:

- In "internally tagged", the unknown event parses as `Unknown` and is
  written back as `{"type":"unknown"}`. Its content is dropped. See
  `protocol` for how to keep it.
- In "flattened fields and untagged variants", the bare `4.0` becomes
  `LegacyCircle(4.0)`.
- The output always puts the tag first, even when the input did not.

## How to read it

Read each enum definition, then its block in the output. The `show`
helper is generic: copy it to experiment with your own input.

Related: `protocol` (integer tags, lossless unknown variants), `renames`
(tag/content aliases), `debug`.
