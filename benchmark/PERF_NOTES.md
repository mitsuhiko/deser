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

### Sequences of Atoms Built Inline

Sequences of a fixed number of numbers or booleans (`[f32; 2]`,
`(u8, u8)`, `[f64; 3]`) that are elements of a sequence (`Vec`,
`VecDeque`, ...) have no sink of their own.  The sink of the sequence
says so when it's started (`Sink::__private_seq`), the driver then turns
its frame into `Container::Inline` while such an element is open and
passes the element's atoms and end to the sink of the sequence
(`__private_inline_atom`, `__private_inline_event`), which builds the
element in its slot with the functions of `InlineSeq` (from
`Deserialize::__private_inline_seq`).  They produce the same values and
errors as the sinks of arrays and tuples, `test_inline_elements` checks
this against the same events with the inlining disabled (including error
collection).  An error in an element gives it a null frame like a failed
sink so recovery is unchanged.  Wrappers that only forward `seq` do not
inline (the default of `__private_seq` returns `false`).

Canada, features and point-cloud got 12%-19% faster in CBOR and
MessagePack (point-cloud MessagePack 6%) and 3.5%-5% in JSON, nothing
else changed.  Derived structs don't do this for their fields (the weight
of tree), that would need code in every derived struct.

### Container Overhead and the Limits of Push

