# Runtime Performance

This folder contains the same code for serde, deser and miniserde to compare the
deserialization of a JSON dump from Twitter.

Current results from running `make bench`:

```
test bench_deserialize_deser_json ... bench:     556,539 ns/iter (+/- 23,576)
test bench_deserialize_miniserde  ... bench:     446,347 ns/iter (+/- 27,251)
test bench_deserialize_serdejson  ... bench:     399,132 ns/iter (+/- 12,660)
test bench_serialize_deser_json   ... bench:     376,043 ns/iter (+/- 5,289)
test bench_serialize_miniserde    ... bench:     307,832 ns/iter (+/- 3,686)
test bench_serialize_serdejson    ... bench:     214,850 ns/iter (+/- 3,782)
```
