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
| JSON   | 1.36x | 0.95x-2.62x | 0.84x | 0.36x-1.42x |
| CBOR   | 0.91x | 0.60x-1.59x | 1.32x | 1.00x-2.07x |
| YAML   | 0.32x | 0.27x-0.43x | 0.61x | 0.25x-1.34x |
| TOML   | 0.51x | 0.37x-1.24x | 0.96x | 0.76x-1.47x |

* **YAML and TOML** deserialize two to four times as fast as serde-saphyr
  and toml.  Serializing is faster or on par, except for float heavy
  YAML (canada, features, point-cloud: 1.2x-1.3x) and the large table of
  web-sys-manifest in TOML (1.47x).
* **JSON** serializes faster than serde_json.  Deserializing is on par for
  string heavy data (twitter, citm-catalog, saphyr) but 1.45x-1.67x slower
  for floats and nesting (canada, features, point-cloud, tree).
* **CBOR** deserializes on par with ciborium, serializing is 1.3x slower.
* **Untagged enums** (cargo-manifest) are slower in every format but YAML
  (1.2x-2.6x) as they buffer the value.

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
  pushed and every event is a virtual call.  On macOS every box also costs
  two thread local lookups for the block cache (about 2%).  Storing small
  sinks inline or keeping the cache in the driver would help.
* **The serialize driver.**  `DATASET/events/ser` only produces the events
  of a value, without a format.  It is more than half of the time of CBOR
  on canada (335 of 631 us) and tree (598 of 818 us), which is why CBOR
  serializes slower than ciborium.
* **Untagged enums** buffer the value and replay it for every variant.
* **YAML** is still about six times slower than JSON on the same data.
  The remaining cost is spread over the libyaml style token machinery
  (simple key tracking, a queue of tokens) and large tokens and events
  that are moved by value.  Serializing floats is slower than
  serde-saphyr.
* **TOML** documents are parsed into a tree (tables can be defined out of
  order), which costs one allocation per inline array (58k for canada).
  An arena was tried and did not pay off.
* **Exact numbers** in JSON (on by default) pass floats with more than 15
  digits or an exponent as number extension values, which costs 4%-9% on
  float heavy data.

## Full Results

