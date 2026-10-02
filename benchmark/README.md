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
| JSON        | 1.07x | 0.65x-1.51x | 0.95x | 0.34x-1.78x |
| CBOR        | 0.76x | 0.54x-1.25x | 1.33x | 1.02x-1.84x |
| MessagePack | 1.41x | 0.89x-2.77x | 1.34x | 0.93x-1.82x |
| YAML        | 0.28x | 0.21x-0.40x | 0.53x | 0.26x-1.01x |
| TOML        | 0.50x | 0.39x-0.79x | 0.97x | 0.72x-1.47x |

* **YAML and TOML** deserialize 2.5 to 5 times as fast as serde-saphyr
  (YAML) and 1.3 to 2.6 times as fast as toml (TOML).  Serializing is
  faster or on par (TOML up to 1.14x slower), except for the large table
  of web-sys-manifest in TOML (1.47x).
* **JSON** serializes faster than serde_json or on par, except for tree
  (1.78x), citm-catalog (1.40x), cargo-manifest (1.38x), logs (1.36x)
  and manifests (1.26x).  Deserializing is faster for logs, manifests
  and citm-catalog, on par for saphyr and cargo-lock, 1.05x-1.09x slower
  for other string heavy data (twitter, github, web-sys-manifest) and
  1.11x-1.31x slower for floats and nesting (canada, registry,
  point-cloud, kubernetes, features), 1.51x for tree.
* **CBOR** deserializes faster than ciborium except for tree (1.14x) and
  citm-catalog (1.25x), serializing is 1.33x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 1.8x-2.8x slower for floats, nesting and integer keyed
  maps (point-cloud, canada, features, citm-catalog, tree).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.34x slower.
