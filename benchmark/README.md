# Runtime Performance

This folder compares every format of deser with a serde based library on
the same data and types: `deser-json` with `serde_json`, `deser-cbor` with
`ciborium`, `deser-msgpack` with `rmp-serde`, `deser-yaml` with
`serde-saphyr`, `deser-toml` with `toml` and `deser-csv` with `csv`.  The results below are from
`cargo run --release -- time "" 10` (`make bench-versus` with 10 rounds)
on an Apple M5 Max with Rust 1.99.  Ratios are deser/serde, below 1 deser
is faster.  The benchmark is built with one codegen unit: it holds the
derived code of both libraries, and with more units a change to the
derived code of one library moved the code of the other into other units
(serde_json serialized up to 25% slower after the derived code of deser
got smaller).

## Where deser Stands

The geometric mean over the 15 datasets, with the best and the worst one:

| format      | de    | de range    | ser   | ser range   |
|-------------|-------|-------------|-------|-------------|
| JSON        | 1.08x | 0.66x-1.52x | 0.97x | 0.32x-1.92x |
| CBOR        | 0.77x | 0.53x-1.29x | 1.33x | 1.03x-1.83x |
| MessagePack | 1.43x | 0.89x-2.74x | 1.33x | 0.99x-1.77x |
| YAML        | 0.28x | 0.21x-0.39x | 0.56x | 0.24x-1.13x |
| TOML        | 0.51x | 0.40x-0.81x | 1.00x | 0.74x-1.55x |

* **YAML and TOML** deserialize 2.5 to 5 times as fast as serde-saphyr
  (YAML) and 1.2 to 2.5 times as fast as toml (TOML).  Serializing is
  faster or on par (YAML up to 1.13x slower, TOML up to 1.19x), except
  for the large table of web-sys-manifest in TOML (1.55x).
* **JSON** serializes faster than serde_json or on par, except for tree
  (1.92x), citm-catalog (1.50x), cargo-manifest (1.45x), logs (1.38x),
  manifests (1.29x) and registry (1.17x).  Deserializing is faster for
  logs, manifests and citm-catalog, on par for saphyr and cargo-lock,
  1.06x-1.09x slower for other string heavy data (twitter, github,
  web-sys-manifest) and 1.09x-1.33x slower for floats and nesting
  (canada, registry, point-cloud, kubernetes, features), 1.52x for tree.
* **CBOR** deserializes faster than ciborium except for tree (1.17x) and
  citm-catalog (1.29x), serializing is 1.33x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 1.8x-2.7x slower for floats, nesting and integer keyed
  maps (point-cloud, canada, features, citm-catalog, tree).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.33x slower.
