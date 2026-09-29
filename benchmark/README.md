# Runtime Performance

This folder compares every format of deser with a serde based library on
the same data and types: `deser-json` with `serde_json`, `deser-cbor` with
`ciborium`, `deser-msgpack` with `rmp-serde`, `deser-yaml` with
`serde-saphyr`, `deser-toml` with `toml` and `deser-csv` with `csv`.  The results below are from
`cargo run --release -- time "" 10` (`make bench-versus` with 10 rounds)
on an Apple M5 Max with Rust 1.98.  Ratios are deser/serde, below 1 deser
is faster.

## Where deser Stands

The geometric mean over the 15 datasets, with the best and the worst one:

| format      | de    | de range    | ser   | ser range   |
|-------------|-------|-------------|-------|-------------|
| JSON        | 1.11x | 0.69x-1.65x | 0.94x | 0.35x-1.71x |
| CBOR        | 0.79x | 0.53x-1.30x | 1.35x | 1.00x-1.88x |
| MessagePack | 1.41x | 0.88x-2.77x | 1.26x | 0.93x-1.71x |
| YAML        | 0.31x | 0.24x-0.45x | 0.54x | 0.25x-0.99x |
| TOML        | 0.51x | 0.44x-0.81x | 0.92x | 0.69x-1.40x |

* **YAML and TOML** deserialize two to four times as fast as serde-saphyr
  and toml.  Serializing is faster or on par, except for the large table
  of web-sys-manifest in TOML (1.40x).
* **JSON** serializes faster than serde_json or on par, except for tree
  (1.71x), cargo-manifest (1.32x), logs (1.30x), citm-catalog (1.27x),
  manifests (1.22x) and registry (1.12x).  Deserializing is faster for
  citm-catalog, manifests and logs, on par for github, 1.05x-1.16x
  slower for other string heavy data (saphyr, twitter, cargo-lock,
  web-sys-manifest) and 1.15x-1.35x slower for floats and nesting
  (canada, registry, kubernetes, point-cloud, features), 1.65x for tree.
* **CBOR** deserializes faster than ciborium except for citm-catalog
  (1.24x) and tree (1.30x), serializing is 1.35x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 1.9x-2.8x slower for floats, nesting and integer keyed
  maps (canada, point-cloud, features, citm-catalog, tree).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.26x slower.
