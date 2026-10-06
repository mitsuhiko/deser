# Fuzzing

Fuzz targets for the format crates, run with
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (which needs a
nightly compiler):

```sh
cargo install cargo-fuzz
cd fuzz
python3 seed-corpus.py              # seeds corpus/ from the test data
cargo +nightly fuzz run json        # runs until it finds a problem
cargo +nightly fuzz run json -- -max_total_time=60 -max_len=4096
```

`make fuzz` (in the root of the repository) runs every target for a
minute (`FUZZ_TIME` sets the seconds, `FUZZ_TARGETS` the targets).

## Targets

There is a target for every format crate: `cbor`, `csv`, `env`, `hj`, `ini`,
`json`, `json5`, `jsonc`, `msgpack`, `php`, `pickle`, `plist`, `toml`,
`urlencoded`, `xml` and `yaml`.  The input is deserialized into dynamic
values (`deser_value::Value`), recordings and a derived type that uses most
features of the derive (`src/typed.rs`), and the results are serialized
again.  Besides crashes the targets check that:

* the stream deserializer of the format finds the same values as the
  deserializer of complete inputs, no matter how the input is split into
  chunks, both for values that are deserialized while their input arrives
  and for values that are read from their frames.
* the output of the serializer can be deserialized again, and that values
  survive further round trips unchanged (see `check_roundtrip` in
  `src/lib.rs`), also through the derived type.
* raw values (JSON, JSONC, JSON5, CBOR and MessagePack) accept what the
  format accepts and hold the same values (see `check_raw`).

The `env` target reads its input as lines of `NAME=VALUE`.

`serialize` generates dynamic values (`src/generate.rs`), serializes them
with every format (and `deser-debug`) and checks that the output can be
deserialized again (like the round trips above).  `transcode` does the
same with values that one format deserialized and another serializes.  Its
input starts with the indexes of the two formats and the flags of the
serializer.

## Inputs

The inputs of the format targets start with a header of 9 bytes: the flags
of the deserializer (a little endian `u32`) and of the serializer (another
`u32`), and the seed of the chunk sizes for the stream deserializer.  A
header of zeroes is the default configuration with the input in one
chunk.  The flags pick the options of the configurations (see
`src/formats.rs`), the upper bits of the deserializer flags enable
location tracking, limits and the policies for duplicate keys (see
`context` in `src/lib.rs`).  This lets the fuzzer explore the configurations, the seed
corpus starts with the defaults.

To look at a crash, run the target with the input:

```sh
cargo +nightly fuzz run json artifacts/json/crash-...
```
