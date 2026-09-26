# Compile Times

This folder contains the same code for serde, deser and miniserde to compare the
impact on compile times.  All use JSON and deriving for a comparison.

Current results:

```
serde
  check
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.43s
  check again
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
  build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.83s
  build --release
    Finished `release` profile [optimized] target(s) in 4.00s

miniserde
  check
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.64s
  check again
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
  build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.05s
  build --release
    Finished `release` profile [optimized] target(s) in 2.82s

deser
  check
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.82s
  check again
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
  build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.10s
  build --release
    Finished `release` profile [optimized] target(s) in 3.31s
```