* **Untagged enums** (cargo-manifest) are 1.34x slower in JSON and 1.60x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 8.87 ms  | 12.26 ms | 0.72x | 6.95 ms | 9.50 ms | 0.73x |
| session-anthropic/json | 25.5 MiB | 3.08 ms  | 4.34 ms  | 0.71x | 2.88 ms | 8.09 ms | 0.36x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 435.2 us | 161.7 us |
| serde_json | 402.9 us | 220.6 us |
| miniserde  | 444.5 us | 313.6 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 417.9 us | 389.7 us | 1.07x | 162.5 us | 217.5 us | 0.75x |
| canada/json              | 2.43 ms  | 2.10 ms  | 1.15x | 1.24 ms  | 1.25 ms  | 1.00x |
| citm-catalog/json        | 894.8 us | 955.7 us | 0.94x | 316.8 us | 248.6 us | 1.27x |
| cargo-manifest/json      | 12.9 us  | 9.6 us   | 1.34x | 2.9 us   | 2.2 us   | 1.32x |
| web-sys-manifest/json    | 204.5 us | 176.7 us | 1.16x | 24.0 us  | 27.9 us  | 0.86x |
| cargo-lock/json          | 39.9 us  | 37.4 us  | 1.07x | 13.5 us  | 19.9 us  | 0.68x |
| saphyr/json              | 364.1 us | 347.3 us | 1.05x | 108.1 us | 311.7 us | 0.35x |
| github/json              | 1.05 ms  | 1.03 ms  | 1.02x | 390.4 us | 605.8 us | 0.64x |
| kubernetes/json          | 5.45 ms  | 4.34 ms  | 1.26x | 1.60 ms  | 1.65 ms  | 0.97x |
| manifests/json           | 720.1 us | 867.8 us | 0.83x | 250.4 us | 204.8 us | 1.22x |
| logs/json                | 3.69 ms  | 5.32 ms  | 0.69x | 1.86 ms  | 1.42 ms  | 1.30x |
| features/json            | 2.86 ms  | 2.12 ms  | 1.35x | 1.62 ms  | 1.75 ms  | 0.93x |
| point-cloud/json         | 1.52 ms  | 1.21 ms  | 1.26x | 1.08 ms  | 1.31 ms  | 0.82x |
| registry/json            | 1.30 ms  | 1.08 ms  | 1.19x | 246.4 us | 219.9 us | 1.12x |
| tree/json                | 2.20 ms  | 1.34 ms  | 1.65x | 951.2 us | 557.5 us | 1.71x |
| twitter/cbor             | 400.7 us | 509.0 us | 0.79x | 126.5 us | 112.0 us | 1.13x |
| canada/cbor              | 1.13 ms  | 1.51 ms  | 0.75x | 613.0 us | 391.7 us | 1.56x |
| citm-catalog/cbor        | 772.6 us | 623.7 us | 1.24x | 264.4 us | 175.2 us | 1.51x |
| cargo-manifest/cbor      | 12.8 us  | 15.3 us  | 0.84x | 2.6 us   | 1.5 us   | 1.73x |
| web-sys-manifest/cbor    | 199.9 us | 246.3 us | 0.81x | 20.1 us  | 19.3 us  | 1.04x |
| cargo-lock/cbor          | 40.0 us  | 60.6 us  | 0.66x | 11.5 us  | 9.7 us   | 1.19x |
| saphyr/cbor              | 328.0 us | 545.8 us | 0.60x | 100.3 us | 73.3 us  | 1.37x |
| github/cbor              | 1.04 ms  | 1.45 ms  | 0.72x | 314.2 us | 292.2 us | 1.08x |
| kubernetes/cbor          | 5.17 ms  | 5.67 ms  | 0.91x | 1.42 ms  | 756.5 us | 1.88x |
| manifests/cbor           | 726.2 us | 1.37 ms  | 0.53x | 224.4 us | 120.2 us | 1.87x |
| logs/cbor                | 3.69 ms  | 6.03 ms  | 0.61x | 1.64 ms  | 1.21 ms  | 1.36x |
| features/cbor            | 1.12 ms  | 1.13 ms  | 0.99x | 568.1 us | 471.6 us | 1.20x |
| point-cloud/cbor         | 845.4 us | 1.15 ms  | 0.74x | 444.8 us | 397.5 us | 1.12x |
| registry/cbor            | 981.8 us | 1.44 ms  | 0.68x | 236.7 us | 236.2 us | 1.00x |
| tree/cbor                | 2.24 ms  | 1.72 ms  | 1.30x | 792.5 us | 476.3 us | 1.66x |
| twitter/msgpack          | 352.8 us | 313.8 us | 1.12x | 122.5 us | 113.7 us | 1.08x |
| canada/msgpack           | 910.2 us | 487.5 us | 1.87x | 571.6 us | 397.5 us | 1.44x |
| citm-catalog/msgpack     | 675.6 us | 295.1 us | 2.29x | 281.0 us | 208.4 us | 1.35x |
| cargo-manifest/msgpack   | 12.0 us  | 7.5 us   | 1.60x | 2.6 us   | 1.7 us   | 1.53x |
| web-sys-manifest/msgpack | 197.2 us | 175.2 us | 1.13x | 21.3 us  | 22.9 us  | 0.93x |
| cargo-lock/msgpack       | 34.7 us  | 29.9 us  | 1.16x | 11.4 us  | 12.0 us  | 0.95x |
| saphyr/msgpack           | 285.7 us | 260.9 us | 1.10x | 93.5 us  | 76.3 us  | 1.23x |
| github/msgpack           | 922.5 us | 904.5 us | 1.02x | 300.5 us | 289.0 us | 1.04x |
| kubernetes/msgpack       | 4.89 ms  | 3.28 ms  | 1.49x | 1.43 ms  | 833.3 us | 1.71x |
| manifests/msgpack        | 663.6 us | 757.6 us | 0.88x | 220.3 us | 147.9 us | 1.49x |
| logs/msgpack             | 3.37 ms  | 3.72 ms  | 0.90x | 1.62 ms  | 1.28 ms  | 1.26x |
| features/msgpack         | 907.3 us | 453.6 us | 2.00x | 536.4 us | 407.5 us | 1.32x |
| point-cloud/msgpack      | 678.5 us | 361.7 us | 1.88x | 403.3 us | 289.9 us | 1.39x |
| registry/msgpack         | 939.6 us | 764.4 us | 1.23x | 220.0 us | 213.3 us | 1.03x |
| tree/msgpack             | 1.97 ms  | 709.2 us | 2.77x | 813.5 us | 567.8 us | 1.43x |
| twitter/yaml             | 2.56 ms  | 8.96 ms  | 0.29x | 1.36 ms  | 3.43 ms  | 0.40x |
| canada/yaml              | 15.22 ms | 46.07 ms | 0.33x | 3.43 ms  | 3.45 ms  | 0.99x |
| citm-catalog/yaml        | 5.31 ms  | 18.67 ms | 0.28x | 1.87 ms  | 3.23 ms  | 0.58x |
| cargo-manifest/yaml      | 37.5 us  | 124.3 us | 0.30x | 19.4 us  | 30.2 us  | 0.64x |
| web-sys-manifest/yaml    | 503.8 us | 1.51 ms  | 0.33x | 196.1 us | 454.3 us | 0.43x |
| cargo-lock/yaml          | 196.7 us | 640.2 us | 0.31x | 131.3 us | 323.5 us | 0.41x |
| saphyr/yaml              | 2.33 ms  | 5.24 ms  | 0.45x | 1.36 ms  | 5.44 ms  | 0.25x |
| github/yaml              | 5.63 ms  | 18.26 ms | 0.31x | 3.73 ms  | 11.52 ms | 0.32x |
| kubernetes/yaml          | 18.04 ms | 50.80 ms | 0.36x | 10.19 ms | 19.95 ms | 0.51x |
| manifests/yaml           | 3.05 ms  | 12.85 ms | 0.24x | 1.71 ms  | 3.31 ms  | 0.52x |
| logs/yaml                | 14.80 ms | 53.08 ms | 0.28x | 9.78 ms  | 15.86 ms | 0.62x |
| features/yaml            | 15.59 ms | 44.17 ms | 0.35x | 3.82 ms  | 3.99 ms  | 0.96x |
| point-cloud/yaml         | 10.41 ms | 32.09 ms | 0.32x | 2.65 ms  | 2.79 ms  | 0.95x |
| registry/yaml            | 4.39 ms  | 13.91 ms | 0.32x | 1.24 ms  | 2.23 ms  | 0.55x |
| tree/yaml                | 17.61 ms | 58.75 ms | 0.30x | 4.52 ms  | 7.56 ms  | 0.60x |
| twitter/toml             | 837.8 us | 1.74 ms  | 0.48x | 1.11 ms  | 1.05 ms  | 1.06x |
| canada/toml              | 4.73 ms  | 10.50 ms | 0.45x | 3.82 ms  | 3.94 ms  | 0.97x |
| citm-catalog/toml        | 2.04 ms  | 4.50 ms  | 0.45x | 2.16 ms  | 2.64 ms  | 0.82x |
| cargo-manifest/toml      | 16.6 us  | 20.4 us  | 0.81x | 13.8 us  | 13.0 us  | 1.06x |
| web-sys-manifest/toml    | 305.4 us | 454.8 us | 0.67x | 202.6 us | 144.3 us | 1.40x |
| cargo-lock/toml          | 78.9 us  | 142.3 us | 0.55x | 109.2 us | 117.7 us | 0.93x |
| saphyr/toml              | 635.5 us | 1.43 ms  | 0.44x | 1.56 ms  | 1.77 ms  | 0.88x |
| github/toml              | 2.31 ms  | 4.44 ms  | 0.52x | 3.51 ms  | 3.41 ms  | 1.03x |
| kubernetes/toml          | 9.96 ms  | 20.08 ms | 0.50x | 11.44 ms | 13.42 ms | 0.85x |
| manifests/toml           | 1.33 ms  | 2.78 ms  | 0.48x | 1.46 ms  | 2.12 ms  | 0.69x |
| logs/toml                | 6.23 ms  | 11.85 ms | 0.53x | 7.10 ms  | 6.73 ms  | 1.06x |
| features/toml            | 5.63 ms  | 12.41 ms | 0.45x | 3.98 ms  | 5.26 ms  | 0.76x |
| point-cloud/toml         | 3.52 ms  | 8.02 ms  | 0.44x | 2.73 ms  | 3.29 ms  | 0.83x |
| registry/toml            | 2.00 ms  | 3.60 ms  | 0.55x | 1.33 ms  | 1.49 ms  | 0.89x |
| tree/toml                | 6.32 ms  | 14.19 ms | 0.45x | 5.95 ms  | 7.72 ms  | 0.77x |
| table/csv                | 6.24 ms  | 3.80 ms  | 1.64x | 5.23 ms  | 1.75 ms  | 2.98x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 680/514 us in JSON, 511/176 us in CBOR, 456/173 us in
MessagePack, 4.59/1.23 ms in YAML and 1.18/2.08 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 244 us, 0.96x of
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

`session-openai` and `session-anthropic` are two long sessions of the pi
coding agent (`data/pi-sessions`), one with an OpenAI model (18.9 MiB in
2,823 lines, 23 screenshots) and one with an Anthropic model (25.5 MiB in
1,028 lines, 29 screenshots).  They were stripped of all personal data
with `scripts/scrub-pi-session.mjs`: text is replaced by random words of
the same length and case, ids by random ids and images by generated PNGs
of about the same size, the structure, keys, numbers and escapes are
kept.  Every line is an entry, fully typed after the TypeScript types of
pi (see `src/sessions.rs`): internally tagged enums on three levels (entry,
message, content), untagged enums for the arguments and details of tools,
recursive JSON schemas and many optional fields.  When the data is loaded,
every entry is serialized again and compared with its line, which makes
sure that the types do not drop anything.

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
* `cargo run --release --example msgpack` isolates primitive and small-container
  deserialization overhead; see [PERF_NOTES.md](PERF_NOTES.md).
* `make bench` runs `cargo bench` (Twitter JSON with miniserde).
