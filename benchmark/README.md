# Runtime Performance

This folder compares every format of deser with a serde based library on
the same data and types: `deser-json` with `serde_json`, `deser-cbor` with
`ciborium`, `deser-msgpack` with `rmp-serde`, `deser-yaml` with
`serde-saphyr`, `deser-toml` with `toml` and `deser-csv` with `csv`.  The results below are from
`make bench-versus` on an Apple M5 Max with Rust 1.98.  Ratios are
deser/serde, below 1 deser is faster.

## Where deser Stands

The geometric mean over the 15 datasets, with the best and the worst one:

| format      | de    | de range    | ser   | ser range   |
|-------------|-------|-------------|-------|-------------|
| JSON        | 1.21x | 0.75x-1.68x | 0.87x | 0.33x-1.50x |
| CBOR        | 0.83x | 0.53x-1.33x | 1.42x | 1.04x-2.10x |
| MessagePack | 1.46x | 0.86x-2.74x | 1.30x | 0.92x-1.85x |
| YAML        | 0.30x | 0.23x-0.44x | 0.56x | 0.23x-1.00x |
| TOML        | 0.51x | 0.43x-0.82x | 0.94x | 0.71x-1.41x |

* **YAML and TOML** deserialize two to four times as fast as serde-saphyr
  and toml.  Serializing is faster or on par, except for the large table
  of web-sys-manifest in TOML (1.41x).
* **JSON** serializes faster than serde_json, except for tree (1.50x) and
  logs (1.39x).  Deserializing is on par or faster for string heavy data
  (twitter, citm-catalog, manifests, logs) but 1.34x-1.68x slower for
  floats and nesting (canada, features, point-cloud, tree, kubernetes).
* **CBOR** deserializes faster than ciborium, serializing is 1.4x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 2.2x-2.7x slower for floats, nesting and integer keyed
  maps (canada, features, point-cloud, tree, citm-catalog).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.3x slower.
* **Untagged enums** (cargo-manifest) are 1.45x slower in JSON and 1.58x
  in MessagePack, in the other formats they are faster.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 444.5 us | 161.4 us |
| serde_json | 398.4 us | 219.0 us |
| miniserde  | 439.8 us | 313.3 us |

## Areas of Interest

* **The cost of an event in the deserialize driver.**  This is what
  separates JSON, CBOR and MessagePack from serde on floats and nesting,
  the parsers themselves are about as fast.  Every nested container gets
  a boxed sink (a `[f64; 2]` included), elements go through a slot before
  they are pushed and every event is a virtual call.  Allocating and freeing the
  boxes from the block cache (including two thread local lookups on
  macOS) is about 6% of tree and canada.  Storing sinks of up to 32 or 128
  bytes inline in their handles (moved to blocks of the driver while their
  containers are open) was tried and is 5%-25% slower: the handles are
  copied several times on the way to the driver, and driving the sinks
  through raw pointers instead of the handles alone costs 3%-6%.
* **The serialize driver.**  `DATASET/events/ser` only produces the events
  of a value, without a format.  It is more than half of the time of CBOR
  on canada (329 of 611 us) and tree (595 of 802 us), which is why CBOR
  and MessagePack serialize slower than ciborium and rmp-serde.
* **Untagged enums** buffer the value and replay it for every variant.
* **YAML** is still about six times slower than JSON on the same data.
  The remaining cost is spread over the libyaml style token machinery
  (simple key tracking, a queue of tokens) and large tokens and events
  that are moved by value.
* **TOML** documents are parsed into a tree (tables can be defined out of
  order), which costs one allocation per inline array (58k for canada).
  An arena was tried and did not pay off.
* **CSV** is compared with the `csv` crate on a table only (see
  `src/table.rs`): reading is 1.60x (5.91 ms vs 3.69 ms) and writing 2.87x
  (4.76 ms vs 1.66 ms) of it.  The parser finds the fields of a record in
  one pass and they are emitted from there, most of the time is spent in
  the drivers and the sinks (`table/events/ser`, the serialize driver
  alone, is about 1 ms).  Formatting floats with the standard library is
  about 8% of writing.  Empty optional numbers used to build an error
  message for every field that was thrown away (a third of reading).
* **Exact numbers** in JSON (on by default) pass floats with more than 15
  digits or an exponent as number extension values.  Float sinks read
  their values with a single dynamic call, which makes this as fast as
  plain floats (canada).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 440.9 us | 428.0 us | 1.03x | 161.4 us | 275.0 us | 0.59x |
