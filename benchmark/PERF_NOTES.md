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

6. **Revisit sink storage only with a profile-backed design.**  The existing
   block cache already avoids most global allocations.  Earlier profiling
   attributed about 6% of tree and Canada to taking/returning blocks,
   including two thread-local lookups on macOS.  Experiments storing sinks
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
