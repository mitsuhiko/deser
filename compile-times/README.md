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
| serde     | 2.63s | 2.80s | 2.83s           |
| miniserde | 1.94s | 2.05s | 2.19s           |
| deser     | 2.27s | 2.41s | 2.63s           |

A library with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code.  It
is a library, in a binary only the code that is used would be compiled.

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.34s | 0.42s | 8.04s           |
| miniserde | 0.16s | 0.20s | 1.55s           |
| deser     | 0.39s | 0.50s | 4.56s           |

* Clean builds are 0.3s-0.4s slower than with miniserde.  The crates of the
  data formats only depend on `deser-core` (everything but the derive
  macros), so `deser-core` and `deser-json` are compiled while `syn` and
  `deser-derive` are.  The critical path is `syn`, `deser-derive` (0.9s,
  miniserde's derive takes 0.15s), `deser` (which re-exports the
  derive macros) and the program.
* Release builds of derived code are 1.8 times as fast as with serde
  but 2.9 times slower than with miniserde (deser 0.8 from 2023 took
  3.1s, with far fewer features).  deser generates 286k lines of LLVM IR
  (`cargo llvm-lines`) for the 100 types (serde 411k, miniserde 128k).
  The frontend (`check`) spends most of its time type and borrow checking
  the derived code, the expanded library has 82k lines (serde 71k,
  miniserde 22k, formatted like `cargo expand`).
* Multimaps (fields that are collections collect repeated keys) made
  the derived code larger: every field got a branch for collecting in
  `field_sink`, `field_atom`, `field_borrowed_atom` and the updates, and
  `finish` fills in the empty collections of missing keys.  That took
  the IR from 200k to 254k lines, the expanded library from 43k to 76k
  lines and release builds from 3.4s to 4.3s, `check` from 0.3s to
  0.4s.  The branches are generated for fields of all types although
  most types never collect (`__private_collects()` is `false`).
* Everything that does not depend on the types of the fields is in
  `deser-core`.  All structs without flattened fields share one sink
  (`StructSink`) which holds the fields in the same block, the derive
  only implements `StructFields` (the sinks of the fields by index, atoms
  by index and `finish`).  This costs an indirect call per field, which
  makes deserializing structs 2%-3% slower than with a sink per struct
  (up to 6% for Twitter in MessagePack) but made the derived code a
  quarter smaller and release builds 1.4 times as fast.  Plain fields are
  emitted by a helper that exists once per type of field
  (`emit_plain_field`), emitting them through trait objects instead makes
  serializing structs 6% slower.
* Unit enums only generate the lookup of their names, two functions
  that set a variant by index and a function that returns the index of a
  variant, with constant tables of the names (`UnitEnum`,
  `UnitVariants`).  Everything else is in `deser-core`: 220 instead of
  520 lines of IR for three variants, 60 of them `emit_plain_field`.
  Helpers in `deser-core` that are `#[inline]` are inlined into the
  derived code before LLVM sees it (MIR inlining) if they are small, so
  those that are called from every type are not `#[inline]`.

## Areas of Interest

* **`deser-derive` itself** is on the critical path of clean builds.  It
  uses loops instead of iterator adapters (`map`, `filter`, `collect`)
  which are instantiated for every closure: that took it from 153k to
  101k lines of LLVM IR (105k with multimaps, serde_derive 122k).  Its
  templates have more tokens than serde_derive's (8.7k against 6.2k
  `quote!` pushes).
  Structs with flattened fields still use the old template with a sink
  per struct, `derive_struct` builds it before it knows whether it's
  needed.
* **`finish`** of derived structs is the largest function of the
  derived code (19% of the IR, about 70 lines per field).  Next are
  `field_atom` (11%) and `field_sink` (8%), both of which grew with the
  multimap branches.  Taking the values with helpers, checking the
  required fields by reference first or computing the missing fields in
  a separate function all end up with about the same IR once the helpers
  are inlined.
* **Unit enums** are not plain (see `PlainSink`) although they
  serialize as an atom, `emit_plain_field` exists for them without ever
  emitting anything.  Matching their names takes about 15 lines per name.
* **Updates** (`deserialize_update`) are implemented by every struct even
  if they are not used, as `UpdateFields` (2.2% of the IR).
* **Every type** costs something even if its derived code is small: its
  fields are boxed, dropped and have a vtable, and each field type is
  instantiated for the generic helpers of the derive.

## Running

`make bench-compile-times` runs `bench.sh` which prints the results of all
three libraries.  The generated libraries remain in `target/many` to
inspect them, for instance with `cargo llvm-lines --release --lib`.
