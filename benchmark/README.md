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
| JSON        | 1.13x | 0.69x-1.69x | 0.96x | 0.38x-1.79x |
| CBOR        | 0.79x | 0.54x-1.27x | 1.33x | 1.00x-1.87x |
| MessagePack | 1.43x | 0.88x-2.77x | 1.33x | 1.00x-1.83x |
| YAML        | 0.28x | 0.21x-0.40x | 0.52x | 0.27x-1.04x |
| TOML        | 0.50x | 0.40x-0.79x | 0.96x | 0.72x-1.49x |

* **YAML and TOML** deserialize 2.5 to 5 times as fast as serde-saphyr
  (YAML) and 1.3 to 2.5 times as fast as toml (TOML).  Serializing is
  faster or on par (TOML up to 1.12x slower), except for the large table
  of web-sys-manifest in TOML (1.49x).
* **JSON** serializes faster than serde_json or on par, except for tree
  (1.79x), citm-catalog (1.40x), logs (1.38x), cargo-manifest (1.38x)
  and manifests (1.28x).  Deserializing is faster for logs and
  manifests, on par for citm-catalog and saphyr, 1.06x-1.15x slower for
  other string heavy data (cargo-lock, github, web-sys-manifest,
  twitter) and 1.15x-1.39x slower for floats and nesting (registry,
  canada, point-cloud, kubernetes, features), 1.69x for tree.
* **CBOR** deserializes faster than ciborium except for tree (1.17x) and
  citm-catalog (1.27x), serializing is 1.33x slower.
* **MessagePack** is on par with rmp-serde for string heavy data but
  deserializes 1.8x-2.8x slower for floats, nesting and integer keyed
  maps (canada, point-cloud, features, citm-catalog, tree).  deser-msgpack
  is a bit faster than deser-cbor there, the gap comes from rmp-serde
  being two to three times as fast as ciborium on this data.
  Serializing is 1.33x slower.
* **Untagged enums** (cargo-manifest) are 1.32x slower in JSON and 1.56x
  in MessagePack, in the other formats they are faster.

The two sessions of the pi coding agent are only benchmarked with JSON
(line by line, like pi reads and writes them) and are not part of the
geometric means above:

| benchmark              | input    | de       | serde    | ratio | ser     | serde   | ratio |
|------------------------|----------|----------|----------|-------|---------|---------|-------|
| session-openai/json    | 18.9 MiB | 9.13 ms  | 11.93 ms | 0.77x | 6.95 ms | 9.54 ms | 0.73x |
| session-anthropic/json | 25.5 MiB | 3.11 ms  | 4.26 ms  | 0.73x | 2.90 ms | 8.08 ms | 0.36x |

The OpenAI session is mostly text with many escapes (code, diffs, tool
output and signatures which are JSON in strings), the Anthropic session
is mostly base64 images.

For the Twitter JSON document `cargo bench` also compares with miniserde:

| library    | de       | ser      |
|------------|----------|----------|
| deser-json | 464.0 us | 164.9 us |
| serde_json | 415.2 us | 216.4 us |
| miniserde  | 473.7 us | 314.4 us |

Profiling findings, attempted optimizations and follow-up ideas are tracked
in [PERF_NOTES.md](PERF_NOTES.md).

## Full Results

