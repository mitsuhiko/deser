# Limitations, Performance and Safety

This document collects the known limitations of deser, where it stands on
performance and how it uses unsafe code.

## Self Describing Formats Only

Deser intentionally only supports self describing formats: formats where the
data says what it is (a map, a string, a number) and structs carry the names
of their fields.  Non self describing formats leave this out and rely on
the reader knowing the type upfront.  Such formats cannot be driven by
the events of deser: the format has to ask the type what to read next
(is the next value a `u8` or a `u16`, how many fields follow, which
variant does a number stand for) while in deser the format tells the type
what it found.

Supporting both kinds of formats with the same traits is the source of a
whole class of runtime failures and surprises in serde (see
[SERDE.md](https://github.com/mitsuhiko/deser/blob/main/SERDE.md)).  Many
of the features that deser is built around (flattening, internally tagged
and untagged enums, catch-all variants, skipping unset fields, lossless
buffering, layers that rewrite events) do not have a meaning in a format
where values are identified by their position.

This makes deser the wrong crate ecosystem for these formats and this is
unlikely to change.  If you need one of them, use serde or the ecosystem
built around the format instead.  Common non self describing formats are:

* [bincode](https://crates.io/crates/bincode)
* [postcard](https://crates.io/crates/postcard)
* [rkyv](https://rkyv.org/)
* [Protocol Buffers](https://protobuf.dev/)
* [Apache Avro](https://avro.apache.org/)

## Thread Safety

Serializables are `Sync`, deserializable types and sinks are `Send` so that
an ongoing serialization or deserialization can move between threads (for
instance while waiting for IO).  This means that types which are not thread
safe (`Rc`, `rc::Weak`, `Cell` and `RefCell`) cannot be serialized or
deserialized.  `Mutex` and `RwLock` can only be deserialized as serializing
them would have to hold the lock guard while the serialization moves between
threads.

## Streaming

XML documents, TOML documents and property lists hold a single value and
cannot be split, so `from_reader` reads the whole stream before it's parsed.
When writing, the output of large values is written in pieces while they are
serialized, except for TOML and binary property lists which need the complete
value (the values of a table come before its subtables, the object table of a
binary property list needs all objects).  Containers whose length is not known
upfront are held back in CBOR and MessagePack until they are complete, as are
XML elements whose attributes can still come.

## Runtime Performance

The current design relies on dynamic dispatch and separately allocated sinks
and emitters for many compound values.  This is the consequence of a certain
level of flexibility and the desire to not use the call stack for recursion.
Deser works around most of this overhead (for instance derived structs and
vectors serialize without allocations, and sinks and emitters are allocated
in an arena instead of one by one).

Compared to serde based libraries in the
[included benchmark](https://github.com/mitsuhiko/deser/tree/main/benchmark)
YAML and TOML are two to four times as fast and CBOR deserializes faster
but serializes slower.  JSON serializes faster but deserializes slower on
float heavy and deeply nested data, MessagePack is on par on string heavy
data and slower on float heavy and deeply nested data.

## Compile Times

The derive generates comparatively little code and relies on dynamic
dispatch instead of monomorphizing everything.  The format crates only
depend on `deser-core`, so they compile in parallel with the derive macros.
See [compile-times](https://github.com/mitsuhiko/deser/tree/main/compile-times)
for a comparison with serde and miniserde.

## Unsafe Code

Deser needs unsafe code internally, primarily to erase lifetimes in the drivers
which keep the chain of borrowed sinks and emitters on the heap rather than the
call stack, and for the arena these sinks and emitters are allocated in.  The
unsafe code is documented, has a dedicated test suite that exercises it
(partial drops, errors, panics, deep nesting) and the test suites of all crates
are run under [miri](https://github.com/rust-lang/miri) with both stacked and
tree borrows (`make miri-test`).  This does not guarantee soundness but if you
find a soundness issue, please report it.
