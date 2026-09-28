# Performance Notes

Working measurements and optimization ideas, not published benchmark results.

## MessagePack Deserialization

### Findings

Reproduced on an Apple M5 Max with Rust 1.98.  The initial run matched the
README: Canada was 2.73x slower than rmp-serde, features 2.74x, point-cloud
2.41x and tree 2.55x.  Both libraries deserialize the same bytes into the
same types; this is not rmp-serde using array-encoded structs while deser
uses maps.

Sampling `canada/msgpack/de` before the float-sink changes gave roughly:

| work | samples |
|------|---------|
| MessagePack parser itself | 24% |
| atom handling, including sinks | 31% |
| opening containers, including sinks | 29% |
| closing containers, including sinks | 16% |

Canada contains 55,563 coordinate tuples.  Each pair needs a sink, a start
event, two atom events and an end event.  The sink blocks are cached, but
initializing, dispatching, finishing and recycling them still costs time.
The parser and driver also maintain separate container stacks.  rmp-serde's
typed recursive visitor avoids this general-purpose event machinery.

Binary number decoding is cheap, so this overhead is much more visible
than with JSON.  String allocation/copying and map insertion can also mask
it, explaining the much smaller gap on string-heavy datasets.  It is not
primarily an f32/f64 conversion problem: integer pairs show the same
small-container penalty.

### Reproducing and Isolating It

From `benchmark/`:

```sh
cargo run --release -- time msgpack/de 5
cargo run --release -- loop canada/msgpack/de 15000
cargo run --release --example msgpack
```

The example compares 100,000 scalars, flat or grouped into 50,000 pairs.
It checks both libraries' results on identical input and also measures
ignoring the value and feeding the driver directly without a parser.
Representative times with the outlined float sink:

| shape | deser | rmp-serde | deser ignoring the value |
|-------|-------|-----------|--------------------------|
| flat f32 | ~0.56 ms | ~0.34 ms | ~0.41 ms |
| f32 pairs | ~1.06 ms | ~0.43 ms | ~0.79 ms |
| f64 pairs | ~1.06 ms | ~0.43 ms | ~0.84 ms |
| u32 pairs | ~1.15 ms | ~0.42 ms | ~0.82 ms |

Even ignoring pairs, without typed sink allocations or an output vector,
is slower than rmp-serde constructing the result.  Feeding the f32 pair
events directly still takes about 1 ms.  These are separate workloads,
not additive timings to subtract from each other.  Ignoring is NOT a
parser-only benchmark: it still goes through the driver.

Keep baseline binaries and alternate runs.  Later runs in this investigation
had substantial machine-load drift, including changes to the unchanged
serde controls.  Do not treat those larger apparent improvements as wins.
For parser-only measurements, a separate internal `Out` implementation that
counts/discards events would be useful; it should not run the driver.

### Float Sink Improvement Implemented

In `deser-core/src/de/impls.rs`, the float sink combined primitive numbers
with lexical parsing, extension handling and errors in one large function.
The sampled native-float path included a no-op call to `Atom`'s drop glue.
Assembly also showed a large stack frame/register-save sequence.

* Outline lexical/extension/error handling and inline the small numeric path.
* Skip drop glue only after matching one of the four primitive numeric
  variants.  Owning variants are consumed by the outlined function instead.
* Do not mark the outlined function cold: JSON commonly emits `Number`
  extension values.

The outlining experiment improved the float-heavy MessagePack datasets by
about 8% in the initial A/B runs, and helped CBOR as well.  This is a local
improvement, not a solution to the remaining 2x-plus gap.  Float acceptance,
lexical/extension handling, errors, signed zero and NaN have regression tests.

Experiments that did not explain or solve the gap:

* Moving the lexical payload instead of borrowing it did not eliminate the
  scalar drop-glue call or produce a convincing improvement.
* Adding `#[inline]` to the original large float method was not enough.
* Allocation is only part of the problem: the null-sink measurements are
  already expensive without constructing any typed sinks.

### Ideas to Try Next

These are hypotheses, not measured wins.  Prefer small experiments and keep
only improvements that survive repeated comparisons.

1. **Avoid the pending slot for scalar sequence elements.**  `SeqSink` in
   `deser-core/src/de/impls.rs` currently flushes the previous `Option<T>`,
   then decodes the next atom into that slot.  Try decoding atoms into a
   local slot and pushing immediately, retaining the pending-slot path for
   nested containers.  This might let the optimizer eliminate more of the
   slot bookkeeping without changing the public API.  Check adapters,
   error collection and alternating atom/container elements carefully.

2. **Audit other primitive sinks for the same code-generation issue.**
   Integer pairs are still slow.  Inspect integer sink assembly and profile
   `citm-catalog/msgpack/de` and `tree/msgpack/de` before extending the float
   optimization.  A smaller numeric path may help, but wide integers,
   lexical keys, range checking and extension fallbacks must remain intact.

