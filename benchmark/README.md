# Runtime Performance

This folder compares every format of deser with a serde based library on
the same data and types: `deser-json` with `serde_json`, `deser-cbor` with
`ciborium`, `deser-msgpack` with `rmp-serde`, `deser-yaml` with
`serde-saphyr` and `deser-toml` with `toml`.  The results below are from
`make bench-versus` on an Apple M5 Max with Rust 1.98 (they do not include
MessagePack yet).  Ratios are deser/serde, below 1 deser is faster.

## Where deser Stands

The geometric mean over the 11 datasets, with the best and the worst one:

| format | de    | de range    | ser   | ser range   |
|--------|-------|-------------|-------|-------------|
| JSON   | 1.29x | 0.98x-1.68x | 0.91x | 0.33x-1.70x |
| CBOR   | 0.87x | 0.60x-1.31x | 1.32x | 1.01x-1.93x |
| YAML   | 0.31x | 0.27x-0.43x | 0.57x | 0.24x-1.02x |
| TOML   | 0.50x | 0.39x-0.81x | 0.96x | 0.76x-1.42x |

* **YAML and TOML** deserialize two to four times as fast as serde-saphyr
  and toml.  Serializing is faster or on par, except for the large table
  of web-sys-manifest in TOML (1.42x).
* **JSON** serializes faster than serde_json.  Deserializing is on par for
  string heavy data (twitter, citm-catalog, saphyr) but 1.40x-1.68x slower
  for floats and nesting (canada, features, point-cloud, tree).
* **CBOR** deserializes on par with ciborium, serializing is 1.3x slower.
* **Untagged enums** (cargo-manifest) are 1.44x slower in JSON, in the
  other formats they are faster.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 449.4 us | 163.6 us |
| serde_json | 409.9 us | 219.1 us |
| miniserde  | 441.1 us | 309.4 us |

## Areas of Interest

* **The cost of an event in the deserialize driver.**  This is what
  separates JSON (and CBOR) from serde on floats and nesting, the parsers
  themselves are about as fast.  Every nested container gets a boxed sink
  (a `[f64; 2]` included), elements go through a slot before they are
  pushed and every event is a virtual call.  Allocating and freeing the
  boxes from the block cache (including two thread local lookups on
  macOS) is about 6% of tree and canada.  Storing sinks of up to 32 or 128
  bytes inline in their handles (moved to blocks of the driver while their
  containers are open) was tried and is 5%-25% slower: the handles are
  copied several times on the way to the driver, and driving the sinks
  through raw pointers instead of the handles alone costs 3%-6%.
* **The serialize driver.**  `DATASET/events/ser` only produces the events
  of a value, without a format.  It is more than half of the time of CBOR
  on canada (335 of 631 us) and tree (598 of 818 us), which is why CBOR
  serializes slower than ciborium.
* **Untagged enums** buffer the value and replay it for every variant.
* **YAML** is still about six times slower than JSON on the same data.
  The remaining cost is spread over the libyaml style token machinery
  (simple key tracking, a queue of tokens) and large tokens and events
  that are moved by value.
* **TOML** documents are parsed into a tree (tables can be defined out of
  order), which costs one allocation per inline array (58k for canada).
  An arena was tried and did not pay off.
* **Exact numbers** in JSON (on by default) pass floats with more than 15
  digits or an exponent as number extension values.  Float sinks read
  their values with a single dynamic call, which makes this as fast as
  plain floats (canada).

## Full Results

