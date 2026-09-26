# Runtime Performance

This folder contains the same code for serde, deser and miniserde to compare the
deserialization of a JSON dump from Twitter.

Current results from running `make bench`:

```
test bench_deserialize_deser_json ... bench:     385,121.88 ns/iter (+/- 8,043.67)
test bench_deserialize_miniserde  ... bench:     419,984.38 ns/iter (+/- 7,978.88)
test bench_deserialize_serdejson  ... bench:     382,646.61 ns/iter (+/- 8,606.28)
test bench_serialize_deser_json   ... bench:     211,954.25 ns/iter (+/- 2,744.81)
test bench_serialize_miniserde    ... bench:     296,743.49 ns/iter (+/- 5,854.23)
test bench_serialize_serdejson    ... bench:     206,627.87 ns/iter (+/- 2,901.65)
```

## Benchmark Binary

The benchmark binary covers more than the Twitter dump.  Synthetic datasets
(see `src/datasets.rs`) stress what the dump barely contains, each
serialized and deserialized with JSON and CBOR:

* `features`: f64 heavy (GeoJSON-like coordinates)
* `point-cloud`: f32 heavy
* `blobs`: many small byte buffers, plain and `BytesFallback<Hex>`
* `registry`: many small `HashMap`s
* `tree`: deeply nested small containers

Usage:

* `cargo run --release -- time [FILTER [ROUNDS]]` times all benchmarks (or
  those whose name contains `FILTER`).  The benchmarks run in interleaved
  rounds (default 5) and the best time is reported, which keeps the noise
  between runs mostly below 1%.  Running without arguments is the same as
  `time`.
* `cargo run --release -- compare BASE NEW` compares two saved outputs of
  `time`:

  ```
  cargo run --release -- time > /tmp/base.txt
  # make changes
  cargo run --release -- time > /tmp/new.txt
  cargo run --release -- compare /tmp/base.txt /tmp/new.txt
  ```

* `cargo run --release -- loop NAME [N]` runs a single benchmark in a loop
  for profiling (for instance `loop tree/cbor/ser 1000`).  `de`, `ser`,
  `ignore`, `de-serde` and `ser-serde` are shortcuts for the Twitter JSON
  benchmarks (`cargo run --release -- de 1000`).
* With the `count-allocs` feature, `allocs [FILTER]` counts the allocations
  of a single run of every benchmark.