* **Untagged enums** (cargo-manifest) are 1.30x slower in JSON and 1.59x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 8.74 ms  | 12.28 ms | 0.71x | 7.03 ms | 9.89 ms | 0.71x |
| session-anthropic/json | 25.5 MiB | 3.02 ms  | 4.23 ms  | 0.72x | 3.04 ms | 8.22 ms | 0.37x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 406.2 us | 162.0 us |
| serde_json | 384.7 us | 207.6 us |
| miniserde  | 431.9 us | 302.2 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 412.2 us | 388.2 us | 1.06x | 159.6 us | 212.8 us | 0.75x |
| canada/json              | 2.33 ms  | 2.13 ms  | 1.09x | 1.23 ms  | 1.21 ms  | 1.02x |
| citm-catalog/json        | 868.4 us | 945.2 us | 0.92x | 330.0 us | 220.1 us | 1.50x |
| cargo-manifest/json      | 12.2 us  | 9.4 us   | 1.30x | 2.9 us   | 2.0 us   | 1.45x |
| web-sys-manifest/json    | 196.7 us | 180.0 us | 1.09x | 23.2 us  | 26.8 us  | 0.87x |
| cargo-lock/json          | 37.6 us  | 37.6 us  | 1.00x | 13.7 us  | 19.3 us  | 0.71x |
| saphyr/json              | 356.9 us | 354.0 us | 1.01x | 101.6 us | 314.2 us | 0.32x |
| github/json              | 1.02 ms  | 950.4 us | 1.07x | 372.6 us | 590.0 us | 0.63x |
| kubernetes/json          | 5.27 ms  | 4.16 ms  | 1.27x | 1.61 ms  | 1.60 ms  | 1.01x |
| manifests/json           | 693.0 us | 823.6 us | 0.84x | 249.7 us | 192.9 us | 1.29x |
| logs/json                | 3.47 ms  | 5.29 ms  | 0.66x | 1.90 ms  | 1.37 ms  | 1.38x |
| features/json            | 2.78 ms  | 2.09 ms  | 1.33x | 1.55 ms  | 1.75 ms  | 0.88x |
| point-cloud/json         | 1.36 ms  | 1.16 ms  | 1.17x | 1.06 ms  | 1.21 ms  | 0.87x |
| registry/json            | 1.25 ms  | 1.11 ms  | 1.13x | 257.7 us | 221.0 us | 1.17x |
| tree/json                | 2.06 ms  | 1.35 ms  | 1.52x | 982.4 us | 512.3 us | 1.92x |
| twitter/cbor             | 411.8 us | 517.6 us | 0.80x | 129.6 us | 123.2 us | 1.05x |
| canada/cbor              | 1.07 ms  | 1.44 ms  | 0.74x | 606.5 us | 446.9 us | 1.36x |
| citm-catalog/cbor        | 767.4 us | 596.4 us | 1.29x | 271.7 us | 152.1 us | 1.79x |
| cargo-manifest/cbor      | 12.6 us  | 15.0 us  | 0.84x | 2.6 us   | 1.7 us   | 1.53x |
| web-sys-manifest/cbor    | 202.4 us | 249.9 us | 0.81x | 22.5 us  | 19.5 us  | 1.15x |
| cargo-lock/cbor          | 38.0 us  | 62.0 us  | 0.61x | 12.0 us  | 11.0 us  | 1.09x |
| saphyr/cbor              | 326.0 us | 513.2 us | 0.64x | 95.8 us  | 67.8 us  | 1.41x |
| github/cbor              | 1.07 ms  | 1.44 ms  | 0.74x | 313.6 us | 281.8 us | 1.11x |
| kubernetes/cbor          | 5.20 ms  | 5.83 ms  | 0.89x | 1.43 ms  | 779.9 us | 1.83x |
| manifests/cbor           | 747.2 us | 1.41 ms  | 0.53x | 224.4 us | 133.0 us | 1.69x |
| logs/cbor                | 3.65 ms  | 6.37 ms  | 0.57x | 1.70 ms  | 1.25 ms  | 1.37x |
| features/cbor            | 1.04 ms  | 1.08 ms  | 0.97x | 549.4 us | 364.8 us | 1.51x |
| point-cloud/cbor         | 757.7 us | 1.10 ms  | 0.69x | 437.2 us | 417.7 us | 1.05x |
| registry/cbor            | 1.01 ms  | 1.61 ms  | 0.63x | 241.1 us | 233.2 us | 1.03x |
| tree/cbor                | 2.20 ms  | 1.88 ms  | 1.17x | 782.5 us | 590.5 us | 1.33x |
| twitter/msgpack          | 371.3 us | 312.2 us | 1.19x | 122.9 us | 111.6 us | 1.10x |
| canada/msgpack           | 907.6 us | 490.2 us | 1.85x | 562.9 us | 400.8 us | 1.40x |
| citm-catalog/msgpack     | 687.9 us | 279.4 us | 2.46x | 285.6 us | 185.5 us | 1.54x |
| cargo-manifest/msgpack   | 11.8 us  | 7.4 us   | 1.59x | 2.6 us   | 1.6 us   | 1.62x |
| web-sys-manifest/msgpack | 197.7 us | 168.0 us | 1.18x | 22.1 us  | 22.3 us  | 0.99x |
| cargo-lock/msgpack       | 34.8 us  | 29.9 us  | 1.16x | 11.6 us  | 11.5 us  | 1.01x |
| saphyr/msgpack           | 297.2 us | 260.2 us | 1.14x | 87.4 us  | 67.2 us  | 1.30x |
| github/msgpack           | 967.8 us | 908.3 us | 1.07x | 300.6 us | 295.3 us | 1.02x |
| kubernetes/msgpack       | 4.92 ms  | 3.25 ms  | 1.51x | 1.45 ms  | 818.1 us | 1.77x |
| manifests/msgpack        | 677.6 us | 764.7 us | 0.89x | 222.6 us | 141.6 us | 1.57x |
| logs/msgpack             | 3.40 ms  | 3.71 ms  | 0.92x | 1.66 ms  | 1.32 ms  | 1.26x |
| features/msgpack         | 896.5 us | 457.4 us | 1.96x | 523.7 us | 332.9 us | 1.57x |
| point-cloud/msgpack      | 662.3 us | 358.2 us | 1.85x | 398.2 us | 277.5 us | 1.43x |
| registry/msgpack         | 949.9 us | 768.1 us | 1.24x | 224.0 us | 218.5 us | 1.03x |
| tree/msgpack             | 1.98 ms  | 724.1 us | 2.74x | 818.3 us | 485.5 us | 1.69x |
| twitter/yaml             | 2.57 ms  | 9.79 ms  | 0.26x | 1.43 ms  | 3.35 ms  | 0.43x |
| canada/yaml              | 15.13 ms | 51.92 ms | 0.29x | 3.34 ms  | 2.96 ms  | 1.13x |
| citm-catalog/yaml        | 5.22 ms  | 20.78 ms | 0.25x | 1.93 ms  | 3.21 ms  | 0.60x |
| cargo-manifest/yaml      | 36.5 us  | 136.1 us | 0.27x | 19.5 us  | 29.3 us  | 0.67x |
| web-sys-manifest/yaml    | 500.4 us | 1.66 ms  | 0.30x | 199.9 us | 460.8 us | 0.43x |
| cargo-lock/yaml          | 194.9 us | 708.0 us | 0.28x | 129.7 us | 343.5 us | 0.38x |
| saphyr/yaml              | 2.28 ms  | 5.80 ms  | 0.39x | 1.31 ms  | 5.53 ms  | 0.24x |
| github/yaml              | 5.45 ms  | 19.98 ms | 0.27x | 3.90 ms  | 11.78 ms | 0.33x |
| kubernetes/yaml          | 18.25 ms | 56.18 ms | 0.32x | 10.79 ms | 21.05 ms | 0.51x |
| manifests/yaml           | 3.05 ms  | 13.93 ms | 0.22x | 1.72 ms  | 3.50 ms  | 0.49x |
| logs/yaml                | 14.41 ms | 58.40 ms | 0.25x | 10.17 ms | 17.39 ms | 0.59x |
| features/yaml            | 15.29 ms | 49.49 ms | 0.31x | 3.73 ms  | 3.61 ms  | 1.04x |
| point-cloud/yaml         | 10.52 ms | 36.24 ms | 0.29x | 2.59 ms  | 2.44 ms  | 1.06x |
| registry/yaml            | 4.34 ms  | 15.20 ms | 0.29x | 1.27 ms  | 2.20 ms  | 0.58x |
| tree/yaml                | 17.43 ms | 82.09 ms | 0.21x | 4.75 ms  | 7.44 ms  | 0.64x |
| twitter/toml             | 869.3 us | 1.75 ms  | 0.50x | 1.23 ms  | 1.05 ms  | 1.17x |
| canada/toml              | 4.60 ms  | 10.23 ms | 0.45x | 3.82 ms  | 3.62 ms  | 1.05x |
| citm-catalog/toml        | 2.21 ms  | 4.28 ms  | 0.52x | 2.34 ms  | 2.61 ms  | 0.90x |
| cargo-manifest/toml      | 16.5 us  | 20.3 us  | 0.81x | 14.2 us  | 12.9 us  | 1.10x |
| web-sys-manifest/toml    | 313.6 us | 449.9 us | 0.70x | 220.8 us | 142.9 us | 1.55x |
| cargo-lock/toml          | 78.6 us  | 150.2 us | 0.52x | 120.2 us | 118.8 us | 1.01x |
| saphyr/toml              | 634.8 us | 1.57 ms  | 0.40x | 1.99 ms  | 1.77 ms  | 1.12x |
| github/toml              | 2.40 ms  | 4.66 ms  | 0.52x | 4.10 ms  | 3.44 ms  | 1.19x |
| kubernetes/toml          | 10.19 ms | 19.59 ms | 0.52x | 13.01 ms | 14.17 ms | 0.92x |
| manifests/toml           | 1.40 ms  | 3.14 ms  | 0.44x | 1.59 ms  | 2.14 ms  | 0.74x |
| logs/toml                | 6.33 ms  | 11.99 ms | 0.53x | 7.47 ms  | 6.82 ms  | 1.09x |
| features/toml            | 5.52 ms  | 12.20 ms | 0.45x | 4.07 ms  | 5.12 ms  | 0.79x |
| point-cloud/toml         | 3.43 ms  | 8.66 ms  | 0.40x | 2.73 ms  | 3.16 ms  | 0.86x |
| registry/toml            | 2.01 ms  | 3.60 ms  | 0.56x | 1.41 ms  | 1.45 ms  | 0.97x |
| tree/toml                | 6.80 ms  | 15.26 ms | 0.45x | 6.60 ms  | 8.11 ms  | 0.81x |
| table/csv                | 5.04 ms  | 3.76 ms  | 1.34x | 2.69 ms  | 1.76 ms  | 1.53x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 643/513 us in JSON, 516/181 us in CBOR, 468/175 us in
MessagePack, 4.43/1.25 ms in YAML and 1.19/2.23 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 226 us, 0.88x of
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