3. **Reduce the cost of starting and ending tiny containers.**  This is
   about 45% of the Canada profile.  Investigate a way for a parent sink to
   accept a small fixed-size sequence without creating, pushing, finishing
   and recycling a separate sink for every pair/triple.  A private optional
   sink capability or a batch interface is worth prototyping.  Preserve a
   general fallback for arbitrary types and incremental input.  This is
   more promising for a large gain than just a faster allocator.

4. **Amortize primitive event dispatch.**  Prototype delivering a run of
   numeric atoms to a sequence sink rather than repeating the complete
   driver entry/dispatch/context-clear cycle for each one.  Measure flat
   vectors separately from pairs: batching just the floats will not remove
   the latter's start/end cost.  Avoid an intermediate allocated event
   buffer.  Compare code size and compile times as well as runtime.

5. **Separate layered and unlayered hot paths more effectively.**  Inspect
   `atom_event`, `start_event` and `end_event` in
   `deser-core/src/de/driver.rs`.  They are deliberately not inlined to
   keep parsers small.  Try narrowly specializing a common sequence path
   rather than inlining the entire driver into every parser.  Layers,
   recovery, input ranges and event data must still behave identically.

6. **Revisit sink storage only with a profile-backed design.**  Sinks are
   allocated in an arena of the state (see `deser-core/src/de/arena.rs`).
   Without any reuse of sink memory deserialization is two to six times
   slower (macOS allocator).  The arena replaced a cache of blocks per
   thread and size class and is on par or faster (geomean -0.9% over all
   deserialization benchmarks, -13% to +3%).  What mattered: the driver
   releases the sinks it's done with (`SinkHandle::release`), so the top
   block is popped right away instead of being marked as dead in its
   footer and popped by the next allocation (that cost up to 8%), the
   root sink is dropped before the state (otherwise the arena is leaked
   and every document allocates a chunk, logs was 50% slower) and the
   chunk of a finished deserialization is parked for the next one.
   Earlier profiling of the per thread cache attributed about 6% of tree
   and Canada to taking/returning blocks, including two thread-local
   lookups on macOS.  Experiments storing sinks
   of up to 32 or 128 bytes inline in handles (then moving them into driver
   blocks while containers are open) regressed 5%-25%: handles are copied
   several times on the way to the driver.  Driving sinks through raw
   pointers instead of handles alone cost 3%-6%.  Do not simply repeat that
   approach.  Any driver-local storage experiment must retain stable
   addresses, reverse drop order and the ability to move an ongoing
   deserialization between threads.

7. **Fast subtree skipping is a separate optimization.**  A parser could
   skip ignored containers without delivering every interior event.  This
   could help unknown fields, but will not fix typed coordinate decoding.
   It must not bypass syntax/UTF-8 validation, limits, layers or observers.

### Validation for Further Changes

* Run the isolated flat/pair/tuple tests and the real MessagePack datasets.
* Check JSON and CBOR too: the sinks and driver are shared, and JSON's
  exact-number extensions exercise a different path from native floats.
* Use tree and integer-heavy data to distinguish numeric wins from container
  wins; use Twitter/logs to check strings and small-document overhead.
* Preserve borrowing, adapters, layers, depth limits, error recovery,
  incremental input and destruction of partially initialized values.
* Run `cargo test --workspace --all-features`.  Include optimized float
  regression tests via
  `cargo test -p deser --test integration --release test_de::test_float`.

## Layout of Atoms and Events

Every value goes through an `Atom` (and usually an `Event`), so how they
are laid out matters as much as their size.  Both are 32 bytes, checked
at compile time together with `Chunk`.

When `Str` and `Lexical` changed from `Cow<str>` to `Text` (a pointer and
a length, two words), numbers in the binary formats became 5-15% slower
although the size stayed the same:

* With `Cow`, the tag of `Atom` was stored in the unused values of the
  capacity of the `Cow`, a full word, and all values started at offset 8.
  `Text` has no such unused values, so the tag became a byte and small
  values (`F32`, `Char`, `Bool`) were placed next to it at offsets 1-4.
* Formats then write an atom with a byte store and a 4 or 8 byte store,
  while `DeserializeDriver::atom_event` copies it with 16 byte loads
  (`ldp q0, q1`).  A load that spans several smaller stores cannot be
  forwarded from them and waits until they reach the cache.  With the
  word sized tag the atom is written with a single `stp`.
* Serializers moved atoms in pieces of different sizes at odd offsets for
  the same reason.

`#[repr(C, u64)]` on `Atom` restores the word sized tag in front of the
values.  This only fits in 32 bytes because no value is larger than 24
bytes: `Implicit` stores the kind of its value in spare bits of the
length of its text (`Slice` reserves bits 60-62 for a tag next to the
owned bit) and the value in one word.  Measured on Apple M5 Max, the
byte sized tag cost up to 15% on `point-cloud/msgpack/ser`, 13% on
`point-cloud/cbor/ser` and 10% on `features/msgpack/de`; with the word
sized tag and packed `Implicit` all groups are within 2% of the `Cow`
based atoms.  The word sized tag alone (with a 32 byte `Implicit` it
would not fit) fixed serialization but left `msgpack/de` 6-10% slower in
an intermediate build; it's not understood why packing `Implicit` also
fixed that.

