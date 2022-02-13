# Compile Times

This folder contains the same code for serde and deser to compare the impact on
compile times.  Both use JSON and deriving for a comparison.  It also compares it
against miniserde, but the miniserde example does not use flattening or automatic
field renaming as this is not supported.

Current results:

```
serde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 6.02s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 6.36s
  build --release
    Finished release [optimized] target(s) in 7.00s

miniserde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 2.77s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 2.98s
  build --release
    Finished release [optimized] target(s) in 3.30s

deser
  check
    Finished dev [unoptimized + debuginfo] target(s) in 3.61s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 3.94s
  build --release
    Finished release [optimized] target(s) in 3.64s
```
