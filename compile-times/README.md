# Compile Times and Binary Sizes

This folder compares the compile times and binary sizes of serde,
miniserde and deser with JSON and derived `Serialize` and `Deserialize`.
The results are from `make bench-compile-times` on an Apple M5 Max with
Rust 1.98, compile times are the best of three runs.

## Where deser Stands

Clean builds of a small program with one struct and one enum
(`LIB-version`), including all dependencies.  They are not incremental:
deser is used through path dependencies which cargo would otherwise
compile incrementally (unlike crates from crates.io), which made
`deser-derive` about 0.2s slower.

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 2.67s | 2.69s | 3.02s           |
| miniserde | 1.91s | 2.05s | 2.12s           |
| deser     | 2.22s | 2.36s | 2.56s           |

A library with 100 structs (eight fields, one of them nested) and 100
enums which are all read and written as JSON, without the dependencies
(generated into `target/many`).  This is the cost of the derived code.  It
is a library, in a binary only the code that is used would be compiled.

| library   | check | build | build --release |
|-----------|-------|-------|-----------------|
| serde     | 0.34s | 0.42s | 7.85s           |
| miniserde | 0.15s | 0.19s | 1.53s           |
| deser     | 0.38s | 0.48s | 3.63s           |

* Clean builds are 0.3s-0.4s slower than with miniserde.  The crates of the
  data formats only depend on `deser-core` (everything but the derive
  macros), so `deser-core` and `deser-json` are compiled while `syn` and
  `deser-derive` are.  The critical path is `syn`, `deser-derive` (0.9s,
  miniserde's derive takes 0.15s), `deser` (which re-exports the
  derive macros) and the program.
* Release builds of derived code are 2.2 times as fast as with serde
  but 2.4 times slower than with miniserde (deser 0.8 from 2023 took
  3.1s, with far fewer features).  deser generates 240k lines of LLVM IR
  (`cargo llvm-lines`) for the 100 types (serde 411k, miniserde 128k).
  The frontend (`check`) spends most of its time type and borrow checking
  the derived code, the expanded library has 61k lines (serde 71k,
  miniserde 22k, formatted like `cargo expand`).
* Multimaps (fields that are collections collect repeated keys) made
  the derived code larger: every field got a branch for collecting in
  the sinks of the fields, atoms and the updates, and `finish` fills in
  the empty collections of missing keys.  That took the IR from 200k to
  254k lines, the expanded library from 43k to 76k lines and release
  builds from 3.4s to 4.3s, `check` from 0.3s to 0.4s.  Since fields are
  deserialized by their slots (see below) the branches exist once per
  type of field and are removed for types that never collect.
* Everything that does not depend on the types of the fields is in
  `deser-core`.  All structs without flattened fields share one sink
  (`StructSink`) which holds the fields in the same block.  What depends
  on the type of a field (its sink, atoms, borrowed atoms and if it
  collects) is done by its `Slot<T, A>` which implements `FieldSlot` once
  per type of field and adapter, not once per struct.  The derive only
  implements `StructFields`: the slot of a field by index and `finish`.
  The shared sink costs an indirect call per field, which makes
  deserializing structs 2%-3% slower than with a sink per struct (up to
  6% for Twitter in MessagePack) but made the derived code a quarter
  smaller and release builds 1.4 times as fast.  The slots cost another
  one: deserializing is as fast overall and 2%-4% slower for Twitter,
  but the IR is 17% smaller (from 288k lines), the expanded library 25%
  (from 82k lines), release builds 22% faster (from 4.6s) and the binary
  with 100 types 16% smaller.  Plain fields are emitted by a helper that
  exists once per type of field (`emit_plain_field`), emitting them
  through trait objects instead makes serializing structs 6% slower.
* Unit enums only generate the lookup of their names, two functions
  that set a variant by index and a function that returns the index of a
  variant, with constant tables of the names (`UnitEnum`,
  `UnitVariants`).  Everything else is in `deser-core`: 220 instead of
  520 lines of IR for three variants, 60 of them `emit_plain_field`.
  Looking up the name of an atom (`unit_enum_set`) is not inlined, it
  would otherwise exist for every unit enum.
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
  derived code (25% of the IR, about 75 lines per field), it builds the
  struct from the slots.  Taking the values with helpers, checking the
  required fields by reference first or computing the missing fields in
  a separate function all end up with about the same IR once the helpers
  are inlined.