| benchmark             | de       | serde    | ratio | ser      | serde    | ratio |
|-----------------------|----------|----------|-------|----------|----------|-------|
| twitter/json          | 439.4 us | 424.8 us | 1.03x | 165.4 us | 286.8 us | 0.58x |
| canada/json           | 3.06 ms  | 2.11 ms  | 1.45x | 1.25 ms  | 1.26 ms  | 0.99x |
| citm-catalog/json     | 906.1 us | 952.8 us | 0.95x | 336.4 us | 295.7 us | 1.14x |
| cargo-manifest/json   | 24.9 us  | 9.5 us   | 2.62x | 3.1 us   | 2.6 us   | 1.19x |
| web-sys-manifest/json | 208.7 us | 175.1 us | 1.19x | 23.3 us  | 30.4 us  | 0.77x |
| cargo-lock/json       | 43.2 us  | 37.1 us  | 1.16x | 12.9 us  | 20.0 us  | 0.65x |
| saphyr/json           | 396.7 us | 364.7 us | 1.09x | 115.8 us | 324.8 us | 0.36x |
| features/json         | 3.43 ms  | 2.13 ms  | 1.61x | 1.63 ms  | 1.78 ms  | 0.92x |
| point-cloud/json      | 1.92 ms  | 1.22 ms  | 1.58x | 1.07 ms  | 1.26 ms  | 0.85x |
| registry/json         | 1.36 ms  | 1.10 ms  | 1.24x | 249.4 us | 244.9 us | 1.02x |
| tree/json             | 2.24 ms  | 1.33 ms  | 1.67x | 965.8 us | 679.0 us | 1.42x |
| twitter/cbor          | 392.2 us | 507.6 us | 0.77x | 138.3 us | 120.0 us | 1.15x |
| canada/cbor           | 1.48 ms  | 1.53 ms  | 0.96x | 631.3 us | 434.4 us | 1.45x |
| citm-catalog/cbor     | 681.1 us | 623.5 us | 1.09x | 285.7 us | 188.5 us | 1.52x |
| cargo-manifest/cbor   | 24.1 us  | 15.2 us  | 1.59x | 2.9 us   | 1.4 us   | 2.07x |
| web-sys-manifest/cbor | 204.0 us | 239.8 us | 0.85x | 21.6 us  | 18.8 us  | 1.15x |
| cargo-lock/cbor       | 37.8 us  | 60.5 us  | 0.62x | 11.5 us  | 9.2 us   | 1.25x |
| saphyr/cbor           | 327.6 us | 548.7 us | 0.60x | 106.9 us | 78.2 us  | 1.37x |
| features/cbor         | 1.43 ms  | 1.15 ms  | 1.24x | 563.3 us | 466.0 us | 1.21x |
| point-cloud/cbor      | 1.01 ms  | 1.13 ms  | 0.89x | 442.4 us | 426.1 us | 1.04x |
| registry/cbor         | 992.1 us | 1.45 ms  | 0.69x | 237.0 us | 236.2 us | 1.00x |
| tree/cbor             | 2.02 ms  | 1.67 ms  | 1.21x | 818.0 us | 495.0 us | 1.65x |
| twitter/yaml          | 2.47 ms  | 8.91 ms  | 0.28x | 1.41 ms  | 3.53 ms  | 0.40x |
| canada/yaml           | 14.70 ms | 46.12 ms | 0.32x | 4.36 ms  | 3.38 ms  | 1.29x |
| citm-catalog/yaml     | 5.10 ms  | 18.81 ms | 0.27x | 1.98 ms  | 3.36 ms  | 0.59x |
| cargo-manifest/yaml   | 48.6 us  | 124.6 us | 0.39x | 19.5 us  | 30.8 us  | 0.63x |
| web-sys-manifest/yaml | 497.2 us | 1.51 ms  | 0.33x | 197.0 us | 465.4 us | 0.42x |
| cargo-lock/yaml       | 190.2 us | 650.0 us | 0.29x | 122.0 us | 325.8 us | 0.37x |
| saphyr/yaml           | 2.27 ms  | 5.32 ms  | 0.43x | 1.37 ms  | 5.39 ms  | 0.25x |
| features/yaml         | 15.19 ms | 44.30 ms | 0.34x | 4.75 ms  | 3.94 ms  | 1.21x |
| point-cloud/yaml      | 9.89 ms  | 32.26 ms | 0.31x | 3.73 ms  | 2.78 ms  | 1.34x |
| registry/yaml         | 4.30 ms  | 14.00 ms | 0.31x | 1.35 ms  | 2.38 ms  | 0.57x |
| tree/yaml             | 17.20 ms | 59.54 ms | 0.29x | 4.70 ms  | 7.90 ms  | 0.59x |
| twitter/toml          | 813.7 us | 2.07 ms  | 0.39x | 1.14 ms  | 1.06 ms  | 1.07x |
| canada/toml           | 5.17 ms  | 10.30 ms | 0.50x | 3.89 ms  | 3.96 ms  | 0.98x |
| citm-catalog/toml     | 1.97 ms  | 4.33 ms  | 0.46x | 2.20 ms  | 2.70 ms  | 0.81x |
| cargo-manifest/toml   | 25.4 us  | 20.5 us  | 1.24x | 14.2 us  | 12.8 us  | 1.11x |
| web-sys-manifest/toml | 303.1 us | 451.8 us | 0.67x | 212.0 us | 144.0 us | 1.47x |
| cargo-lock/toml       | 73.9 us  | 145.0 us | 0.51x | 115.9 us | 118.5 us | 0.98x |
| saphyr/toml           | 623.8 us | 1.49 ms  | 0.42x | 1.72 ms  | 1.76 ms  | 0.97x |
| features/toml         | 6.10 ms  | 14.65 ms | 0.42x | 4.29 ms  | 5.29 ms  | 0.81x |
| point-cloud/toml      | 3.78 ms  | 10.36 ms | 0.37x | 2.85 ms  | 3.31 ms  | 0.86x |
| registry/toml         | 1.98 ms  | 3.64 ms  | 0.54x | 1.37 ms  | 1.49 ms  | 0.92x |
| tree/toml             | 6.33 ms  | 14.28 ms | 0.44x | 6.09 ms  | 7.98 ms  | 0.76x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 732/524 us in JSON, 482/187 us in CBOR, 4.50/1.27 ms in YAML
and 1.15/2.20 ms in TOML.  Ignoring the Twitter JSON document
(`twitter/json/ignore`) takes 283 us, 1.12x of serde_json.

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
