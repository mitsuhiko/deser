# protocol

```
cargo run -p protocol
```

## Why

Real wire protocols often use integer opcodes instead of variant names.
They have to accept old field names from older clients, and they must
not destroy messages from newer clients that they don't understand yet.
This example is a chat relay: it parses each message, handles the ones
it knows and forwards everything else byte-for-byte.

## What it shows

- Integer tags: `#[deser(tag = "op")]` with `#[deser(rename = 1)]` etc.
  on the variants, so the tag is `"op": 1`.
- `#[deser(repr)]` + `#[repr(u16)]` on `ErrorCode`: the enum is written
  as its discriminant (`429`). Unknown codes are rejected with a list of
  the known ones.
- `#[deser(tag_alias = "type")]` also accepts old clients that sent
  `"type": 1`. The relay always writes `op`.
- Lossless unknown messages: `#[deser(other)] Unknown(#[deser(tag)] u64,
  Recording)` keeps the opcode and a `deser::de::Recording` of the
  content. Serializing it reproduces the original message.
- Zero-copy: `&'a str` fields and a `Cow` with `Borrowed`. Asserts check
  that nothing was copied.
- The same types with CBOR and MessagePack, where integer tags are real
  integers on the wire.

## What you should see

Pairs of `received:` / `sent:` lines. Things to notice:

- `{"type": 1, ...}` is received and sent back as `{"op":1, ...}`.
- `unknown op 7 with {"room":...,"reaction":{...}}` is forwarded exactly
  as it came in, emoji included.
- `error: ... unknown variant `500` of ErrorCode, expected one of `400`,
  `403`, `429``.
- `CBOR: 13 bytes` and `MessagePack: 13 bytes`, each round-tripping to
  `Error { code: Forbidden }`.

## How to read it

`relay()` is the core: parse, inspect, serialize. The `Message` enum
definition shows how the protocol is described.

Related: `enums` (the basic representations), `borrowing`, `renames`
(aliases).