| benchmark                | de       | serde    | ratio | ser      | serde    | ratio |
|--------------------------|----------|----------|-------|----------|----------|-------|
| twitter/json             | 447.2 us | 388.0 us | 1.15x | 159.7 us | 210.0 us | 0.76x |
| canada/json              | 2.47 ms  | 2.11 ms  | 1.17x | 1.23 ms  | 1.24 ms  | 0.99x |
| citm-catalog/json        | 928.7 us | 959.4 us | 0.97x | 318.7 us | 227.0 us | 1.40x |
| cargo-manifest/json      | 12.1 us  | 9.2 us   | 1.32x | 2.9 us   | 2.1 us   | 1.38x |
| web-sys-manifest/json    | 203.2 us | 178.2 us | 1.14x | 23.5 us  | 26.7 us  | 0.88x |
| cargo-lock/json          | 39.3 us  | 37.0 us  | 1.06x | 13.4 us  | 19.1 us  | 0.70x |
| saphyr/json              | 360.2 us | 355.1 us | 1.01x | 106.8 us | 278.9 us | 0.38x |
| github/json              | 1.05 ms  | 939.5 us | 1.12x | 372.9 us | 583.1 us | 0.64x |
| kubernetes/json          | 5.45 ms  | 4.14 ms  | 1.32x | 1.61 ms  | 1.58 ms  | 1.02x |
| manifests/json           | 719.4 us | 814.2 us | 0.88x | 254.0 us | 198.3 us | 1.28x |
| logs/json                | 3.66 ms  | 5.31 ms  | 0.69x | 1.90 ms  | 1.38 ms  | 1.38x |
| features/json            | 2.88 ms  | 2.08 ms  | 1.39x | 1.55 ms  | 1.76 ms  | 0.88x |
| point-cloud/json         | 1.47 ms  | 1.17 ms  | 1.25x | 1.08 ms  | 1.24 ms  | 0.87x |
| registry/json            | 1.30 ms  | 1.13 ms  | 1.15x | 242.2 us | 226.4 us | 1.07x |
| tree/json                | 2.29 ms  | 1.36 ms  | 1.69x | 940.8 us | 526.3 us | 1.79x |
| twitter/cbor             | 413.9 us | 490.1 us | 0.84x | 126.5 us | 120.9 us | 1.05x |
| canada/cbor              | 1.07 ms  | 1.43 ms  | 0.75x | 609.4 us | 442.1 us | 1.38x |
| citm-catalog/cbor        | 768.3 us | 603.1 us | 1.27x | 269.1 us | 149.5 us | 1.80x |
| cargo-manifest/cbor      | 12.1 us  | 14.8 us  | 0.82x | 2.6 us   | 1.6 us   | 1.62x |
| web-sys-manifest/cbor    | 201.6 us | 241.3 us | 0.84x | 20.3 us  | 19.6 us  | 1.04x |
| cargo-lock/cbor          | 39.0 us  | 59.5 us  | 0.66x | 12.0 us  | 11.3 us  | 1.06x |
| saphyr/cbor              | 321.2 us | 478.4 us | 0.67x | 103.8 us | 74.3 us  | 1.40x |
| github/cbor              | 1.09 ms  | 1.41 ms  | 0.77x | 318.3 us | 294.8 us | 1.08x |
| kubernetes/cbor          | 5.09 ms  | 5.52 ms  | 0.92x | 1.45 ms  | 776.4 us | 1.87x |
| manifests/cbor           | 742.3 us | 1.37 ms  | 0.54x | 223.9 us | 134.8 us | 1.66x |
| logs/cbor                | 3.63 ms  | 5.96 ms  | 0.61x | 1.72 ms  | 1.27 ms  | 1.36x |
| features/cbor            | 1.03 ms  | 1.05 ms  | 0.98x | 557.3 us | 387.2 us | 1.44x |
| point-cloud/cbor         | 757.5 us | 1.09 ms  | 0.70x | 437.6 us | 420.0 us | 1.04x |
| registry/cbor            | 982.2 us | 1.41 ms  | 0.70x | 233.4 us | 234.5 us | 1.00x |
| tree/cbor                | 2.20 ms  | 1.89 ms  | 1.17x | 809.7 us | 495.5 us | 1.63x |
| twitter/msgpack          | 367.9 us | 312.8 us | 1.18x | 121.5 us | 108.6 us | 1.12x |
| canada/msgpack           | 887.9 us | 481.5 us | 1.84x | 566.0 us | 408.0 us | 1.39x |
| citm-catalog/msgpack     | 693.2 us | 280.7 us | 2.47x | 285.9 us | 185.1 us | 1.54x |
| cargo-manifest/msgpack   | 11.4 us  | 7.3 us   | 1.56x | 2.6 us   | 1.6 us   | 1.62x |
| web-sys-manifest/msgpack | 193.6 us | 172.3 us | 1.12x | 21.2 us  | 21.2 us  | 1.00x |
| cargo-lock/msgpack       | 34.9 us  | 28.4 us  | 1.23x | 11.7 us  | 11.4 us  | 1.03x |
| saphyr/msgpack           | 294.0 us | 257.1 us | 1.14x | 95.9 us  | 73.1 us  | 1.31x |
| github/msgpack           | 962.5 us | 878.1 us | 1.10x | 301.2 us | 293.2 us | 1.03x |
| kubernetes/msgpack       | 4.80 ms  | 3.21 ms  | 1.50x | 1.46 ms  | 801.1 us | 1.83x |
| manifests/msgpack        | 675.2 us | 766.5 us | 0.88x | 223.4 us | 143.6 us | 1.56x |
| logs/msgpack             | 3.32 ms  | 3.66 ms  | 0.91x | 1.67 ms  | 1.29 ms  | 1.29x |
| features/msgpack         | 867.3 us | 446.1 us | 1.94x | 523.6 us | 349.1 us | 1.50x |
| point-cloud/msgpack      | 653.3 us | 358.7 us | 1.82x | 393.6 us | 270.4 us | 1.46x |
| registry/msgpack         | 940.8 us | 767.6 us | 1.23x | 221.5 us | 220.4 us | 1.00x |
| tree/msgpack             | 1.99 ms  | 719.4 us | 2.77x | 834.5 us | 494.4 us | 1.69x |
| twitter/yaml             | 2.50 ms  | 9.80 ms  | 0.26x | 1.41 ms  | 3.65 ms  | 0.38x |
| canada/yaml              | 15.29 ms | 52.79 ms | 0.29x | 3.43 ms  | 3.29 ms  | 1.04x |
| citm-catalog/yaml        | 5.21 ms  | 20.85 ms | 0.25x | 1.87 ms  | 3.58 ms  | 0.52x |
| cargo-manifest/yaml      | 36.4 us  | 137.3 us | 0.27x | 19.4 us  | 33.1 us  | 0.59x |
| web-sys-manifest/yaml    | 499.9 us | 1.65 ms  | 0.30x | 201.1 us | 504.5 us | 0.40x |
| cargo-lock/yaml          | 193.9 us | 705.6 us | 0.27x | 135.2 us | 351.4 us | 0.38x |
| saphyr/yaml              | 2.30 ms  | 5.73 ms  | 0.40x | 1.38 ms  | 5.14 ms  | 0.27x |
| github/yaml              | 5.27 ms  | 20.06 ms | 0.26x | 3.90 ms  | 12.24 ms | 0.32x |
| kubernetes/yaml          | 18.23 ms | 59.07 ms | 0.31x | 10.74 ms | 21.85 ms | 0.49x |
| manifests/yaml           | 3.01 ms  | 14.04 ms | 0.21x | 1.75 ms  | 3.73 ms  | 0.47x |
| logs/yaml                | 14.83 ms | 58.73 ms | 0.25x | 10.04 ms | 17.94 ms | 0.56x |
| features/yaml            | 15.67 ms | 50.35 ms | 0.31x | 3.69 ms  | 3.94 ms  | 0.94x |
| point-cloud/yaml         | 10.55 ms | 36.66 ms | 0.29x | 2.61 ms  | 2.77 ms  | 0.94x |
| registry/yaml            | 4.30 ms  | 15.29 ms | 0.28x | 1.31 ms  | 2.35 ms  | 0.56x |
| tree/yaml                | 17.45 ms | 71.28 ms | 0.24x | 4.81 ms  | 7.97 ms  | 0.60x |
| twitter/toml             | 818.0 us | 1.73 ms  | 0.47x | 1.16 ms  | 1.03 ms  | 1.12x |
| canada/toml              | 4.60 ms  | 10.25 ms | 0.45x | 3.82 ms  | 3.68 ms  | 1.04x |
| citm-catalog/toml        | 1.97 ms  | 4.16 ms  | 0.47x | 2.18 ms  | 2.66 ms  | 0.82x |
| cargo-manifest/toml      | 15.8 us  | 20.0 us  | 0.79x | 13.9 us  | 12.4 us  | 1.12x |
| web-sys-manifest/toml    | 304.3 us | 442.3 us | 0.69x | 208.4 us | 140.1 us | 1.49x |
| cargo-lock/toml          | 75.8 us  | 143.5 us | 0.53x | 113.6 us | 115.0 us | 0.99x |
| saphyr/toml              | 625.9 us | 1.45 ms  | 0.43x | 1.71 ms  | 1.75 ms  | 0.98x |
| github/toml              | 2.28 ms  | 4.34 ms  | 0.53x | 3.69 ms  | 3.34 ms  | 1.11x |
| kubernetes/toml          | 9.68 ms  | 20.22 ms | 0.48x | 12.57 ms | 13.37 ms | 0.94x |
| manifests/toml           | 1.32 ms  | 2.77 ms  | 0.48x | 1.49 ms  | 2.07 ms  | 0.72x |
| logs/toml                | 6.11 ms  | 11.75 ms | 0.52x | 7.15 ms  | 6.37 ms  | 1.12x |
| features/toml            | 5.42 ms  | 12.11 ms | 0.45x | 4.05 ms  | 5.35 ms  | 0.76x |
| point-cloud/toml         | 3.38 ms  | 8.55 ms  | 0.40x | 2.74 ms  | 3.22 ms  | 0.85x |
| registry/toml            | 1.92 ms  | 3.54 ms  | 0.54x | 1.36 ms  | 1.46 ms  | 0.94x |
| tree/toml                | 6.12 ms  | 13.92 ms | 0.44x | 5.95 ms  | 8.03 ms  | 0.74x |
| table/csv                | 6.21 ms  | 3.60 ms  | 1.73x | 5.32 ms  | 1.70 ms  | 3.12x |

`blobs` has no serde counterpart (serde has no bytes for `Vec<u8>`):
de/ser take 689/512 us in JSON, 515/179 us in CBOR, 462/175 us in
MessagePack, 4.53/1.26 ms in YAML and 1.16/2.02 ms in TOML.  Ignoring the
Twitter JSON document (`twitter/json/ignore`) takes 260 us, 1.02x of
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
