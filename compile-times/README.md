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
| serde     | 3.23s | 3.03s | 3.24s           |
| miniserde | 2.22s | 2.30s | 2.24s           |
| deser     | 2.31s | 2.39s | 2.38s           |

A program with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code:

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.34s | 0.49s | 8.09s           |
| miniserde | 0.15s | 0.24s | 1.58s           |
| deser     | 0.34s | 0.48s | 4.82s           |

* Clean builds are about as fast as with miniserde.  The crates of the
  data formats only depend on `deser-core` (everything but the derive
  macros), so `deser-core` and `deser-json` are compiled while `syn` and
  `deser-derive` are, which leaves `syn` and `deser-derive` as the
  critical path.
* Release builds of derived code are 1.7 times as fast as with serde but
  still three times slower than with miniserde.  deser generates 320k
  lines of LLVM IR (`cargo llvm-lines`) for the 100 types, serde 410k and
  miniserde 128k.  Most of it is deserialization.

## Areas of Interest

* **`deser-derive` itself** takes 0.64s to compile (miniserde's derive
  0.15s) and is on the critical path of clean builds.
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
