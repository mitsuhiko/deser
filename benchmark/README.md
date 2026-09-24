# Runtime Performance

This folder contains the same code for serde, deser and miniserde to compare the
deserialization of a JSON dump from Twitter.

Current results from running `make bench`:

```
test bench_deserialize_deser_json ... bench:     540,554 ns/iter (+/- 32,365)
test bench_deserialize_miniserde  ... bench:     416,958 ns/iter (+/- 23,882)
test bench_deserialize_serdejson  ... bench:     378,004 ns/iter (+/- 11,273)
test bench_serialize_deser_json   ... bench:     378,483 ns/iter (+/- 8,634)
test bench_serialize_miniserde    ... bench:     288,004 ns/iter (+/- 17,721)
test bench_serialize_serdejson    ... bench:     202,206 ns/iter (+/- 13,510)
```
