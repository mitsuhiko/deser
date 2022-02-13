# Compile Times

This folder contains the same code for serde and deser to compare the impact on
compile times.  Both use JSON and deriving for a comparison.  It also compares it
against miniserde, but the miniserde example does not use flattening or automatic
field renaming as this is not supported.

Current results:

```
serde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 5.78s
  check again
    Finished dev [unoptimized + debuginfo] target(s) in 0.11s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 6.14s
  build --release
    Finished release [optimized] target(s) in 6.65s

miniserde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 2.74s
  check again
    Finished dev [unoptimized + debuginfo] target(s) in 0.08s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 2.96s
  build --release
    Finished release [optimized] target(s) in 3.11s

deser
  check
    Finished dev [unoptimized + debuginfo] target(s) in 3.82s
  check again
    Finished dev [unoptimized + debuginfo] target(s) in 0.09s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 3.72s
  build --release
    Finished release [optimized] target(s) in 3.63s
```
