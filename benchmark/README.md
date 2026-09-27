# Runtime Performance

This folder benchmarks deser against serde, miniserde and the serde based
libraries of the formats.

## Twitter JSON (`cargo bench`)

`benches/bench.rs` compares the deserialization of a JSON dump from Twitter
with serde, deser and miniserde.  Current results from running `make bench`:

```
test bench_deserialize_deser_json ... bench:     440,510.45 ns/iter (+/- 9,947.53)
test bench_deserialize_miniserde  ... bench:     439,822.95 ns/iter (+/- 10,659.55)
test bench_deserialize_serdejson  ... bench:     397,839.59 ns/iter (+/- 5,283.65)
test bench_serialize_deser_json   ... bench:     175,657.55 ns/iter (+/- 3,909.28)
test bench_serialize_miniserde    ... bench:     313,294.78 ns/iter (+/- 5,118.25)
test bench_serialize_serdejson    ... bench:     218,652.08 ns/iter (+/- 4,405.83)
```

## Benchmark Binary

The benchmark binary compares every format of deser with a serde library:

| format | deser        | serde          |
|--------|--------------|----------------|
| JSON   | `deser-json` | `serde_json`   |
| CBOR   | `deser-cbor` | `ciborium`     |
| YAML   | `deser-yaml` | `serde-saphyr` |
| TOML   | `deser-toml` | `toml`         |

Every dataset is serialized and deserialized with all four formats.  The
real world data comes from the benchmarks of the serde libraries (vendored
in `data/` with `scripts/update-benchmark-data.sh`):

* `twitter` (json-benchmark): a Twitter search result, strings and small
  integers.
* `canada` (json-benchmark): the border of Canada as GeoJSON, f32 pairs.
* `citm-catalog` (json-benchmark): a catalog of events, integers and maps
  keyed by ids.
* `cargo-manifest` (toml-rs): the manifest of cargo, untagged enums.
* `web-sys-manifest` (toml-rs): the manifest of web-sys with a table of
  about 1,600 features.
* `cargo-lock`: the `Cargo.lock` of serde-saphyr, an array of tables.
* `saphyr`: the YAML document of the serde-saphyr benchmark (generated, see
  `src/saphyr.rs`) which refers to shared values with anchors and aliases.

If a dataset comes from a document, the document is the input of its own
format (for instance `canada/json/de` reads the original `canada.json`).
The inputs of the other formats are serialized from the deserialized value
with deser.  deser and serde always deserialize the same input.  When the
data is loaded, every input is checked to deserialize to the same value
with both libraries.

Synthetic datasets (see `src/datasets.rs`) stress what the real world data
barely contains:

* `features`: f64 heavy (GeoJSON-like coordinates)
* `point-cloud`: f32 heavy
* `blobs`: many small byte buffers, plain and `BytesFallback<Hex>` (deser
  only, serde has no bytes for `Vec<u8>`)
* `registry`: many small `HashMap`s
* `tree`: deeply nested small containers

The benchmarks are named `DATASET/FORMAT/OP` where `OP` is `de` or `ser`
for deser and `de-serde` or `ser-serde` for serde.  `DATASET/events/ser`
only produces the events of the value with the serialize driver, without
a format.  It's the part of the cost of serialization that all formats
share.

Usage:

* `cargo run --release -- time [FILTER [ROUNDS]]` times all benchmarks (or
  those whose name contains `FILTER`).  The benchmarks run in interleaved
  rounds (default 5) and the best time is reported, which keeps the noise
  between runs mostly below 1%.  Running without arguments is the same as
  `time`.
* `cargo run --release -- versus [FILTER [ROUNDS]]` times the benchmarks
  like `time` and prints deser next to serde with the ratio of the times
  (below 1 deser is faster) and the geometric mean of the ratios by format
  and operation.  `make bench-versus` runs all of them.
  `cargo run --release -- table RESULTS` prints the same table for a saved
  output of `time`.
* `cargo run --release -- compare BASE NEW` compares two saved outputs of
  `time`:

  ```
  cargo run --release -- time > /tmp/base.txt
  # make changes
  cargo run --release -- time > /tmp/new.txt
  cargo run --release -- compare /tmp/base.txt /tmp/new.txt
  ```

* `cargo run --release -- loop NAME [N]` runs a single benchmark in a loop
  for profiling (for instance `loop tree/cbor/ser 1000`).  `de`, `ser`,
  `ignore`, `de-serde` and `ser-serde` are shortcuts for the Twitter JSON
  benchmarks (`cargo run --release -- de 1000`).
* `cargo run --release -- list` lists all benchmarks.
* `cargo run --release -- sizes` prints the sizes of the inputs.
* `cargo run --release -- interop` prints the sizes of the output of deser
  and serde and whether they read each other's output.  The serialization
  benchmarks of YAML do not produce the same output as the libraries use
  different styles.
* With the `count-allocs` feature, `allocs [FILTER]` counts the allocations
  of a single run of every benchmark.

### Results

The geometric mean of deser/serde over the 11 datasets, from
`make bench-versus` (Apple M5 Max, Rust 1.98).  Below 1 deser is faster.
The range shows the best and the worst dataset.

| format | serde library  | de    | de range    | ser   | ser range   |
|--------|----------------|-------|-------------|-------|-------------|
| JSON   | `serde_json`   | 1.35x | 0.92x-2.70x | 0.90x | 0.37x-2.17x |
| CBOR   | `ciborium`     | 0.92x | 0.59x-1.65x | 1.59x | 1.04x-2.87x |
| YAML   | `serde-saphyr` | 0.49x | 0.37x-0.86x | 0.70x | 0.27x-1.42x |
| TOML   | `toml`         | 0.61x | 0.44x-0.95x | 1.12x | 0.91x-1.96x |

Notes on the comparison:

* serde-saphyr is configured without its budget (`budget: None`) as the
  default budget rejects the larger documents and deser-yaml does not
  limit the input by default either.
* deser-json and serde_json (without its `float_roundtrip` feature) do not
  round all floats with 17 significant digits correctly.  The check of the
  inputs accepts floats that differ in the last bits.
