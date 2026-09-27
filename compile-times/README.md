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
| serde     | 2.62s | 2.78s | 3.18s           |
| miniserde | 1.91s | 2.15s | 2.36s           |
| deser     | 2.96s | 3.44s | 3.49s           |

A program with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code:

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.33s | 0.45s | 8.12s           |
| miniserde | 0.15s | 0.24s | 1.59s           |
| deser     | 0.34s | 0.47s | 6.72s           |

* Clean builds are slower than with serde (0.3s-0.7s), they are dominated
  by the dependencies.
* Debug builds of derived code cost the same as with serde.
* Release builds of derived code are 17% faster than with serde, but four
  times slower than with miniserde.  deser does not generate less code
  than serde (505k lines of LLVM IR according to `cargo llvm-lines`,
  serde 410k, miniserde 128k) and the binary is larger (2.2 MiB, serde
  1.5 MiB, miniserde 0.9 MiB), the code is just cheaper to optimize as
  it's less generic.

## Areas of Interest

* **The derive macro is on the critical path of clean builds.**  `deser`
  re-exports the derive macros, so it only starts compiling after `syn`
  and `deser-derive` are done: syn, deser-derive, deser (1.3s in debug
  builds), deser-json and the program are compiled one after the other.
  serde avoids this with `serde_core` which compiles in parallel with the
  proc macro.  A core crate without the macros would take an estimated
  1s off a clean debug build.
* **Default methods of `Sink` are compiled for every sink.**  A quarter
  of the LLVM IR of the 100 types comes from default methods that are
  instantiated for every sink type because they are in its vtable:
  `unexpected_atom` (180 lines, for three sinks per type, 10% of the
  total), the `__private_*_atom` shortcuts (70 lines each, for the key
  and enum sinks) and `map`/`seq` (55 lines each).  Only small parts of
  them depend on the sink, moving the rest into shared non-generic
  functions would shrink the code of every derived type.
* **The derived `finish` and `value_for_key`** are the largest functions
  that are unique to a type (about 380 lines each for eight fields).

## Running

`make bench-compile-times` runs `bench.sh` which prints the results of all
three libraries.  The generated programs remain in `target/many` to
inspect them, for instance with `cargo llvm-lines --release`.
