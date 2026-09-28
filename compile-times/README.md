# Compile Times

This folder compares the compile times of serde, miniserde and deser with
JSON and derived `Serialize` and `Deserialize`.  The results are from
`make bench-compile-times` on an Apple M5 Max with Rust 1.98, the best of
three runs.

## Where deser Stands

Clean builds of a small program with one struct and one enum
(`LIB-version`), including all dependencies.  They are not incremental:
deser is used through path dependencies which cargo would otherwise
compile incrementally (unlike crates from crates.io), which made
`deser-derive` about 0.2s slower.

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 2.59s | 2.72s | 2.83s           |
| miniserde | 1.86s | 1.99s | 2.18s           |
| deser     | 2.55s | 2.60s | 2.62s           |

A library with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code.  It
is a library, in a binary only the code that is used would be compiled.

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.34s | 0.41s | 7.85s           |
| miniserde | 0.15s | 0.19s | 1.52s           |
| deser     | 0.29s | 0.37s | 3.72s           |

* Clean builds are 0.4s-0.7s slower than with miniserde.  The crates of
  the data formats only depend on `deser-core` (everything but the derive
  macros), so `deser-core` and `deser-json` are compiled while `syn` and
  `deser-derive` are.  The critical path is `syn`, `deser-derive` (0.9s
  to 1.0s, miniserde's derive takes 0.15s), `deser` (which re-exports the
  derive macros) and the program.
* Release builds of derived code are twice as fast as with serde but
  still 2.4 times slower than with miniserde (deser 0.8 from 2023 took
  3.1s, with far fewer features).  deser generates 230k lines of LLVM IR
  (`cargo llvm-lines`) for the 100 types (serde 411k, miniserde 128k).
  The frontend (`check`) spends most of its time type and borrow checking
  the derived code, the expanded library has 48k lines (serde 71k,
  miniserde 22k).
* Everything that does not depend on the types of the fields is in
  `deser-core`.  All structs without flattened fields share one sink
  (`StructSink`) which holds the fields in the same block, the derive
  only implements `StructFields` (the sinks of the fields by index, atoms
  by index and `finish`).  This costs an indirect call per field, which
  makes deserializing structs 2%-3% slower than with a sink per struct
  (up to 6% for Twitter in MessagePack) but made the derived code a
  quarter smaller and release builds 1.4 times as fast.  Unit enums share
  one sink too.  Plain fields are emitted by a helper that exists once
  per type of field (`emit_plain_field`), emitting them through trait
  objects instead makes serializing structs 6% slower.

## Areas of Interest

* **`deser-derive` itself** is on the critical path of clean builds.  It
  uses loops instead of iterator adapters (`map`, `filter`, `collect`)
  which are instantiated for every closure: that took it from 153k to
  101k lines of LLVM IR (serde_derive 122k).  Its templates have more
  tokens than serde_derive's (8.7k against 6.2k `quote!` pushes).
  Structs with flattened fields still use the old template with a sink
  per struct, `derive_struct` builds it before it knows whether it's
  needed.
* **`finish`** of derived structs is the largest function of the
  derived code (17% of the IR, about 50 lines per field).  Taking the
  values with helpers, checking the required fields by reference first or
  computing the missing fields in a separate function all end up with
  about the same IR once the helpers are inlined.
* **Unit enums** take 18% of the IR (410 lines for three variants), about
  half of it serialization which is not generic.
* **Updates** (`deserialize_update`) are implemented by every struct even
  if they are not used, as `UpdateFields` (1.4% of the IR).
* **Every type** costs something even if its derived code is small: its
  fields are boxed, dropped and have a vtable, and each field type is
  instantiated for the generic helpers of the derive.

## Running

`make bench-compile-times` runs `bench.sh` which prints the results of all
three libraries.  The generated libraries remain in `target/many` to
inspect them, for instance with `cargo llvm-lines --release --lib`.
