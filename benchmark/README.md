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

For profiling, the benchmark binary runs individual parts in a loop
(`cargo run --release -- de 1000`, with `de`, `ser`, `ignore`, `de-serde` or
`ser-serde`) and prints comparative timings when run without arguments.  The
`count-allocs` feature adds an `allocs` mode which counts allocations.
