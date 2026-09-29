# Runtime Performance

This folder compares every format of deser with a serde based library on
the same data and types: `deser-json` with `serde_json`, `deser-cbor` with
`ciborium`, `deser-msgpack` with `rmp-serde`, `deser-yaml` with
`serde-saphyr`, `deser-toml` with `toml` and `deser-csv` with `csv`.  The results below are from
`cargo run --release -- time "" 10` (`make bench-versus` with 10 rounds)
on an Apple M5 Max with Rust 1.98.  Ratios are deser/serde, below 1 deser
is faster.  The benchmark is built with one codegen unit: it holds the
derived code of both libraries, and with more units a change to the
derived code of one library moved the code of the other into other units
(serde_json serialized up to 25% slower after the derived code of deser
got smaller).

## Where deser Stands

The geometric mean over the 15 datasets, with the best and the worst one:

| format      | de    | de range    | ser   | ser range   |
|-------------|-------|-------------|-------|-------------|
| JSON        | 1.10x | 0.67x-1.61x | 0.95x | 0.36x-1.72x |
| CBOR        | 0.79x | 0.53x-1.29x | 1.33x | 0.99x-1.84x |
| MessagePack | 1.40x | 0.86x-2.77x | 1.33x | 0.98x-1.78x |
| YAML        | 0.28x | 0.21x-0.40x | 0.53x | 0.25x-1.00x |
| TOML        | 0.49x | 0.39x-0.78x | 0.98x | 0.74x-1.45x |

* **YAML and TOML** deserialize 2.5 to 5 times as fast as serde-saphyr
  (YAML) and 1.3 to 2.6 times as fast as toml (TOML).  Serializing is faster or on par (TOML up to 1.12x slower),
  except for the large table of web-sys-manifest in TOML (1.45x).
* **JSON** serializes faster than serde_json or on par, except for tree
  (1.72x), citm-catalog (1.39x), logs (1.37x), cargo-manifest (1.33x)
  and manifests (1.27x).  Deserializing is faster for citm-catalog,
  manifests and logs, on par for saphyr, 1.05x-1.14x slower for other
  string heavy data (cargo-lock, github, twitter, web-sys-manifest) and
  1.13x-1.35x slower for floats and nesting (canada, registry,
  point-cloud, kubernetes, features), 1.61x for tree.
* **CBOR** deserializes faster than ciborium except for tree (1.17x) and
  citm-catalog (1.29x), serializing is 1.33x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 1.8x-2.8x slower for floats, nesting and integer keyed
  maps (canada, point-cloud, features, citm-catalog, tree).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.33x slower.