| benchmark             | de       | serde    | ratio | ser      | serde    | ratio |
|-----------------------|----------|----------|-------|----------|----------|-------|
| twitter/json          | 453.7 us | 397.8 us | 1.14x | 159.9 us | 222.0 us | 0.72x |
| canada/json           | 2.98 ms  | 2.12 ms  | 1.40x | 1.24 ms  | 1.22 ms  | 1.02x |
| citm-catalog/json     | 951.8 us | 971.9 us | 0.98x | 322.8 us | 255.3 us | 1.26x |
| cargo-manifest/json   | 13.8 us  | 9.6 us   | 1.44x | 3.1 us   | 2.2 us   | 1.41x |
| web-sys-manifest/json | 204.5 us | 177.7 us | 1.15x | 23.8 us  | 27.8 us  | 0.86x |
| cargo-lock/json       | 43.7 us  | 39.4 us  | 1.11x | 13.3 us  | 20.3 us  | 0.66x |
| saphyr/json           | 402.3 us | 364.1 us | 1.10x | 102.3 us | 307.1 us | 0.33x |
| features/json         | 3.35 ms  | 2.16 ms  | 1.55x | 1.63 ms  | 1.77 ms  | 0.92x |
| point-cloud/json      | 1.95 ms  | 1.24 ms  | 1.58x | 1.07 ms  | 1.25 ms  | 0.86x |
| registry/json         | 1.37 ms  | 1.09 ms  | 1.25x | 247.3 us | 217.9 us | 1.13x |
| tree/json             | 2.28 ms  | 1.35 ms  | 1.68x | 962.2 us | 565.2 us | 1.70x |
| twitter/cbor          | 391.2 us | 513.1 us | 0.76x | 132.7 us | 119.7 us | 1.11x |
| canada/cbor           | 1.52 ms  | 1.51 ms  | 1.01x | 618.2 us | 393.6 us | 1.57x |
| citm-catalog/cbor     | 709.4 us | 625.3 us | 1.13x | 275.5 us | 177.0 us | 1.56x |
| cargo-manifest/cbor   | 13.0 us  | 15.4 us  | 0.84x | 2.9 us   | 1.5 us   | 1.93x |
| web-sys-manifest/cbor | 198.1 us | 249.6 us | 0.79x | 22.3 us  | 19.4 us  | 1.15x |
| cargo-lock/cbor       | 38.1 us  | 62.4 us  | 0.61x | 11.9 us  | 9.9 us   | 1.20x |
| saphyr/cbor           | 339.7 us | 570.5 us | 0.60x | 95.3 us  | 67.3 us  | 1.42x |
| features/cbor         | 1.49 ms  | 1.14 ms  | 1.31x | 554.2 us | 465.0 us | 1.19x |
| point-cloud/cbor      | 1.04 ms  | 1.15 ms  | 0.91x | 430.3 us | 424.7 us | 1.01x |
| registry/cbor         | 999.8 us | 1.46 ms  | 0.69x | 244.7 us | 236.9 us | 1.03x |
| tree/cbor             | 2.04 ms  | 1.72 ms  | 1.18x | 800.0 us | 470.1 us | 1.70x |
| twitter/yaml          | 2.48 ms  | 9.22 ms  | 0.27x | 1.39 ms  | 3.45 ms  | 0.40x |
| canada/yaml           | 14.94 ms | 46.62 ms | 0.32x | 3.43 ms  | 3.36 ms  | 1.02x |
| citm-catalog/yaml     | 5.19 ms  | 18.98 ms | 0.27x | 1.92 ms  | 3.33 ms  | 0.58x |
| cargo-manifest/yaml   | 37.1 us  | 126.7 us | 0.29x | 19.9 us  | 30.6 us  | 0.65x |
| web-sys-manifest/yaml | 498.2 us | 1.53 ms  | 0.33x | 198.6 us | 463.6 us | 0.43x |
| cargo-lock/yaml       | 195.9 us | 649.6 us | 0.30x | 127.6 us | 325.3 us | 0.39x |
| saphyr/yaml           | 2.30 ms  | 5.39 ms  | 0.43x | 1.37 ms  | 5.68 ms  | 0.24x |
| features/yaml         | 15.27 ms | 44.66 ms | 0.34x | 3.89 ms  | 3.91 ms  | 0.99x |
| point-cloud/yaml      | 10.03 ms | 32.76 ms | 0.31x | 2.66 ms  | 2.78 ms  | 0.96x |
| registry/yaml         | 4.28 ms  | 14.17 ms | 0.30x | 1.31 ms  | 2.27 ms  | 0.58x |
| tree/yaml             | 17.31 ms | 59.74 ms | 0.29x | 4.76 ms  | 7.47 ms  | 0.64x |
| twitter/toml          | 819.4 us | 1.77 ms  | 0.46x | 1.14 ms  | 1.07 ms  | 1.06x |
| canada/toml           | 5.15 ms  | 10.57 ms | 0.49x | 3.87 ms  | 3.95 ms  | 0.98x |
| citm-catalog/toml     | 2.02 ms  | 4.61 ms  | 0.44x | 2.21 ms  | 2.77 ms  | 0.80x |
| cargo-manifest/toml   | 16.6 us  | 20.6 us  | 0.81x | 14.6 us  | 13.2 us  | 1.11x |
| web-sys-manifest/toml | 299.8 us | 449.5 us | 0.67x | 207.4 us | 146.0 us | 1.42x |
| cargo-lock/toml       | 76.1 us  | 143.6 us | 0.53x | 118.2 us | 116.8 us | 1.01x |
| saphyr/toml           | 632.4 us | 1.54 ms  | 0.41x | 1.96 ms  | 1.78 ms  | 1.10x |
| features/toml         | 6.22 ms  | 13.56 ms | 0.46x | 4.13 ms  | 5.40 ms  | 0.76x |
| point-cloud/toml      | 3.83 ms  | 8.94 ms  | 0.43x | 2.78 ms  | 3.37 ms  | 0.83x |
| registry/toml         | 2.00 ms  | 3.66 ms  | 0.55x | 1.38 ms  | 1.50 ms  | 0.92x |
| tree/toml             | 6.25 ms  | 15.92 ms | 0.39x | 6.16 ms  | 7.94 ms  | 0.78x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 765/528 us in JSON, 502/190 us in CBOR, 4.48/1.27 ms in YAML
and 1.15/2.28 ms in TOML.  Ignoring the Twitter JSON document
(`twitter/json/ignore`) takes 287 us, 1.11x of serde_json.

