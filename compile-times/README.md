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
| serde     | 2.74s | 2.98s | 2.91s           |
| miniserde | 2.01s | 2.10s | 2.28s           |
| deser     | 2.37s | 2.77s | 2.75s           |

A library with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code.  It
is a library, in a binary only the code that is used would be compiled.

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.38s | 0.46s | 8.55s           |
| miniserde | 0.16s | 0.21s | 1.67s           |
| deser     | 0.38s | 0.46s | 5.28s           |

* Clean builds are 0.4s-0.7s slower than with miniserde.  The crates of
  the data formats only depend on `deser-core` (everything but the derive
  macros), so `deser-core` and `deser-json` are compiled while `syn` and
  `deser-derive` are.  The critical path is `syn`, `deser-derive` (0.8s
  to 0.9s, miniserde's derive takes 0.15s), `deser` (which re-exports the
  derive macros) and the program.
* Release builds of derived code are 1.6 times as fast as with serde but
  still three times slower than with miniserde (deser 0.8 from 2023 took
  3.1s, with far fewer features).  deser generates 296k lines of LLVM IR
  (`cargo llvm-lines`) for the 100 types, most of it is deserialization.
  Everything that does not depend on the types of the fields is in
  `deser-core`: the key handling and updates of structs, the sinks of
  unit enums and the default methods of `Sink`.  Where this costs
  runtime performance it's not done (for instance the sinks of fields
  are created inline).

## Areas of Interest

* **`deser-derive` itself** is on the critical path of clean builds.  A
  third of its code is iterator adapters (`map`, `filter`, `collect`)
  which are instantiated for every closure, a third the `quote!`
  templates.
* **Fast paths** are the largest parts of the derived code that remain.
  Removing the atom shortcuts of structs (`__private_value_atom`) builds
  0.26s faster but deserializes structs 5%-11% slower, emitting plain
  fields out of line (`emit_plain_fields`) builds 0.3s faster but
  serializes structs 3%-8% slower.
* **Updates** (`deserialize_update`) cost 0.35s for the 100 types even if
  they are not used, as every struct implements them.
* **Every type** costs something even if its derived code is small: its
  sink is boxed, dropped and has a vtable, and each field type is
  instantiated for the generic helpers of the derive.  The unit enums
  alone take 0.4s.

## Running

`make bench-compile-times` runs `bench.sh` which prints the results of all
three libraries.  The generated libraries remain in `target/many` to
inspect them, for instance with `cargo llvm-lines --release --lib`.