* **Untagged enums** (cargo-manifest) are 1.30x slower in JSON and 1.56x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 8.86 ms  | 12.14 ms | 0.73x | 6.93 ms | 9.50 ms | 0.73x |
| session-anthropic/json | 25.5 MiB | 2.98 ms  | 4.23 ms  | 0.70x | 2.98 ms | 8.12 ms | 0.37x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 431.3 us | 159.7 us |
| serde_json | 389.4 us | 212.7 us |
| miniserde  | 440.4 us | 314.8 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 424.8 us | 391.8 us | 1.08x | 165.4 us | 212.7 us | 0.78x |
| canada/json              | 2.40 ms  | 2.12 ms  | 1.13x | 1.23 ms  | 1.22 ms  | 1.00x |
| citm-catalog/json        | 883.8 us | 958.3 us | 0.92x | 320.2 us | 230.4 us | 1.39x |
| cargo-manifest/json      | 12.1 us  | 9.3 us   | 1.30x | 2.8 us   | 2.1 us   | 1.33x |
| web-sys-manifest/json    | 203.7 us | 178.5 us | 1.14x | 23.3 us  | 27.3 us  | 0.85x |
| cargo-lock/json          | 38.9 us  | 37.2 us  | 1.05x | 13.5 us  | 19.6 us  | 0.69x |
| saphyr/json              | 356.0 us | 355.3 us | 1.00x | 98.6 us  | 274.2 us | 0.36x |
| github/json              | 1.02 ms  | 943.8 us | 1.08x | 361.2 us | 571.5 us | 0.63x |
| kubernetes/json          | 5.35 ms  | 4.15 ms  | 1.29x | 1.58 ms  | 1.57 ms  | 1.00x |
| manifests/json           | 700.5 us | 812.8 us | 0.86x | 250.2 us | 196.6 us | 1.27x |
| logs/json                | 3.52 ms  | 5.28 ms  | 0.67x | 1.88 ms  | 1.37 ms  | 1.37x |
| features/json            | 2.83 ms  | 2.09 ms  | 1.35x | 1.61 ms  | 1.76 ms  | 0.92x |
| point-cloud/json         | 1.46 ms  | 1.17 ms  | 1.25x | 1.06 ms  | 1.24 ms  | 0.86x |
| registry/json            | 1.28 ms  | 1.11 ms  | 1.15x | 239.8 us | 238.7 us | 1.00x |
| tree/json                | 2.19 ms  | 1.36 ms  | 1.61x | 927.0 us | 538.6 us | 1.72x |
| twitter/cbor             | 410.4 us | 503.9 us | 0.81x | 126.8 us | 124.6 us | 1.02x |
| canada/cbor              | 1.09 ms  | 1.43 ms  | 0.77x | 607.4 us | 438.5 us | 1.39x |
| citm-catalog/cbor        | 765.6 us | 595.2 us | 1.29x | 270.4 us | 147.6 us | 1.83x |
| cargo-manifest/cbor      | 12.1 us  | 15.0 us  | 0.81x | 2.6 us   | 1.7 us   | 1.53x |
| web-sys-manifest/cbor    | 199.8 us | 236.0 us | 0.85x | 20.5 us  | 19.4 us  | 1.06x |
| cargo-lock/cbor          | 39.0 us  | 59.9 us  | 0.65x | 11.7 us  | 11.4 us  | 1.03x |
| saphyr/cbor              | 324.4 us | 476.1 us | 0.68x | 96.1 us  | 70.1 us  | 1.37x |
| github/cbor              | 1.09 ms  | 1.38 ms  | 0.79x | 311.0 us | 285.7 us | 1.09x |
| kubernetes/cbor          | 5.08 ms  | 5.66 ms  | 0.90x | 1.43 ms  | 776.7 us | 1.84x |
| manifests/cbor           | 730.5 us | 1.37 ms  | 0.53x | 223.4 us | 133.1 us | 1.68x |
| logs/cbor                | 3.50 ms  | 5.90 ms  | 0.59x | 1.71 ms  | 1.27 ms  | 1.35x |
| features/cbor            | 1.04 ms  | 1.06 ms  | 0.98x | 564.9 us | 357.6 us | 1.58x |
| point-cloud/cbor         | 764.2 us | 1.09 ms  | 0.70x | 437.7 us | 410.5 us | 1.07x |
| registry/cbor            | 978.4 us | 1.42 ms  | 0.69x | 235.3 us | 237.9 us | 0.99x |
| tree/cbor                | 2.20 ms  | 1.87 ms  | 1.17x | 793.3 us | 493.3 us | 1.61x |
| twitter/msgpack          | 357.7 us | 308.1 us | 1.16x | 126.2 us | 112.8 us | 1.12x |
| canada/msgpack           | 889.6 us | 487.6 us | 1.82x | 567.6 us | 388.0 us | 1.46x |
| citm-catalog/msgpack     | 671.3 us | 269.9 us | 2.49x | 283.3 us | 190.0 us | 1.49x |
| cargo-manifest/msgpack   | 11.4 us  | 7.3 us   | 1.56x | 2.6 us   | 1.6 us   | 1.62x |
| web-sys-manifest/msgpack | 188.7 us | 166.0 us | 1.14x | 21.2 us  | 21.6 us  | 0.98x |
| cargo-lock/msgpack       | 34.2 us  | 29.4 us  | 1.16x | 11.5 us  | 11.6 us  | 0.99x |
| saphyr/msgpack           | 287.5 us | 266.3 us | 1.08x | 88.8 us  | 66.3 us  | 1.34x |
| github/msgpack           | 939.1 us | 896.5 us | 1.05x | 294.9 us | 285.3 us | 1.03x |
| kubernetes/msgpack       | 4.73 ms  | 3.21 ms  | 1.47x | 1.43 ms  | 806.0 us | 1.78x |
| manifests/msgpack        | 657.0 us | 760.3 us | 0.86x | 221.4 us | 139.0 us | 1.59x |
| logs/msgpack             | 3.24 ms  | 3.67 ms  | 0.88x | 1.63 ms  | 1.29 ms  | 1.26x |
| features/msgpack         | 866.9 us | 451.8 us | 1.92x | 518.3 us | 329.0 us | 1.58x |
| point-cloud/msgpack      | 651.7 us | 361.1 us | 1.80x | 392.7 us | 273.7 us | 1.43x |
| registry/msgpack         | 925.1 us | 774.2 us | 1.19x | 217.3 us | 219.1 us | 0.99x |
| tree/msgpack             | 1.97 ms  | 711.4 us | 2.77x | 802.1 us | 483.2 us | 1.66x |
| twitter/yaml             | 2.52 ms  | 9.78 ms  | 0.26x | 1.39 ms  | 3.58 ms  | 0.39x |
| canada/yaml              | 15.20 ms | 52.58 ms | 0.29x | 3.34 ms  | 3.33 ms  | 1.00x |
| citm-catalog/yaml        | 5.23 ms  | 20.70 ms | 0.25x | 1.85 ms  | 3.36 ms  | 0.55x |
| cargo-manifest/yaml      | 36.6 us  | 136.6 us | 0.27x | 19.5 us  | 31.9 us  | 0.61x |
| web-sys-manifest/yaml    | 495.8 us | 1.66 ms  | 0.30x | 201.3 us | 492.1 us | 0.41x |
| cargo-lock/yaml          | 193.1 us | 710.8 us | 0.27x | 133.3 us | 343.8 us | 0.39x |
| saphyr/yaml              | 2.28 ms  | 5.76 ms  | 0.40x | 1.35 ms  | 5.34 ms  | 0.25x |
| github/yaml              | 5.25 ms  | 19.86 ms | 0.26x | 3.81 ms  | 11.96 ms | 0.32x |
| kubernetes/yaml          | 18.04 ms | 58.65 ms | 0.31x | 10.50 ms | 21.44 ms | 0.49x |
| manifests/yaml           | 2.99 ms  | 13.97 ms | 0.21x | 1.71 ms  | 3.59 ms  | 0.48x |
| logs/yaml                | 14.66 ms | 58.38 ms | 0.25x | 9.80 ms  | 17.61 ms | 0.56x |
| features/yaml            | 15.50 ms | 50.00 ms | 0.31x | 3.75 ms  | 3.91 ms  | 0.96x |
| point-cloud/yaml         | 10.63 ms | 36.58 ms | 0.29x | 2.60 ms  | 2.74 ms  | 0.95x |
| registry/yaml            | 4.31 ms  | 15.23 ms | 0.28x | 1.26 ms  | 2.28 ms  | 0.55x |
| tree/yaml                | 17.36 ms | 71.14 ms | 0.24x | 4.80 ms  | 7.71 ms  | 0.62x |
| twitter/toml             | 804.2 us | 1.72 ms  | 0.47x | 1.14 ms  | 1.03 ms  | 1.11x |
| canada/toml              | 4.53 ms  | 10.01 ms | 0.45x | 3.76 ms  | 3.63 ms  | 1.04x |
| citm-catalog/toml        | 1.98 ms  | 4.22 ms  | 0.47x | 2.18 ms  | 2.63 ms  | 0.83x |
| cargo-manifest/toml      | 15.8 us  | 20.2 us  | 0.78x | 13.9 us  | 12.6 us  | 1.10x |
| web-sys-manifest/toml    | 296.5 us | 450.8 us | 0.66x | 203.2 us | 139.7 us | 1.45x |
| cargo-lock/toml          | 75.7 us  | 142.8 us | 0.53x | 116.3 us | 115.0 us | 1.01x |
| saphyr/toml              | 623.9 us | 1.45 ms  | 0.43x | 1.79 ms  | 1.76 ms  | 1.02x |
| github/toml              | 2.24 ms  | 4.33 ms  | 0.52x | 3.68 ms  | 3.35 ms  | 1.10x |
| kubernetes/toml          | 9.52 ms  | 19.94 ms | 0.48x | 13.03 ms | 13.40 ms | 0.97x |
| manifests/toml           | 1.31 ms  | 3.13 ms  | 0.42x | 1.51 ms  | 2.05 ms  | 0.74x |
| logs/toml                | 6.05 ms  | 11.63 ms | 0.52x | 7.27 ms  | 6.49 ms  | 1.12x |
| features/toml            | 5.39 ms  | 12.34 ms | 0.44x | 4.06 ms  | 5.28 ms  | 0.77x |
| point-cloud/toml         | 3.37 ms  | 8.72 ms  | 0.39x | 2.74 ms  | 3.05 ms  | 0.90x |
| registry/toml            | 1.92 ms  | 3.54 ms  | 0.54x | 1.35 ms  | 1.44 ms  | 0.94x |
| tree/toml                | 6.18 ms  | 13.57 ms | 0.46x | 5.95 ms  | 7.77 ms  | 0.77x |
| table/csv                | 6.30 ms  | 3.70 ms  | 1.70x | 5.23 ms  | 1.67 ms  | 3.14x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 668/498 us in JSON, 504/178 us in CBOR, 449/171 us in
MessagePack, 4.50/1.24 ms in YAML and 1.15/2.16 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 238 us, 0.93x of
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
