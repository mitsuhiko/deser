# Compile Times

This folder contains the same code for serde, deser and miniserde to compare the
impact on compile times.  Both use JSON and deriving for a comparison.

Current results:

```
serde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 5.76s
  check again
    Finished dev [unoptimized + debuginfo] target(s) in 0.10s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 6.26s
  build --release
    Finished release [optimized] target(s) in 7.43s

miniserde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 3.37s
  check again
    Finished dev [unoptimized + debuginfo] target(s) in 0.09s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 3.04s
  build --release
    Finished release [optimized] target(s) in 3.36s

deser
  check
    Finished dev [unoptimized + debuginfo] target(s) in 3.60s
  check again
    Finished dev [unoptimized + debuginfo] target(s) in 0.09s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 3.85s
  build --release
    Finished release [optimized] target(s) in 3.63s
```
