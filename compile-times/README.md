# Compile Times

This folder compares the compile times of serde, miniserde and deser with
JSON and derived `Serialize` and `Deserialize`.  The results are from
`make bench-compile-times` on an Apple M5 Max with Rust 1.98, the best of
three runs.

## Where deser Stands

Clean builds of a small program with one struct and one enum
(`LIB-version`), including all dependencies:

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 2.44s | 2.83s | 2.96s           |
| miniserde | 1.93s | 2.08s | 2.25s           |
| deser     | 2.93s | 3.38s | 3.50s           |

A program with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code:

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.33s | 0.45s | 8.06s           |
| miniserde | 0.15s | 0.24s | 1.59s           |
| deser     | 0.30s | 0.44s | 4.33s           |

* Clean builds are the slowest of the three (0.5s-1.3s slower than
  serde and miniserde).  This is the order in which the crates are
  compiled, not the amount of code (see below).
* Release builds of derived code are almost twice as fast as with serde
  but still almost three times slower than with miniserde.  deser
  generates 291k lines of LLVM IR (`cargo llvm-lines`) for the 100
  types, serde 410k and miniserde 128k.  Most of it is deserialization,
  a program that only derives `Deserialize` takes 3.0s to build, one that
  only derives `Serialize` 1.5s.

## Areas of Interest

* **The derive macro is on the critical path of clean builds.**  `deser`
  re-exports the derive macros, so it only starts compiling after `syn`
  and `deser-derive` are done: syn, deser-derive, deser (1.2s-1.5s in
  debug builds), deser-json and the program are compiled one after the
  other.  serde avoids this with `serde_core` which compiles in parallel
  with the proc macro.  With a core crate that does not depend on
  `deser-derive` (and the formats depending on it) the clean debug build
  of the small program takes 2.2s, as fast as miniserde.
* **`deser-derive` itself** takes 0.64s to compile (miniserde's derive
  0.15s).  After the core crate it would be the critical path.
* **The atom shortcuts of derived structs** (`__private_value_atom`)
  inline the conversion of every field's type into every struct and are
  the largest part of the derived deserialization that remains.  They
  are worth 5%-11% at runtime for struct heavy data, calling the
  conversions out of line saves little.
* **The plain fields of derived structs** (`emit_plain_fields`) are half
  of the derived serialization code.  Emitting them through a function
  that exists once per type builds 20% faster but serializes structs
  3%-8% slower, so it's not done.

## Running

`make bench-compile-times` runs `bench.sh` which prints the results of all
three libraries.  The generated programs remain in `target/many` to
inspect them, for instance with `cargo llvm-lines --release`.
