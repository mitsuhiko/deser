# streams

```
cargo run -p streams
```

## Why

Real programs read files and sockets, not only in-memory strings. Some
inputs hold a single value (a config file). Others are a stream of many
values (a log). For streams you want constant memory and to recover from
a bad record. This example shows both with `std::io` and converts a
stream between three formats.

## What it shows

- Single values: `deser_toml::to_writer` / `deser_toml::from_reader` with
  a `File`.
- Streams of values with the format-independent
  `deser::io::Writer::new(writer, config)` and
  `deser::io::Reader::new(reader, config)`. The format's
  (de)serializer config selects the format and framing:
  - JSON Lines: `trailing(Trailing::Newline)` on both configs.
  - A CBOR sequence: the default `deser_cbor` configs.
  - YAML multi-document (`---`): the default `deser_yaml` configs.
- `reader.read::<T>()` returns `Result<Option<T>>`. A bad JSON line is
  reported and skipped, and reading continues.
- `reader.iter::<T>()` for iterator-style consumption.
- `writer.get_mut()` for writing raw bytes (here, a deliberately broken
  line), and `flush` / `into_inner`.
- Only one value is buffered at a time.

It writes two temp files: `deser-streams-config.toml` and
`deser-streams-events.jsonl` in `std::env::temp_dir()`.

## What you should see

```
config: Config { name: "uploads", workers: 4 }
skipped: EndOfFile: unexpected end of file at line 3 column 35
80 bytes of CBOR
event: login
user: jane
---
event: upload
...
```

The broken third line is skipped. The other three events go from JSON
Lines to CBOR to YAML documents, and are finally read back from YAML
(checked with an assert).

## How to read it

Follow the data: TOML file → JSON Lines file → CBOR `Vec<u8>` → YAML
`String`. Each step swaps only the config passed to `Reader`/`Writer`.

Related: `json-lines` (the in-memory version), `tokio-server` (the async
version).
