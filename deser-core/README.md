# deser-core

The core of [deser](https://github.com/mitsuhiko/deser).

This crate is an implementation detail of deser, use the
[`deser`](https://crates.io/crates/deser) crate instead which re-exports
everything in here together with the derive macros:

```sh
cargo add deser --features derive
```

The crates of the data formats depend on this crate so that they can be
compiled without waiting for the derive macros.
