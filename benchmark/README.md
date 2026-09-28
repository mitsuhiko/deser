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
| JSON        | 1.24x | 0.75x-1.79x | 0.85x | 0.35x-1.45x |
| CBOR        | 0.82x | 0.53x-1.31x | 1.37x | 1.00x-1.93x |
| MessagePack | 1.44x | 0.87x-2.69x | 1.33x | 0.95x-1.80x |
| YAML        | 0.31x | 0.24x-0.44x | 0.56x | 0.24x-0.98x |
| TOML        | 0.51x | 0.42x-0.81x | 0.94x | 0.70x-1.41x |

* **YAML and TOML** deserialize two to four times as fast as serde-saphyr
  and toml.  Serializing is faster or on par, except for the large table
  of web-sys-manifest in TOML (1.41x).
* **JSON** serializes faster than serde_json, except for tree (1.45x),
  logs (1.27x) and cargo-manifest (1.12x).  Deserializing is faster for
  manifests and logs and 1.04x-1.13x slower for other string heavy data
  (twitter, citm-catalog, saphyr, web-sys-manifest) but 1.32x-1.79x
  slower for floats and nesting (canada, features, point-cloud, tree,
  kubernetes).
* **CBOR** deserializes faster than ciborium, serializing is 1.4x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 2.1x-2.7x slower for floats, nesting and integer keyed
  maps (canada, features, point-cloud, tree, citm-catalog).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.3x slower.