| canada/json              | 3.03 ms  | 2.12 ms  | 1.43x | 1.24 ms  | 1.24 ms  | 1.00x |
| citm-catalog/json        | 962.9 us | 971.9 us | 0.99x | 322.0 us | 290.8 us | 1.11x |
| cargo-manifest/json      | 14.1 us  | 9.7 us   | 1.45x | 3.1 us   | 2.4 us   | 1.29x |
| web-sys-manifest/json    | 206.2 us | 186.2 us | 1.11x | 23.6 us  | 30.9 us  | 0.76x |
| cargo-lock/json          | 44.2 us  | 39.4 us  | 1.12x | 12.7 us  | 20.1 us  | 0.63x |
| saphyr/json              | 402.7 us | 356.0 us | 1.13x | 100.7 us | 308.8 us | 0.33x |
| github/json              | 1.15 ms  | 967.0 us | 1.19x | 371.7 us | 629.5 us | 0.59x |
| kubernetes/json          | 5.82 ms  | 4.35 ms  | 1.34x | 1.73 ms  | 1.67 ms  | 1.04x |
| manifests/json           | 793.9 us | 879.6 us | 0.90x | 262.5 us | 251.7 us | 1.04x |
| logs/json                | 4.13 ms  | 5.54 ms  | 0.75x | 2.05 ms  | 1.47 ms  | 1.39x |
| features/json            | 3.38 ms  | 2.14 ms  | 1.58x | 1.63 ms  | 1.78 ms  | 0.92x |
| point-cloud/json         | 1.95 ms  | 1.23 ms  | 1.60x | 1.07 ms  | 1.33 ms  | 0.80x |
| registry/json            | 1.36 ms  | 1.09 ms  | 1.25x | 243.9 us | 241.1 us | 1.01x |
| tree/json                | 2.33 ms  | 1.39 ms  | 1.68x | 967.7 us | 644.7 us | 1.50x |
| twitter/cbor             | 395.1 us | 505.6 us | 0.78x | 126.7 us | 111.2 us | 1.14x |
| canada/cbor              | 1.53 ms  | 1.51 ms  | 1.02x | 610.9 us | 394.1 us | 1.55x |
| citm-catalog/cbor        | 746.5 us | 624.1 us | 1.20x | 271.2 us | 181.2 us | 1.50x |
| cargo-manifest/cbor      | 13.2 us  | 15.3 us  | 0.86x | 3.0 us   | 1.5 us   | 2.00x |
| web-sys-manifest/cbor    | 205.0 us | 253.3 us | 0.81x | 20.6 us  | 18.9 us  | 1.09x |
| cargo-lock/cbor          | 38.6 us  | 60.4 us  | 0.64x | 11.5 us  | 9.6 us   | 1.20x |
| saphyr/cbor              | 329.2 us | 546.0 us | 0.60x | 93.6 us  | 65.3 us  | 1.43x |
| github/cbor              | 1.06 ms  | 1.43 ms  | 0.74x | 312.1 us | 281.6 us | 1.11x |
| kubernetes/cbor          | 5.31 ms  | 5.79 ms  | 0.92x | 1.56 ms  | 743.7 us | 2.10x |
| manifests/cbor           | 740.2 us | 1.38 ms  | 0.53x | 233.3 us | 137.4 us | 1.70x |
| logs/cbor                | 3.80 ms  | 6.07 ms  | 0.63x | 1.80 ms  | 1.24 ms  | 1.45x |
| features/cbor            | 1.52 ms  | 1.14 ms  | 1.33x | 553.5 us | 327.6 us | 1.69x |
| point-cloud/cbor         | 1.06 ms  | 1.15 ms  | 0.93x | 431.3 us | 397.7 us | 1.08x |
| registry/cbor            | 996.2 us | 1.44 ms  | 0.69x | 241.6 us | 231.2 us | 1.04x |
| tree/cbor                | 2.15 ms  | 1.69 ms  | 1.27x | 802.4 us | 473.1 us | 1.70x |
| twitter/msgpack          | 335.0 us | 318.6 us | 1.05x | 126.0 us | 116.4 us | 1.08x |
| canada/msgpack           | 1.34 ms  | 490.9 us | 2.74x | 571.7 us | 359.1 us | 1.59x |
| citm-catalog/msgpack     | 642.6 us | 298.1 us | 2.16x | 286.9 us | 206.1 us | 1.39x |
| cargo-manifest/msgpack   | 12.3 us  | 7.8 us   | 1.58x | 2.9 us   | 1.8 us   | 1.61x |
| web-sys-manifest/msgpack | 202.0 us | 183.3 us | 1.10x | 21.4 us  | 23.3 us  | 0.92x |
| cargo-lock/msgpack       | 33.3 us  | 31.2 us  | 1.07x | 11.6 us  | 12.6 us  | 0.92x |
| saphyr/msgpack           | 296.0 us | 275.0 us | 1.08x | 88.6 us  | 72.5 us  | 1.22x |
| github/msgpack           | 905.0 us | 914.8 us | 0.99x | 300.9 us | 290.8 us | 1.03x |
| kubernetes/msgpack       | 4.95 ms  | 3.39 ms  | 1.46x | 1.57 ms  | 850.9 us | 1.85x |
| manifests/msgpack        | 667.7 us | 772.1 us | 0.86x | 239.2 us | 153.9 us | 1.55x |
| logs/msgpack             | 3.40 ms  | 3.77 ms  | 0.90x | 1.81 ms  | 1.35 ms  | 1.35x |
| features/msgpack         | 1.25 ms  | 461.3 us | 2.70x | 524.3 us | 345.8 us | 1.52x |
| point-cloud/msgpack      | 875.6 us | 364.6 us | 2.40x | 385.8 us | 252.4 us | 1.53x |
| registry/msgpack         | 940.0 us | 771.0 us | 1.22x | 225.6 us | 221.6 us | 1.02x |
| tree/msgpack             | 1.87 ms  | 720.9 us | 2.59x | 824.0 us | 599.1 us | 1.38x |
| twitter/yaml             | 2.47 ms  | 9.10 ms  | 0.27x | 1.39 ms  | 3.37 ms  | 0.41x |
| canada/yaml              | 14.95 ms | 46.68 ms | 0.32x | 3.42 ms  | 3.42 ms  | 1.00x |
| citm-catalog/yaml        | 5.20 ms  | 18.98 ms | 0.27x | 1.93 ms  | 3.09 ms  | 0.62x |
| cargo-manifest/yaml      | 37.4 us  | 125.3 us | 0.30x | 19.9 us  | 29.3 us  | 0.68x |
| web-sys-manifest/yaml    | 500.3 us | 1.53 ms  | 0.33x | 197.8 us | 440.9 us | 0.45x |
| cargo-lock/yaml          | 192.8 us | 646.8 us | 0.30x | 136.8 us | 319.8 us | 0.43x |
| saphyr/yaml              | 2.35 ms  | 5.37 ms  | 0.44x | 1.37 ms  | 5.92 ms  | 0.23x |
| github/yaml              | 5.36 ms  | 18.65 ms | 0.29x | 3.85 ms  | 11.45 ms | 0.34x |
| kubernetes/yaml          | 18.25 ms | 51.97 ms | 0.35x | 10.53 ms | 20.05 ms | 0.53x |
| manifests/yaml           | 3.00 ms  | 13.13 ms | 0.23x | 1.73 ms  | 3.24 ms  | 0.54x |
| logs/yaml                | 14.99 ms | 53.47 ms | 0.28x | 10.16 ms | 15.63 ms | 0.65x |
| features/yaml            | 15.32 ms | 44.69 ms | 0.34x | 3.85 ms  | 4.00 ms  | 0.96x |
| point-cloud/yaml         | 10.02 ms | 32.62 ms | 0.31x | 2.65 ms  | 2.82 ms  | 0.94x |
| registry/yaml            | 4.24 ms  | 14.04 ms | 0.30x | 1.33 ms  | 2.15 ms  | 0.62x |
| tree/yaml                | 17.28 ms | 59.50 ms | 0.29x | 4.83 ms  | 7.19 ms  | 0.67x |
| twitter/toml             | 817.0 us | 1.75 ms  | 0.47x | 1.15 ms  | 1.06 ms  | 1.09x |
| canada/toml              | 5.17 ms  | 10.54 ms | 0.49x | 3.86 ms  | 3.95 ms  | 0.98x |
| citm-catalog/toml        | 2.04 ms  | 4.42 ms  | 0.46x | 2.25 ms  | 2.65 ms  | 0.85x |
| cargo-manifest/toml      | 16.8 us  | 20.6 us  | 0.82x | 14.4 us  | 13.0 us  | 1.11x |
| web-sys-manifest/toml    | 304.4 us | 469.7 us | 0.65x | 207.4 us | 146.9 us | 1.41x |
| cargo-lock/toml          | 74.7 us  | 150.5 us | 0.50x | 108.3 us | 115.0 us | 0.94x |
| saphyr/toml              | 625.3 us | 1.47 ms  | 0.43x | 1.58 ms  | 1.80 ms  | 0.88x |
| github/toml              | 2.27 ms  | 4.59 ms  | 0.50x | 3.63 ms  | 3.45 ms  | 1.05x |
| kubernetes/toml          | 10.01 ms | 20.39 ms | 0.49x | 12.02 ms | 13.70 ms | 0.88x |
| manifests/toml           | 1.34 ms  | 2.81 ms  | 0.48x | 1.55 ms  | 2.18 ms  | 0.71x |
| logs/toml                | 6.35 ms  | 12.09 ms | 0.53x | 7.51 ms  | 6.78 ms  | 1.11x |
| features/toml            | 6.11 ms  | 12.98 ms | 0.47x | 4.04 ms  | 5.21 ms  | 0.78x |
| point-cloud/toml         | 3.88 ms  | 8.39 ms  | 0.46x | 2.75 ms  | 3.22 ms  | 0.85x |
| registry/toml            | 2.01 ms  | 3.66 ms  | 0.55x | 1.39 ms  | 1.50 ms  | 0.92x |
| tree/toml                | 6.44 ms  | 15.03 ms | 0.43x | 6.14 ms  | 7.88 ms  | 0.78x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 746/515 us in JSON, 512/181 us in CBOR, 449/180 us in
MessagePack, 4.48/1.24 ms in YAML and 1.15/2.04 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 287 us, 1.12x of
serde_json.

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
a document costs (see `src/logs.rs`).  `table` is a table of 20,000 rows
which only exists for CSV (see `src/table.rs`).

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