## Datasets

The real world data comes from the benchmarks of the serde libraries
(vendored in `data/` with `scripts/update-benchmark-data.sh`):

* `twitter`: a Twitter search result, strings and small integers.
* `canada`: the border of Canada as GeoJSON, f32 pairs.
* `citm-catalog`: a catalog of events, integers and maps keyed by ids.
* `cargo-manifest`: the manifest of cargo, untagged enums.
* `web-sys-manifest`: the manifest of web-sys, a table of 1,600 features.
* `cargo-lock`: the `Cargo.lock` of serde-saphyr, an array of tables.
* `saphyr`: the YAML document of the serde-saphyr benchmark with anchors
  and aliases (generated, see `src/saphyr.rs`).
* `github`: responses of the GitHub REST API (pages of issues, pull
  requests, repositories, workflow runs, commits and releases) built from
  the examples of its OpenAPI description.  Many optional fields and nulls,
  nested users, enums and timestamps.
* `kubernetes`: the OpenAPI description of the Kubernetes API.  Large maps,
  recursive schemas, references as untagged enum, optional fields.

Synthetic data (see `src/datasets.rs`): `features` (f64 heavy), `point-cloud`
(f32 heavy), `blobs` (small byte buffers), `registry` (small `HashMap`s)
and `tree` (deeply nested small containers).  `manifests` are Kubernetes
manifests (internally tagged by `kind`, flattened fields, see
`src/manifests.rs`) and `logs` are 5,000 structured log events of a few
hundred bytes which are read and written one by one, which measures what
a document costs (see `src/logs.rs`).

A document is the input of its own format, the inputs of the other formats
are serialized from it with deser.  Both libraries read the same input and
the results are checked to be equal when the data is loaded.
serde-saphyr runs without its budget (`budget: None`) as the default
rejects the larger documents.  rmp-serde writes structs as maps
(`to_vec_named`) like deser does, its default is arrays.  deser-json and serde_json (without
`float_roundtrip`) do not round all floats with 17 digits correctly, the
check accepts differences in the last bits.

## Running

Benchmarks are named `DATASET/FORMAT/OP` with `OP` being `de` or `ser`
(`de-serde` and `ser-serde` for serde).  `DATASET/events/ser` only runs the
serialize driver without a format.  Always use `--release`:

* `make bench-versus` or `cargo run --release -- versus [FILTER [ROUNDS]]`
  prints the comparison with serde.  Benchmarks run in interleaved rounds
  (default 5) and the best time is reported.
* `cargo run --release -- time [FILTER [ROUNDS]] > FILE` saves the times,
  `table FILE` prints the comparison for them and `compare BASE NEW`
  compares two saved runs.
* `cargo run --release -- loop NAME [N]` runs one benchmark in a loop for
  profiling.
* `list`, `sizes` and `interop` list the benchmarks, print the input sizes
  and check that deser and serde read each other's output.  With the
  `count-allocs` feature `allocs [FILTER]` counts allocations.
* `make bench` runs `cargo bench` (Twitter JSON with miniserde).
