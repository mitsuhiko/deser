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
| JSON        | 1.11x | 0.70x-1.65x | 0.94x | 0.37x-1.69x |
| CBOR        | 0.79x | 0.53x-1.33x | 1.35x | 0.98x-1.90x |
| MessagePack | 1.41x | 0.89x-2.77x | 1.25x | 0.90x-1.73x |
| YAML        | 0.31x | 0.24x-0.44x | 0.55x | 0.25x-1.01x |
| TOML        | 0.50x | 0.42x-0.80x | 0.92x | 0.70x-1.34x |

* **YAML and TOML** deserialize two to four times as fast as serde-saphyr
  and toml.  Serializing is faster or on par, except for the large table
  of web-sys-manifest in TOML (1.34x).
* **JSON** serializes faster than serde_json or on par, except for tree
  (1.69x), cargo-manifest (1.32x), logs (1.32x), citm-catalog (1.29x)
  and manifests (1.24x).  Deserializing is faster for citm-catalog,
  manifests and logs, on par for github, 1.03x-1.13x slower for other
  string heavy data (saphyr, twitter, cargo-lock, web-sys-manifest) and
  1.15x-1.34x slower for floats and nesting (canada, registry,
  kubernetes, point-cloud, features), 1.65x for tree.
* **CBOR** deserializes faster than ciborium except for citm-catalog
  (1.25x) and tree (1.33x), serializing is 1.35x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 1.9x-2.8x slower for floats, nesting and integer keyed
  maps (canada, point-cloud, features, citm-catalog, tree).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.25x slower.
