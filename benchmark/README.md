# Runtime Performance

This folder contains the same code for serde, deser and miniserde to compare the
deserialization of a JSON dump from Twitter.

Current results from running `make bench`:

```
test bench_deserialize_deser_json ... bench:   1,752,943 ns/iter (+/- 16,093)
test bench_deserialize_miniserde  ... bench:     775,237 ns/iter (+/- 5,328)
test bench_deserialize_serdejson  ... bench:     688,497 ns/iter (+/- 12,663)
test bench_serialize_deser_json   ... bench:   1,311,754 ns/iter (+/- 63,892)
test bench_serialize_miniserde    ... bench:     491,493 ns/iter (+/- 2,428)
test bench_serialize_serdejson    ... bench:     319,705 ns/iter (+/- 2,511)
```
