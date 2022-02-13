# Compile Times

This folder contains the same code for serde and deser to compare the impact on
compile times.  Both use JSON and deriving for a comparison.

Current results:

```
Serde
  check
    Finished dev [unoptimized + debuginfo] target(s) in 6.07s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 6.63s
  build --release
    Finished release [optimized] target(s) in 7.06s
Deser
  check
    Finished dev [unoptimized + debuginfo] target(s) in 4.05s
  build
    Finished dev [unoptimized + debuginfo] target(s) in 3.94s
  build --release
    Finished release [optimized] target(s) in 3.86s
```