* **Untagged enums** (cargo-manifest) are 1.28x slower in JSON and 1.55x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 8.85 ms  | 11.84 ms | 0.75x | 6.95 ms | 9.52 ms | 0.73x |
| session-anthropic/json | 25.5 MiB | 3.03 ms  | 4.25 ms  | 0.71x | 3.09 ms | 8.18 ms | 0.38x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 400.3 us | 159.0 us |
| serde_json | 385.0 us | 208.1 us |
| miniserde  | 434.3 us | 305.2 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 411.0 us | 391.4 us | 1.05x | 159.7 us | 208.7 us | 0.77x |
| canada/json              | 2.33 ms  | 2.11 ms  | 1.11x | 1.24 ms  | 1.25 ms  | 0.99x |
| citm-catalog/json        | 864.1 us | 958.9 us | 0.90x | 321.0 us | 229.0 us | 1.40x |
| cargo-manifest/json      | 12.0 us  | 9.4 us   | 1.28x | 2.9 us   | 2.1 us   | 1.38x |
| web-sys-manifest/json    | 191.4 us | 175.8 us | 1.09x | 23.3 us  | 27.0 us  | 0.86x |
| cargo-lock/json          | 37.4 us  | 38.3 us  | 0.98x | 13.4 us  | 19.3 us  | 0.69x |
| saphyr/json              | 348.8 us | 357.8 us | 0.97x | 99.4 us  | 296.4 us | 0.34x |
| github/json              | 1.01 ms  | 951.4 us | 1.06x | 382.1 us | 587.6 us | 0.65x |
| kubernetes/json          | 5.30 ms  | 4.18 ms  | 1.27x | 1.62 ms  | 1.58 ms  | 1.03x |
| manifests/json           | 686.7 us | 820.0 us | 0.84x | 253.0 us | 200.1 us | 1.26x |
| logs/json                | 3.48 ms  | 5.36 ms  | 0.65x | 1.89 ms  | 1.39 ms  | 1.36x |
| features/json            | 2.75 ms  | 2.10 ms  | 1.31x | 1.56 ms  | 1.77 ms  | 0.88x |
| point-cloud/json         | 1.39 ms  | 1.17 ms  | 1.18x | 1.07 ms  | 1.23 ms  | 0.87x |
| registry/json            | 1.25 ms  | 1.11 ms  | 1.13x | 241.2 us | 226.4 us | 1.07x |
| tree/json                | 2.05 ms  | 1.36 ms  | 1.51x | 933.5 us | 525.8 us | 1.78x |
| twitter/cbor             | 421.0 us | 507.0 us | 0.83x | 126.7 us | 121.2 us | 1.05x |
| canada/cbor              | 1.07 ms  | 1.44 ms  | 0.74x | 605.2 us | 438.2 us | 1.38x |
| citm-catalog/cbor        | 767.5 us | 616.1 us | 1.25x | 268.6 us | 147.1 us | 1.83x |
| cargo-manifest/cbor      | 12.2 us  | 16.3 us  | 0.75x | 2.6 us   | 1.7 us   | 1.53x |
| web-sys-manifest/cbor    | 195.4 us | 245.1 us | 0.80x | 21.0 us  | 19.8 us  | 1.06x |
| cargo-lock/cbor          | 39.6 us  | 66.3 us  | 0.60x | 12.0 us  | 11.4 us  | 1.05x |
| saphyr/cbor              | 322.1 us | 491.8 us | 0.65x | 96.5 us  | 68.7 us  | 1.40x |
| github/cbor              | 1.09 ms  | 1.41 ms  | 0.77x | 327.6 us | 306.4 us | 1.07x |
| kubernetes/cbor          | 5.14 ms  | 5.81 ms  | 0.89x | 1.47 ms  | 798.8 us | 1.84x |
| manifests/cbor           | 750.2 us | 1.40 ms  | 0.54x | 223.4 us | 133.8 us | 1.67x |
| logs/cbor                | 3.60 ms  | 6.39 ms  | 0.56x | 1.70 ms  | 1.27 ms  | 1.34x |
| features/cbor            | 1.01 ms  | 1.06 ms  | 0.95x | 554.1 us | 385.9 us | 1.44x |
| point-cloud/cbor         | 761.7 us | 1.10 ms  | 0.69x | 437.8 us | 418.7 us | 1.05x |
| registry/cbor            | 986.2 us | 1.72 ms  | 0.57x | 246.6 us | 242.2 us | 1.02x |
| tree/cbor                | 2.20 ms  | 1.93 ms  | 1.14x | 785.3 us | 487.2 us | 1.61x |
| twitter/msgpack          | 372.4 us | 312.8 us | 1.19x | 122.7 us | 106.8 us | 1.15x |
| canada/msgpack           | 891.1 us | 483.9 us | 1.84x | 565.4 us | 386.5 us | 1.46x |
| citm-catalog/msgpack     | 686.4 us | 279.5 us | 2.46x | 286.1 us | 189.4 us | 1.51x |
| cargo-manifest/msgpack   | 11.5 us  | 7.4 us   | 1.55x | 2.6 us   | 1.6 us   | 1.62x |
| web-sys-manifest/msgpack | 189.3 us | 170.7 us | 1.11x | 20.8 us  | 22.3 us  | 0.93x |
| cargo-lock/msgpack       | 35.6 us  | 29.9 us  | 1.19x | 11.8 us  | 11.6 us  | 1.02x |
| saphyr/msgpack           | 291.3 us | 270.0 us | 1.08x | 90.0 us  | 66.2 us  | 1.36x |
| github/msgpack           | 987.5 us | 900.0 us | 1.10x | 308.2 us | 293.5 us | 1.05x |
| kubernetes/msgpack       | 4.87 ms  | 3.23 ms  | 1.51x | 1.47 ms  | 804.0 us | 1.82x |
| manifests/msgpack        | 677.0 us | 760.3 us | 0.89x | 222.6 us | 139.1 us | 1.60x |
| logs/msgpack             | 3.35 ms  | 3.69 ms  | 0.91x | 1.67 ms  | 1.30 ms  | 1.28x |
| features/msgpack         | 877.6 us | 470.4 us | 1.87x | 520.5 us | 331.4 us | 1.57x |
| point-cloud/msgpack      | 650.2 us | 359.2 us | 1.81x | 399.6 us | 275.9 us | 1.45x |
| registry/msgpack         | 950.7 us | 781.8 us | 1.22x | 224.0 us | 222.7 us | 1.01x |
| tree/msgpack             | 1.98 ms  | 715.4 us | 2.77x | 815.2 us | 485.5 us | 1.68x |
| twitter/yaml             | 2.53 ms  | 9.79 ms  | 0.26x | 1.41 ms  | 3.54 ms  | 0.40x |
| canada/yaml              | 15.05 ms | 52.36 ms | 0.29x | 3.35 ms  | 3.33 ms  | 1.01x |
| citm-catalog/yaml        | 5.20 ms  | 20.75 ms | 0.25x | 1.95 ms  | 3.41 ms  | 0.57x |
| cargo-manifest/yaml      | 36.7 us  | 137.9 us | 0.27x | 19.7 us  | 31.9 us  | 0.62x |
| web-sys-manifest/yaml    | 498.7 us | 1.66 ms  | 0.30x | 203.2 us | 494.7 us | 0.41x |
| cargo-lock/yaml          | 193.2 us | 709.2 us | 0.27x | 126.9 us | 347.6 us | 0.37x |
| saphyr/yaml              | 2.26 ms  | 5.68 ms  | 0.40x | 1.37 ms  | 5.21 ms  | 0.26x |
| github/yaml              | 5.29 ms  | 19.97 ms | 0.26x | 3.83 ms  | 12.06 ms | 0.32x |
| kubernetes/yaml          | 18.19 ms | 59.05 ms | 0.31x | 11.02 ms | 21.51 ms | 0.51x |
| manifests/yaml           | 3.00 ms  | 14.04 ms | 0.21x | 1.76 ms  | 3.69 ms  | 0.48x |
| logs/yaml                | 14.88 ms | 58.90 ms | 0.25x | 10.03 ms | 17.64 ms | 0.57x |
| features/yaml            | 15.56 ms | 49.99 ms | 0.31x | 3.73 ms  | 3.93 ms  | 0.95x |
| point-cloud/yaml         | 10.60 ms | 36.44 ms | 0.29x | 2.63 ms  | 2.74 ms  | 0.96x |
| registry/yaml            | 4.29 ms  | 15.25 ms | 0.28x | 1.31 ms  | 2.33 ms  | 0.56x |
| tree/yaml                | 17.39 ms | 71.01 ms | 0.24x | 4.86 ms  | 7.70 ms  | 0.63x |
| twitter/toml             | 814.0 us | 1.73 ms  | 0.47x | 1.15 ms  | 1.02 ms  | 1.13x |
| canada/toml              | 4.69 ms  | 10.21 ms | 0.46x | 3.82 ms  | 3.75 ms  | 1.02x |
| citm-catalog/toml        | 1.99 ms  | 4.13 ms  | 0.48x | 2.20 ms  | 2.65 ms  | 0.83x |
| cargo-manifest/toml      | 16.0 us  | 20.3 us  | 0.79x | 13.6 us  | 12.5 us  | 1.09x |
| web-sys-manifest/toml    | 299.2 us | 436.4 us | 0.69x | 207.3 us | 141.2 us | 1.47x |
| cargo-lock/toml          | 76.7 us  | 142.9 us | 0.54x | 112.8 us | 115.7 us | 0.97x |
| saphyr/toml              | 624.9 us | 1.45 ms  | 0.43x | 1.75 ms  | 1.75 ms  | 1.00x |
| github/toml              | 2.29 ms  | 4.38 ms  | 0.52x | 3.72 ms  | 3.33 ms  | 1.12x |
| kubernetes/toml          | 9.83 ms  | 19.57 ms | 0.50x | 12.56 ms | 13.52 ms | 0.93x |
| manifests/toml           | 1.32 ms  | 3.14 ms  | 0.42x | 1.51 ms  | 2.10 ms  | 0.72x |
| logs/toml                | 6.10 ms  | 11.83 ms | 0.52x | 7.31 ms  | 6.41 ms  | 1.14x |
| features/toml            | 5.49 ms  | 12.23 ms | 0.45x | 4.10 ms  | 5.12 ms  | 0.80x |
| point-cloud/toml         | 3.40 ms  | 8.66 ms  | 0.39x | 2.74 ms  | 3.28 ms  | 0.84x |
| registry/toml            | 1.96 ms  | 3.57 ms  | 0.55x | 1.36 ms  | 1.47 ms  | 0.93x |
| tree/toml                | 6.29 ms  | 13.45 ms | 0.47x | 6.06 ms  | 7.97 ms  | 0.76x |
| table/csv                | 5.03 ms  | 3.73 ms  | 1.35x | 2.70 ms  | 1.72 ms  | 1.57x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 642/509 us in JSON, 512/178 us in CBOR, 464/174 us in
MessagePack, 4.52/1.24 ms in YAML and 1.15/2.10 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 225 us, 0.88x of
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