Measured with a scratch example (50,000 `[f32; 2]` in MessagePack, before
the inlining above): rmp-serde 427 us, deser 1059 us.  A hand written
automaton for exactly this type behind one dynamic call per event (the
ideal push design, with a minimal parser) took 578 us, statically
composed typed state machines per type (checking every level for every
event) 715 us, and 1133 us for `Vec<Vec<[u32; 2]>>` where deser took
1285 us.  So dispatching events to the sink on top of a stack is the
right push design and container heavy binary data cannot reach serde
without pulling (which serde's recursive typed code does).  Buffering
events (a tape) and consuming it with typed code doesn't help either,
writing the events costs about as much as dispatching them.

Of the remaining overhead the driver's bookkeeping for a container is
about 6 ns even with null sinks, typed child sinks add about 3.5 ns.
Tried and not worth it:

* Calling the methods of `StructSink` without dynamic dispatch for the
  `Struct` handle variant (all derived structs share it): no change (the
  indirect calls are predicted well).  This is also why opening a
  container with one call instead of `next_value` and `map`/`seq` was not
  pursued further, it would only save one of them.
* Handling integers in range inline in the integer sinks without calling
  the drop glue of the atom (like the float sinks do): under 1%.
* Inlining the event functions of the driver into the parser (again):
  4%-18% slower.

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
   Done for integers, it made no difference (see above).

3. **Reduce the cost of starting and ending tiny containers.**  Done for
   sequences of atoms in sequences (see above).

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
   allocated in an arena of the state (see `deser-core/src/arena.rs`).
   Without any reuse of sink memory deserialization is two to six times
   slower (macOS allocator).  The arena replaced a cache of blocks per
   thread and size class.  Together with the emitters of serializations
   in the arena (which had no cache at all), the driver stacks that are
   kept with the arena and recordings of single events without an
   allocation, deserializing is 1.5% faster (geomean, -8% to +2%) and
   serializing 1.3% faster (-12% to +4%, the events benchmarks vary by up
   to 7% with inlining).  What mattered: the driver
   releases the sinks it's done with (`SinkHandle::release`), so the top
   block is popped right away instead of being marked as dead in its
   footer and popped by the next allocation (that cost up to 8%), the
   root sink is dropped before the state (otherwise the arena is orphaned
   and every document allocates a chunk, logs was 50% slower), the
   chunk of a finished deserialization is parked for the next one and
   the state stays small (nested replays move it, keeping the buffers of
   the driver stacks in the state made small documents 2% slower, they
   are in the first chunk).
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
at compile time together with `Emit`.

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

### JSON Digits

Numbers are parsed eight digits at a time (the digits are checked and
combined with a few multiplications in a `u64`) while the significand
cannot overflow, the rest one by one.  Canada got 18% faster, features
13%, others did not change.

`Cursor::digits` also takes runs of fewer than eight digits from the
word: the first byte that is not a digit is found with the trailing
zeros of the mask, the digits are shifted to the top of the word and
combined like eight.  Only the digits at the end of the input (less than
a word left) are parsed one by one.  Parsing alone (without the driver)
got 16%-19% faster on floats with 5-9 digits (Canada as `f32`), the
instructions of `canada`, `features` and `point-cloud` went down by 10%
to 13%.  Deserializing is only faster for point-cloud (7%): for the
others the parser was not the bottleneck.  Eight digits and no digits
are separate branches: when the position after the digits was computed
from the count (a data dependency instead of a predicted branch) nine
digit integers (citm-catalog) were 8% slower to parse.

### JSON Strings

`Cursor::parse_str` handles strings without escapes (the string is a
slice of the input) inlined into the parser, strings with escapes and
incomplete strings are handled out of line by `parse_str_slow` (which
scans the string again).  When everything was out of line, about a third
of its samples were the call, saving registers and returning the result
in memory.  Parsing alone took 17%-20% fewer instructions and time on
Twitter and citm-catalog, deserializing got 4%-12% faster for all
datasets except the ones that are mostly numbers.

`skip_to_escape` checks the first 32 bytes a word at a time (inlined),
longer strings continue out of line with SIMD (NEON or SSE2): two blocks
of 16 bytes, then 64 bytes at once until one has a byte that needs
escaping.  For Twitter and Kubernetes (43% and 72% the contents of
strings, mostly of 32 bytes and more) SIMD made no difference in time.
It matters for really long strings: the base64 images of
session-anthropic (24.5 MB in 179 strings) are parsed 38% faster,
deserializing is 22% faster.  With only two words inlined Twitter got
6% slower to parse, without the blocks of 16 bytes GitHub 2%-3%.

Strings with escapes (`parse_str_slow`) are what session-openai is made
of: 16 MB of its 19.8 MB are in strings with escapes (code, diffs, JSON
in strings), 460,000 escapes with a median of 13 bytes between them.
What helped (parsing alone, in order):

* copying the pieces between escapes with `extend` (word sized copies)
  instead of `extend_from_slice` which calls `memcpy`: 11%
* finding the escapes with `EscapeScanner`, which computes a bit for each
  of 64 bytes once and takes the following escapes of the block from the
  bits: 6%
* unescaping the escapes that stand for a byte with a table before
  calling `parse_escape`: 1%-2%

Copying 16 bytes at a time into the buffer while checking them (without
scanning first) was 3% slower.

The buffer for the unescaped strings is kept with the state (see
`State::__private_take_scratch`, a buffer of the arena which is parked
with its first chunk) between values and deserializations, up to 256 KiB.
Growing it for every line of a JSON Lines file cost 8% of deserializing
session-openai and 6% of session-anthropic.  Together session-openai
deserializes 16% faster, session-anthropic 29%.

Using `EscapeScanner` for writing strings with escapes did not help
serialization (`write_escaped_str_slow`).

`Cursor::parse_whitespace` checks eight spaces before it looks at the
next byte.  Checking the byte first (only looking for runs of spaces
after a whitespace character) is fewer instructions for compact JSON but
made parsing Twitter and citm-catalog 15%-20% slower.  Not understood.

Classifying all eight bytes of the word as whitespace (space, line feed,
tab, carriage return with a few SWAR operations) instead of comparing
with eight spaces was slower everywhere although citm-catalog is 71%
whitespace: skipping to the first other byte of the word with its
trailing zeros made deserializing 5%-35% slower (also Canada which has
no whitespace, the position then depends on the data instead of
predicted branches), only skipping words of any whitespace and continuing
byte by byte up to 11% (the check of the word fails at the end of every
run and before every token of compact JSON, and it is inlined at every
call site).

### Error Handling After Events

The parser checks the result of every event (`sink!`).  When raw values
were added, the check whether an error requests a raw value was inlined
in all of them, which made deserializing 2%-8% slower (citm-catalog,
GitHub, Twitter, tree) without being executed.  The error is handled by
the cold `sink_error` now, which brought it back.  The same for
`DriverCore::finish_event` (the error path of every event in the
driver) made no difference.

### Allocations and the Arena

The JSON datasets allocate as often as serde_json (see `allocs`), most
allocations are the strings of the values and the collections.  On
Twitter malloc and free are 15% (the strings of the values half of it,
dropping the result the other half), `Arena` 0.1%, copying 6% (mostly
the strings).  Moving values into their slots and collections (`Status`
into the `Vec`, the fields into the struct) is about 1.3%.  In
cargo-manifest most copying is in inserting into the `BTreeMap`.  The
key lookups of derived structs are compiled into a jump table on the
length and word compares, which is as good as it gets.

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
was about 1 ms.

Empty optional numbers used to construct an error message for every field
and throw it away, accounting for about a third of reading.  After that
was fixed the error was still created (without a message) and thrown away,
about 4% of reading `table`: `Deserialize::__private_rejects_empty_lexical`
now tells `Option` that numbers and booleans reject empty text, which
makes them `None` without delivering it.  Keep successful empty/optional
handling out of error construction when changing lexical conversion or
adapters.

Scanning `table` (1.9 MB, 220,000 fields) took 1.05 ms looking at the
bytes one by one, mostly for the mispredicted branch at the end of every
field.  Finding the end of a field eight bytes at a time (with the
position of the first special character from `trailing_zeros`) was 40%
slower: the fields are short and the next search depends on the result
of the last one, while the predicted byte loop runs ahead.  What helped is
a mask of the special characters of 64 bytes (NEON, SSE2 or SWAR) that is
kept for the fields that follow, together with scanning unquoted fields
after each other without going through the modes: 0.65 ms.  The rest is
storing the fields.

Records are multimaps (names can repeat), which made derived structs ask
every field if it collects (a virtual call per field, about 4% of
reading); `StructFields::collect_fields` has these as bits.  The lenient
rules for booleans compared with all eight spellings ignoring case before
the exact ones (about 2.5%).  Fields of strings are not checked for UTF-8
again.

Writing was 3.1 times slower than `csv`, now 1.6 times.  It formatted
numbers into a buffer and copied them (with `memcpy` calls for a few
bytes), formatted floats with the standard library, looked for special
characters in the bytes after the last full word one by one and compared
every key with the name of its column with `memcmp`.  Fields of up to 32
bytes are now copied with two writes that overlap, floats are formatted
with `zmij` and words that overlap cover short fields and keys.  What is
left are the drivers (`emit_plain_fields` and the plain sink are about a
quarter) and dropping atoms.