When changing `Atom`, `Event`, `Text`, `Bytes` or `ExtValue`, check the
layout with `RUSTC_BOOTSTRAP=1 cargo rustc -p deser-core --lib --release
-- -Zprint-type-sizes` and compare `point-cloud`, `canada` and `features`
in MessagePack and CBOR against a baseline binary.

## Derived Structs

All derived structs without flattened fields share one sink
(`StructSink`), the derive implements `StructFields` for a struct with the
slot and the values of the fields.  Compared to a sink per struct this
made the derived code a quarter smaller (see `compile-times`) at the cost
of an indirect call per field (the lookup of keys by function pointer and
the dispatch of values through `StructFields`).  Deserializing is 2%-3%
slower (geometric mean for JSON, CBOR and MessagePack, up to 6% for
Twitter in MessagePack), serializing is unchanged.

Findings while getting there, measured against a baseline binary:

* A separate allocation for the fields cost 4%-8% on small structs (tree,
  blobs), the sink and its fields share one block now (`StructBox`, a
  handle variant with the same representation as boxed sinks).
* Borrowed atoms went through `field_borrowed_atom` which forwarded to
  `field_atom`, structs that cannot borrow (`StructInfo::borrows`) call
  `field_atom` directly.
* Tracking the seen fields in a `u64` with the words of larger structs
  allocated out of line, and not dropping the fields after `finish` (they
  are empty then) brought it from 4%-7% to 2%-3%.
* The key lookup is not the problem: scanning the names in the core
  (instead of calling the function pointer) is slower for large structs,
  and predicting that keys come in the order of the fields gained
  nothing.
* Serializing through trait objects (a generic loop over `field` with a
  plainness check on `dyn Serialize`) was 6% slower, going through the
  driver for every field 10% slower.  `emit_plain_field` keeps the static
  dispatch but exists once per type of field.

## Other Profiling Findings and Experiments

These observations were previously in the benchmark README.  Timings refer
to the runs documented there, not a fresh comparison after every change.

### Serialization Driver

`DATASET/events/ser` only produces the events of a value, without a format.
It took 329 us on Canada versus 611 us for CBOR serialization, and 595 us on
tree versus 802 us for CBOR.  The driver is therefore an important target
for closing the CBOR/MessagePack serialization gap, independently of byte
encoding.  Profile event production separately before optimizing a format's
serializer.

### Untagged Enums

Untagged enums buffer the value and replay it for each attempted variant.
Cargo-manifest exercises this path.  Potential experiments: reject clearly
incompatible shapes before replay, or reduce repeated setup for failed
variants.  Preserve variant order and the existing error behavior.

### JSON Exact Numbers

Exact numbers are enabled by default.  Floats with more than 15 digits or
an exponent are passed as `Number` extension values.  Float sinks read the
numeric value with a single dynamic call; earlier Canada measurements made
this as fast as plain floats.  Recheck this path when changing primitive
sinks rather than assuming extensions are rare or cold.

### JSON Float Rounding

Floats are rounded correctly.  Before, the value was the significand
divided (or multiplied) by a power of ten, which is exact for significands
up to 2^53 and exponents up to 22 but otherwise off by up to a unit in the
last place (`2e-23`, 16-17 digit coordinates).  Significands up to 2^53
still take that path, larger ones (with exponents up to 22) use the
algorithm of Eisel and Lemire with a 45 entry table of powers of five,
everything else parses the text with the standard library.  Canada (all
17 digit coordinates) got about 10% slower, features and point-cloud about
5%.  Tried and slower: checking the division's result with exact 128 bit
or 64 bit integer arithmetic (about 3.5 ns per number even without
branches) and always parsing the text with the standard library (38% on
Canada).  The standard library's parser is not usable directly because it
scans the digits again.

### YAML

YAML was still about six times slower than JSON on the same data.  The
remaining cost was spread over the libyaml-style token machinery (simple
key tracking and the token queue) and large tokens/events moved by value.
Investigate token/event size and copying, not just scalar parsing.

### TOML

Documents are parsed into a tree because tables can be defined out of
order.  This costs one allocation per inline array, tens of thousands for
Canada.  An arena was tried and did not pay off; do not assume fewer
allocations automatically make this faster.

### CSV

The parser finds the fields of a record in one pass and emits from that
record.  Most time was in the drivers and sinks; `table/events/ser` alone
was about 1 ms.  Formatting floats with the standard library was about 8%
of writing, so a faster formatter would only address a small part.

Empty optional numbers used to construct an error message for every field
and throw it away, accounting for about a third of reading.  That was fixed.
Keep successful empty/optional handling out of error construction when
changing lexical conversion or adapters.