* **Untagged enums** (cargo-manifest) are 1.33x slower in JSON and 1.58x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 8.90 ms  | 12.35 ms | 0.72x | 6.99 ms | 9.70 ms | 0.72x |
| session-anthropic/json | 25.5 MiB | 3.03 ms  | 4.34 ms  | 0.70x | 3.05 ms | 8.14 ms | 0.37x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 417.5 us | 158.9 us |
| serde_json | 396.2 us | 217.0 us |
| miniserde  | 432.1 us | 312.4 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 412.6 us | 393.5 us | 1.05x | 163.6 us | 218.4 us | 0.75x |
| canada/json              | 2.42 ms  | 2.11 ms  | 1.15x | 1.26 ms  | 1.25 ms  | 1.01x |
| citm-catalog/json        | 901.0 us | 954.6 us | 0.94x | 320.5 us | 248.7 us | 1.29x |
| cargo-manifest/json      | 12.8 us  | 9.6 us   | 1.33x | 2.9 us   | 2.2 us   | 1.32x |
| web-sys-manifest/json    | 200.8 us | 177.1 us | 1.13x | 23.5 us  | 27.6 us  | 0.85x |
| cargo-lock/json          | 40.4 us  | 37.3 us  | 1.08x | 13.4 us  | 19.5 us  | 0.69x |
| saphyr/json              | 360.9 us | 351.7 us | 1.03x | 101.9 us | 274.0 us | 0.37x |
| github/json              | 1.02 ms  | 1.03 ms  | 0.99x | 392.6 us | 594.9 us | 0.66x |
| kubernetes/json          | 5.43 ms  | 4.35 ms  | 1.25x | 1.62 ms  | 1.68 ms  | 0.97x |
| manifests/json           | 713.4 us | 865.8 us | 0.82x | 254.5 us | 206.0 us | 1.24x |
| logs/json                | 3.77 ms  | 5.36 ms  | 0.70x | 1.89 ms  | 1.43 ms  | 1.32x |
| features/json            | 2.86 ms  | 2.13 ms  | 1.34x | 1.64 ms  | 1.78 ms  | 0.92x |
| point-cloud/json         | 1.59 ms  | 1.22 ms  | 1.31x | 1.07 ms  | 1.31 ms  | 0.82x |
| registry/json            | 1.31 ms  | 1.08 ms  | 1.21x | 249.2 us | 234.3 us | 1.06x |
| tree/json                | 2.22 ms  | 1.34 ms  | 1.65x | 940.6 us | 557.5 us | 1.69x |
| twitter/cbor             | 396.6 us | 508.1 us | 0.78x | 129.4 us | 113.1 us | 1.14x |
| canada/cbor              | 1.15 ms  | 1.50 ms  | 0.76x | 614.8 us | 425.2 us | 1.45x |
| citm-catalog/cbor        | 769.0 us | 616.6 us | 1.25x | 271.4 us | 180.2 us | 1.51x |
| cargo-manifest/cbor      | 12.7 us  | 15.3 us  | 0.83x | 2.6 us   | 1.5 us   | 1.73x |
| web-sys-manifest/cbor    | 201.9 us | 251.9 us | 0.80x | 19.9 us  | 19.5 us  | 1.02x |
| cargo-lock/cbor          | 39.3 us  | 60.4 us  | 0.65x | 11.6 us  | 9.6 us   | 1.21x |
| saphyr/cbor              | 326.0 us | 542.0 us | 0.60x | 94.9 us  | 66.0 us  | 1.44x |
| github/cbor              | 1.02 ms  | 1.43 ms  | 0.71x | 327.7 us | 300.8 us | 1.09x |
| kubernetes/cbor          | 5.14 ms  | 5.69 ms  | 0.90x | 1.45 ms  | 764.9 us | 1.90x |
| manifests/cbor           | 730.4 us | 1.37 ms  | 0.53x | 231.7 us | 123.0 us | 1.88x |
| logs/cbor                | 3.71 ms  | 6.12 ms  | 0.61x | 1.68 ms  | 1.21 ms  | 1.38x |
| features/cbor            | 1.13 ms  | 1.13 ms  | 1.00x | 566.5 us | 444.1 us | 1.28x |
| point-cloud/cbor         | 848.8 us | 1.13 ms  | 0.75x | 443.9 us | 416.5 us | 1.07x |
| registry/cbor            | 992.1 us | 1.44 ms  | 0.69x | 232.8 us | 237.4 us | 0.98x |
| tree/cbor                | 2.24 ms  | 1.68 ms  | 1.33x | 795.6 us | 471.5 us | 1.69x |
| twitter/msgpack          | 352.9 us | 313.2 us | 1.13x | 124.7 us | 116.8 us | 1.07x |
| canada/msgpack           | 904.9 us | 485.5 us | 1.86x | 569.1 us | 397.7 us | 1.43x |
| citm-catalog/msgpack     | 680.3 us | 290.7 us | 2.34x | 283.9 us | 214.4 us | 1.32x |
| cargo-manifest/msgpack   | 12.0 us  | 7.6 us   | 1.58x | 2.6 us   | 1.7 us   | 1.53x |
| web-sys-manifest/msgpack | 194.7 us | 169.2 us | 1.15x | 20.7 us  | 21.8 us  | 0.95x |
| cargo-lock/msgpack       | 34.9 us  | 30.0 us  | 1.16x | 11.2 us  | 12.4 us  | 0.90x |
| saphyr/msgpack           | 288.7 us | 265.2 us | 1.09x | 86.5 us  | 71.1 us  | 1.22x |
| github/msgpack           | 904.0 us | 910.2 us | 0.99x | 315.2 us | 304.4 us | 1.04x |
| kubernetes/msgpack       | 4.85 ms  | 3.32 ms  | 1.46x | 1.46 ms  | 841.6 us | 1.73x |
| manifests/msgpack        | 671.6 us | 757.8 us | 0.89x | 225.9 us | 148.3 us | 1.52x |
| logs/msgpack             | 3.39 ms  | 3.74 ms  | 0.91x | 1.64 ms  | 1.30 ms  | 1.27x |
| features/msgpack         | 901.3 us | 451.0 us | 2.00x | 531.1 us | 403.7 us | 1.32x |
| point-cloud/msgpack      | 676.6 us | 362.2 us | 1.87x | 401.7 us | 293.6 us | 1.37x |
| registry/msgpack         | 945.1 us | 772.5 us | 1.22x | 219.9 us | 218.9 us | 1.00x |
| tree/msgpack             | 1.98 ms  | 713.4 us | 2.77x | 803.5 us | 565.1 us | 1.42x |
| twitter/yaml             | 2.51 ms  | 8.97 ms  | 0.28x | 1.37 ms  | 3.42 ms  | 0.40x |
| canada/yaml              | 15.17 ms | 45.86 ms | 0.33x | 3.45 ms  | 3.42 ms  | 1.01x |
| citm-catalog/yaml        | 5.28 ms  | 18.62 ms | 0.28x | 1.92 ms  | 3.21 ms  | 0.60x |
| cargo-manifest/yaml      | 37.5 us  | 124.8 us | 0.30x | 19.7 us  | 30.3 us  | 0.65x |
| web-sys-manifest/yaml    | 495.3 us | 1.51 ms  | 0.33x | 196.0 us | 451.3 us | 0.43x |
| cargo-lock/yaml          | 196.0 us | 639.7 us | 0.31x | 131.0 us | 320.3 us | 0.41x |
| saphyr/yaml              | 2.33 ms  | 5.27 ms  | 0.44x | 1.36 ms  | 5.50 ms  | 0.25x |
| github/yaml              | 5.59 ms  | 18.24 ms | 0.31x | 3.80 ms  | 11.47 ms | 0.33x |
| kubernetes/yaml          | 17.89 ms | 50.74 ms | 0.35x | 10.26 ms | 20.19 ms | 0.51x |
| manifests/yaml           | 3.05 ms  | 12.95 ms | 0.24x | 1.73 ms  | 3.28 ms  | 0.53x |
| logs/yaml                | 14.77 ms | 53.05 ms | 0.28x | 9.82 ms  | 15.71 ms | 0.63x |
| features/yaml            | 15.68 ms | 44.18 ms | 0.36x | 3.87 ms  | 3.97 ms  | 0.97x |
| point-cloud/yaml         | 10.48 ms | 32.03 ms | 0.33x | 2.67 ms  | 2.80 ms  | 0.95x |
| registry/yaml            | 4.37 ms  | 13.86 ms | 0.32x | 1.31 ms  | 2.23 ms  | 0.59x |
| tree/yaml                | 17.55 ms | 58.68 ms | 0.30x | 4.70 ms  | 7.21 ms  | 0.65x |
| twitter/toml             | 833.8 us | 1.77 ms  | 0.47x | 1.12 ms  | 1.06 ms  | 1.06x |
| canada/toml              | 4.71 ms  | 10.31 ms | 0.46x | 3.82 ms  | 3.90 ms  | 0.98x |
| citm-catalog/toml        | 2.05 ms  | 4.18 ms  | 0.49x | 2.19 ms  | 2.66 ms  | 0.82x |
| cargo-manifest/toml      | 16.5 us  | 20.6 us  | 0.80x | 13.8 us  | 13.1 us  | 1.05x |
| web-sys-manifest/toml    | 305.4 us | 461.2 us | 0.66x | 197.8 us | 148.0 us | 1.34x |
| cargo-lock/toml          | 77.9 us  | 143.2 us | 0.54x | 108.8 us | 115.9 us | 0.94x |
| saphyr/toml              | 631.6 us | 1.46 ms  | 0.43x | 1.56 ms  | 1.74 ms  | 0.89x |
| github/toml              | 2.28 ms  | 4.43 ms  | 0.51x | 3.50 ms  | 3.43 ms  | 1.02x |
| kubernetes/toml          | 9.97 ms  | 19.96 ms | 0.50x | 11.64 ms | 13.81 ms | 0.84x |
| manifests/toml           | 1.35 ms  | 3.23 ms  | 0.42x | 1.48 ms  | 2.12 ms  | 0.70x |
| logs/toml                | 6.31 ms  | 11.89 ms | 0.53x | 6.99 ms  | 6.65 ms  | 1.05x |
| features/toml            | 5.67 ms  | 12.46 ms | 0.45x | 4.06 ms  | 5.21 ms  | 0.78x |
| point-cloud/toml         | 3.53 ms  | 8.46 ms  | 0.42x | 2.75 ms  | 3.27 ms  | 0.84x |
| registry/toml            | 1.98 ms  | 3.62 ms  | 0.55x | 1.36 ms  | 1.52 ms  | 0.90x |
| tree/toml                | 6.43 ms  | 14.34 ms | 0.45x | 5.98 ms  | 7.95 ms  | 0.75x |
| table/csv                | 6.20 ms  | 3.80 ms  | 1.63x | 5.24 ms  | 1.72 ms  | 3.06x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 693/510 us in JSON, 511/179 us in CBOR, 456/175 us in
MessagePack, 4.55/1.23 ms in YAML and 1.18/2.18 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 240 us, 0.95x of
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