* **Untagged enums** (cargo-manifest) are 1.43x slower in JSON and 1.54x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 11.17 ms | 12.56 ms | 0.89x | 7.07 ms | 9.93 ms | 0.71x |
| session-anthropic/json | 25.5 MiB | 4.09 ms  | 4.38 ms  | 0.93x | 3.14 ms | 8.43 ms | 0.37x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 473.0 us | 161.6 us |
| serde_json | 396.7 us | 217.3 us |
| miniserde  | 439.5 us | 317.7 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 480.9 us | 429.4 us | 1.12x | 157.7 us | 279.9 us | 0.56x |
| canada/json              | 3.25 ms  | 2.11 ms  | 1.54x | 1.24 ms  | 1.25 ms  | 1.00x |
| citm-catalog/json        | 995.7 us | 957.9 us | 1.04x | 317.9 us | 306.1 us | 1.04x |
| cargo-manifest/json      | 13.9 us  | 9.7 us   | 1.43x | 2.9 us   | 2.6 us   | 1.12x |
| web-sys-manifest/json    | 214.8 us | 189.8 us | 1.13x | 23.4 us  | 31.3 us  | 0.75x |
| cargo-lock/json          | 46.2 us  | 38.0 us  | 1.22x | 13.1 us  | 20.1 us  | 0.65x |
| saphyr/json              | 403.7 us | 358.9 us | 1.12x | 98.3 us  | 278.5 us | 0.35x |
| github/json              | 1.20 ms  | 964.6 us | 1.25x | 366.8 us | 618.8 us | 0.59x |
| kubernetes/json          | 5.79 ms  | 4.39 ms  | 1.32x | 1.61 ms  | 1.69 ms  | 0.95x |
| manifests/json           | 811.0 us | 872.4 us | 0.93x | 250.0 us | 251.3 us | 0.99x |
| logs/json                | 4.13 ms  | 5.51 ms  | 0.75x | 1.88 ms  | 1.48 ms  | 1.27x |
| features/json            | 3.45 ms  | 2.13 ms  | 1.62x | 1.61 ms  | 1.77 ms  | 0.91x |
| point-cloud/json         | 1.86 ms  | 1.22 ms  | 1.52x | 1.07 ms  | 1.32 ms  | 0.81x |
| registry/json            | 1.36 ms  | 1.10 ms  | 1.23x | 246.9 us | 237.7 us | 1.04x |
| tree/json                | 2.41 ms  | 1.35 ms  | 1.79x | 954.5 us | 656.8 us | 1.45x |
| twitter/cbor             | 426.5 us | 507.7 us | 0.84x | 124.6 us | 111.9 us | 1.11x |
| canada/cbor              | 1.39 ms  | 1.50 ms  | 0.93x | 609.0 us | 412.4 us | 1.48x |
| citm-catalog/cbor        | 763.5 us | 619.7 us | 1.23x | 265.6 us | 139.1 us | 1.91x |
| cargo-manifest/cbor      | 13.1 us  | 15.3 us  | 0.86x | 2.7 us   | 1.5 us   | 1.80x |
| web-sys-manifest/cbor    | 206.2 us | 245.4 us | 0.84x | 20.4 us  | 19.0 us  | 1.07x |
| cargo-lock/cbor          | 40.1 us  | 63.0 us  | 0.64x | 11.3 us  | 9.7 us   | 1.16x |
| saphyr/cbor              | 333.5 us | 555.0 us | 0.60x | 93.0 us  | 59.0 us  | 1.58x |
| github/cbor              | 1.08 ms  | 1.44 ms  | 0.75x | 304.8 us | 287.7 us | 1.06x |
| kubernetes/cbor          | 5.27 ms  | 5.74 ms  | 0.92x | 1.44 ms  | 744.1 us | 1.93x |
| manifests/cbor           | 737.9 us | 1.40 ms  | 0.53x | 221.5 us | 135.4 us | 1.64x |
| logs/cbor                | 3.78 ms  | 6.06 ms  | 0.62x | 1.63 ms  | 1.22 ms  | 1.34x |
| features/cbor            | 1.35 ms  | 1.13 ms  | 1.19x | 564.7 us | 453.4 us | 1.25x |
| point-cloud/cbor         | 970.4 us | 1.14 ms  | 0.85x | 438.1 us | 407.0 us | 1.08x |
| registry/cbor            | 994.4 us | 1.45 ms  | 0.69x | 239.8 us | 239.3 us | 1.00x |
| tree/cbor                | 2.21 ms  | 1.68 ms  | 1.31x | 787.3 us | 474.2 us | 1.66x |
| twitter/msgpack          | 357.5 us | 315.9 us | 1.13x | 123.0 us | 111.8 us | 1.10x |
| canada/msgpack           | 1.14 ms  | 490.0 us | 2.33x | 566.2 us | 313.7 us | 1.80x |
| citm-catalog/msgpack     | 673.8 us | 298.0 us | 2.26x | 279.4 us | 208.1 us | 1.34x |
| cargo-manifest/msgpack   | 12.0 us  | 7.8 us   | 1.54x | 2.7 us   | 1.7 us   | 1.59x |
| web-sys-manifest/msgpack | 194.3 us | 182.5 us | 1.06x | 21.4 us  | 22.1 us  | 0.97x |
| cargo-lock/msgpack       | 35.4 us  | 30.6 us  | 1.16x | 11.3 us  | 11.9 us  | 0.95x |
| saphyr/msgpack           | 288.1 us | 276.5 us | 1.04x | 87.8 us  | 69.5 us  | 1.26x |
| github/msgpack           | 936.4 us | 934.5 us | 1.00x | 296.3 us | 289.0 us | 1.03x |
| kubernetes/msgpack       | 4.89 ms  | 3.27 ms  | 1.50x | 1.46 ms  | 807.3 us | 1.80x |
| manifests/msgpack        | 671.7 us | 770.6 us | 0.87x | 221.6 us | 148.5 us | 1.49x |
| logs/msgpack             | 3.47 ms  | 3.74 ms  | 0.93x | 1.65 ms  | 1.30 ms  | 1.26x |
| features/msgpack         | 1.10 ms  | 454.1 us | 2.43x | 544.1 us | 307.9 us | 1.77x |
| point-cloud/msgpack      | 749.6 us | 362.5 us | 2.07x | 398.0 us | 234.0 us | 1.70x |
| registry/msgpack         | 957.0 us | 790.1 us | 1.21x | 226.9 us | 220.6 us | 1.03x |
| tree/msgpack             | 1.94 ms  | 720.9 us | 2.69x | 808.3 us | 566.2 us | 1.43x |
| twitter/yaml             | 2.63 ms  | 9.10 ms  | 0.29x | 1.38 ms  | 3.31 ms  | 0.42x |
| canada/yaml              | 15.11 ms | 46.41 ms | 0.33x | 3.41 ms  | 3.47 ms  | 0.98x |
| citm-catalog/yaml        | 5.28 ms  | 18.78 ms | 0.28x | 1.89 ms  | 3.09 ms  | 0.61x |
| cargo-manifest/yaml      | 37.6 us  | 126.2 us | 0.30x | 19.7 us  | 29.2 us  | 0.67x |
| web-sys-manifest/yaml    | 509.9 us | 1.52 ms  | 0.34x | 198.9 us | 445.9 us | 0.45x |
| cargo-lock/yaml          | 198.3 us | 642.3 us | 0.31x | 133.8 us | 319.8 us | 0.42x |
| saphyr/yaml              | 2.35 ms  | 5.30 ms  | 0.44x | 1.38 ms  | 5.76 ms  | 0.24x |
| github/yaml              | 5.69 ms  | 18.41 ms | 0.31x | 3.88 ms  | 11.40 ms | 0.34x |
| kubernetes/yaml          | 18.07 ms | 51.13 ms | 0.35x | 10.28 ms | 20.02 ms | 0.51x |
| manifests/yaml           | 3.06 ms  | 12.86 ms | 0.24x | 1.74 ms  | 3.18 ms  | 0.55x |
| logs/yaml                | 14.79 ms | 53.32 ms | 0.28x | 9.65 ms  | 15.51 ms | 0.62x |
| features/yaml            | 15.82 ms | 44.31 ms | 0.36x | 3.88 ms  | 4.01 ms  | 0.97x |
| point-cloud/yaml         | 10.28 ms | 32.29 ms | 0.32x | 2.65 ms  | 2.87 ms  | 0.92x |
| registry/yaml            | 4.37 ms  | 14.05 ms | 0.31x | 1.29 ms  | 2.18 ms  | 0.59x |
| tree/yaml                | 17.59 ms | 59.19 ms | 0.30x | 4.67 ms  | 7.05 ms  | 0.66x |
| twitter/toml             | 837.0 us | 1.77 ms  | 0.47x | 1.11 ms  | 1.05 ms  | 1.06x |
| canada/toml              | 5.06 ms  | 10.38 ms | 0.49x | 3.87 ms  | 3.95 ms  | 0.98x |
| citm-catalog/toml        | 2.06 ms  | 4.47 ms  | 0.46x | 2.22 ms  | 2.67 ms  | 0.83x |
| cargo-manifest/toml      | 16.7 us  | 20.7 us  | 0.81x | 14.0 us  | 13.0 us  | 1.08x |
| web-sys-manifest/toml    | 302.9 us | 471.2 us | 0.64x | 205.5 us | 146.0 us | 1.41x |
| cargo-lock/toml          | 78.4 us  | 144.5 us | 0.54x | 117.4 us | 116.0 us | 1.01x |
| saphyr/toml              | 644.8 us | 1.47 ms  | 0.44x | 1.73 ms  | 1.78 ms  | 0.97x |
| github/toml              | 2.35 ms  | 4.47 ms  | 0.52x | 3.52 ms  | 3.39 ms  | 1.04x |
| kubernetes/toml          | 10.10 ms | 20.72 ms | 0.49x | 11.74 ms | 13.57 ms | 0.86x |
| manifests/toml           | 1.35 ms  | 2.82 ms  | 0.48x | 1.48 ms  | 2.12 ms  | 0.70x |
| logs/toml                | 6.37 ms  | 12.06 ms | 0.53x | 7.17 ms  | 6.77 ms  | 1.06x |
| features/toml            | 6.04 ms  | 12.49 ms | 0.48x | 4.07 ms  | 5.34 ms  | 0.76x |
| point-cloud/toml         | 3.72 ms  | 7.84 ms  | 0.47x | 2.72 ms  | 3.29 ms  | 0.83x |
| registry/toml            | 2.02 ms  | 3.64 ms  | 0.56x | 1.39 ms  | 1.49 ms  | 0.93x |
| tree/toml                | 6.33 ms  | 15.02 ms | 0.42x | 6.28 ms  | 7.71 ms  | 0.81x |
| table/csv                | 6.14 ms  | 3.70 ms  | 1.66x | 4.59 ms  | 1.72 ms  | 2.67x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 759/504 us in JSON, 507/175 us in CBOR, 457/173 us in
MessagePack, 4.54/1.24 ms in YAML and 1.18/2.02 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 292 us, 1.13x of
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