* **Unit enums** are not plain (see `PlainSink`) although they
  serialize as an atom, `emit_plain_field` exists for them without ever
  emitting anything.  Matching their names takes about 15 lines per name.
* **Updates** (`deserialize_update`) are implemented by every struct even
  if they are not used, as `UpdateFields` (2.2% of the IR).  They could
  use slots like the fields of `StructFields`, but they update the
  fields of the struct in place (`&mut T`, not a `Slot`).
* **Every type** costs something even if its derived code is small: its
  fields are boxed, dropped and have a vtable, and each field type is
  instantiated for the generic helpers of the derive.

## Binary Sizes

The stripped binaries of the small program and of a program with the
100 structs and enums of the library above, each of which is read and
written as JSON (the library with a `main`).  They are built with the
default release profile and a profile optimized for size (`lto = "fat"`,
`codegen-units = 1`, `opt-level = "s"`, `panic = "abort"`).  In
parentheses is how much larger they are than hello world (in KiB).
`deser (speedups)` enables the `speedups` feature of `deser-json`.

| program                     | release          | size optimized  |
|-----------------------------|------------------|-----------------|
| hello world                 | 334 KiB          | 279 KiB         |
| serde                       | 418 KiB (+84)    | 311 KiB (+32)   |
| miniserde                   | 383 KiB (+49)    | 295 KiB (+16)   |
| deser                       | 619 KiB (+285)   | 425 KiB (+145)  |
| deser (speedups)            | 586 KiB (+252)   | 409 KiB (+129)  |
| serde, 100 types            | 1226 KiB (+892)  | 860 KiB (+581)  |
| miniserde, 100 types        | 644 KiB (+310)   | 491 KiB (+211)  |
| deser, 100 types            | 979 KiB (+645)   | 687 KiB (+408)  |
| deser (speedups), 100 types | 963 KiB (+629)   | 655 KiB (+375)  |

* The fixed cost of deser is high: the small program is 200 KiB larger
  than with serde (110 KiB optimized for size).  The derived code of
  the small program is only 3.5 KiB, the rest is the runtime (the
  drivers, the JSON reader and writer, errors, extension values like big
  integers and base64 bytes) and the parts of the standard library it
  uses.
* A type costs less than with serde: 3.6 KiB per struct and enum
  against 8.1 KiB with serde (2.6 KiB against 5.5 KiB optimized for
  size, miniserde 2.6 KiB and 2 KiB).  With 100 types deser is 20%
  smaller than serde.  It was 6.5 KiB (3.9 KiB): the fields are
  deserialized by slots that exist once per type of field (see above),
  functions like `from_str` only create the sink of the value for every
  type (`deserialize_value`) and the helpers that serialize, describe
  and look up unit enums are not inlined into every type.
* The serialize driver is specialized for every writer it drives (the
  writers' event handlers are inlined into it for speed), every writer a
  program can reach costs a copy of it.  The formats keep the pausable
  drivers of the stream serializers out of `to_string` and `to_vec`
  (`drive_whole` and friends): that was 49 KiB in the small program,
  another 16 KiB was the pretty printer of JSON that constant compact
  configurations do not refer to anymore.  Layers of the deserializer
  are only linked into programs that add layers (16 KiB).
* **Floats** without `speedups` are formatted with the standard
  library's `{:e}` and parsed again to pick the even digits of ties
  (`format_finite`).  This links the float formatting and parsing of
  the standard library, with `zmij` the program is 32 KiB smaller
  (16 KiB optimized for size).
* **Profiles** matter more than with serde: on their own
  `codegen-units = 1` and `panic = "abort"` each make the small program
  64 KiB smaller, serde does not change.  Functions that are `#[inline]`
  (and drop glue) are copied into every codegen unit that uses them
  (`Error` is dropped by ten copies of its drop glue) and the drivers
  hold values that have to be dropped when unwinding.

## Running

`make bench-compile-times` runs `bench.sh` which prints the results of all
three libraries, `make bench-binary-sizes` only measures the binary sizes
(`./bench.sh compile` only the compile times).  The generated libraries
remain in `target/many` to inspect them, for instance with
`cargo llvm-lines --release --lib`, the programs of the binary sizes in
`target/size` (for instance for `cargo bloat`).
