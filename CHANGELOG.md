# Changelog

All notable changes to deser are documented here.

## Unreleased

- **Breaking:** merged `deser::ser::PausableSink` into `EventSink`.  An
  event sink receives the value of every event (if it sets
  `EventSink::DESCRIBED`) and can pause the driver with
  `EventSink::pause`, which only `SerializeDriver::drive_until` invokes.
  `SerializeDriver::drive_described` was removed, use `drive_sink` with a
  sink that sets `DESCRIBED`.
- **Breaking:** removed `DeserializeDriver::from_sink` and
  `DeserializeDriver::from_state`, use `DeserializeDriver::from_fn` (for
  instance `from_fn(|_| sink)`) or `deser::de::deserialize_value`.
- **Breaking:** `SinkHandle` no longer repeats the methods of `Sink` as
  inherent methods, import `deser::de::Sink` to call them.
- **Breaking:** removed `deser::de::SlotWrapper`.  It could not be used
  outside of deser (sinks have to be implemented for a type of your
  crate), create a slot wrapper with `make_slot_wrapper!` instead, whose
  documentation now describes the type it creates.
- **Breaking:** removed `deser::ser::Written`,
  `StreamSerializer::drive_partial` returns `true` once the value is
  complete (like `SerializeDriver::drive_until`).
- **Breaking:** `Source::set` takes the source like the other settings of
  the state: `Source(input.into()).set(state)`.
- **Breaking:** removed `InputBuffer::drive_transient` (use
  `driver.transient(|driver| buffer.drive(driver))`) and
  `Error::push_error` (use `Error::from_errors`).
- **Breaking:** removed `BytesFormat::is_seq` (compare with
  `BytesFormat::SEQ`), `Error::error_count` (use `errors().count()`),
  `Atom::widen_float` (sinks receive floats as `F64` through
  `Sink::unexpected_atom`) and `OwnedDriver::driver` (use
  `OwnedDriver::with`).
- Added raw values, the equivalent of serde_json's `RawValue` without
  in-band signalling: `deser_json::RawJson`, `deser_jsonc::RawJsonc`,
  `deser_json5::RawJson5`, `deser_cbor::RawCbor` and
  `deser_msgpack::RawMsgpack` hold the encoding of a value in their
  format.  Values of the same format keep their input:
  they are validated without producing events (about 2.5x faster than
  recording them) and written out again unchanged, with the `Borrowed`
  adapter the input is borrowed.  Values of other formats are encoded.
  Raw values are deserialized later with `Raw::deserialize`, other
  formats serialize the value they hold, types that do not take raw
  values (like `deser_value::Value`) receive it parsed.  Formats support
  them with the
  new `deser::ext::{Raw, RawFormat, RawFormatInfo, RawInput}`,
  `State::set_raw_format` and `Error::is_raw_request`.  See the new
  `raw-values` example.
- Added `deser::de::deserialize_value` to implement functions like
  `from_str` of formats so that only the code that depends on the type
  of the value exists once per type.  The formats no longer use hidden
  helpers of `deser-core`.
- Added `DeserializeDriver::multimap_value` and
  `deser::de::missing_multimap_value` for formats that read a single
  value of a key of a multimap (like `deser_env::var`): collections take
  the value as their only item and are empty if the key is missing.

## 0.9.1

- `deser-json`, `deser-toml` and `deser-yaml` format floats with `zmij`
  by default (the new `zmij` feature, which `deser-jsonc`, `deser-json5`
  and `deser-hj` forward).  This is faster and makes binaries smaller,
  the output is the same.  The `speedups` feature implies `zmij` and
  only adds `simdutf8`.
- Moved code that exists once per derived type into `deser-core`.
  Fields of derived structs are deserialized through slots that exist
  once per field type and the helpers for unit enums are no longer
  inlined into every type.  A program with 100 derived types is 23%
  smaller and release builds of derived code are 22% faster.
- Kept unused driver instances and layers out of binaries.  Serializing
  a value at once no longer links the pausable stream driver, constant
  compact JSON configurations no longer link the pretty printer and
  deserializer layers are only linked when layers are added.

## 0.9.0

This release is close to a rewrite of deser.  Almost every public API
changed, the list below summarizes the state of the release rather than
every intermediate step.

### Core

- **Breaking:** everything but the derive macros moved into the new
  `deser-core` crate which `deser` re-exports.  Formats depend on
  `deser-core` and compile in parallel with the derive.  The derive
  macros are no longer re-exported from `deser::derive`, use
  `deser::Serialize` and `deser::Deserialize`.
- **Breaking:** raised the minimum supported Rust version to 1.88 and
  moved all crates to the 2024 edition.
- **Breaking:** the standard library is optional (the `std` feature,
  enabled by default).  Without it `deser` and most formats only need
  `alloc`.  The `io` feature (also default) gates the readers and
  writers of `std::io` streams, reading and writing streams without IO
  (`deser::stream`) does not need it.
- **Breaking:** `DeserializerState` and `SerializerState` were merged into
  a single `deser::State` which is passed as `&mut State` to all methods
  of sinks, serializers and emitters.  Extension values no longer use a
  `RefCell`.  The state carries event data (`State::event_mut` and
  `State::take_event`, used for CBOR and YAML tags), the input range of
  the current event, the source and the policies of a deserialization.
- **Breaking:** `Descriptor` was removed.  Names are only used for error
  messages (`Sink::expecting`), maps and sequences carry a
  `ContainerShape` (order, length and whether keys repeat) on their start
  event and bytes carry their fallback format.
- **Breaking:** `Deserialize`, `Sink`, `SinkHandle`, `DeserializeDriver`
  and related types have a lifetime `'de` so that values can borrow from
  the input (`&str`, `&[u8]`, `Cow` with the `Borrowed` adapter, derived
  structs and enums with lifetimes).  `DeserializeOwned` is implemented
  for types that do not borrow.  Formats pass on borrowed data with
  `DeserializeDriver::emit_borrowed`.
- **Breaking:** drivers are `Send` so that ongoing (de)serializations can
  be suspended across `.await`.  `Serialize` requires `Sync`,
  `Deserialize` and `Sink` require `Send`, types like `Rc` and `RefCell`
  are no longer supported.
- **Breaking:** sinks, emitters and owned values are allocated in an arena
  of the state instead of individual boxes (`SinkHandle::arena` and
  `SinkHandle::heap`, `SerializeHandle::arena` and
  `SerializeHandle::heap`).  `Deserialize::deserialize_into` and related
  methods take the `State`.
- Extended the data model: `Atom::F32`, `Atom::Lexical` for text whose
  type the format cannot express (query strings, CSV, environment
  variables), `Atom::Implicit` for values whose type was inferred from
  their text (YAML plain scalars, Hjson), `Atom::Ext` for extension
  values with a fallback (`u128`/`i128` and the well-known types in
  `deser::ext`: `Datetime`, `Timestamp`, `Duration`, `Uuid`, `Decimal`,
  `BigInt` and `Number`), and multimaps for maps whose keys repeat.
  `Text` and `Bytes` are two word large, borrowed or owned data.
- How lexical atoms are interpreted is configurable with `LexicalRules`
  (strict for JSON keys, lenient for query strings, CSV and environment
  variables where `yes`, `on` and `1` are booleans and empty values are
  `None`).
- Added layers (`deser::de::Layer` and `deser::ser::Layer`) that sit
  between a format and the types and can observe, change, drop or insert
  events.  `deser::de::Limits` limits depth, number of events and the
  length of containers, strings and bytes.
- Added `Recording` (owned) and `RecordBuf` (keeps borrowed data
  borrowed) which record values and replay them later with all event
  data, input ranges and replayable state.  Buffered values (tagged and
  untagged enums, `DefaultOnError`, ...) keep borrowed data borrowed and
  do not lose information.
- Added updates (`Deserialize::deserialize_update`,
  `Deserializer::update`) which merge input into an existing value, for
  instance to layer configuration files over defaults.
- Added the `deser::de::Deserializer` and `deser::ser::Serializer` traits
  which all formats implement, with `deserialize_with` and
  `serialize_with` to configure the drivers.
- Added `DuplicateKeys` (duplicate keys are an error by default) and
  `UnknownFields` (reject or collect unknown keys) policies on the state.
  Both work with flattened fields and internally tagged enums.  Like the
  other settings in the state (`LexicalRules`, `BytesFormat`, `Layout`)
  they are read and changed with their `of` and `set` functions.
- Added `Serialize::describe` and `deser::ser::Describe` with which values
  describe their Rust shape.  `deser-debug` uses it to format values like
  `#[derive(Debug)]`.
- Added `deser::hints` with formatting hints: `Layout` (and the `Compact`
  and `Expanded` adapters) asks formats to lay out containers inline or
  expanded.
- Added `OwnedDriver`, `deser::stream::Streamed<T>` (sequences whose
  elements are handed out while they are read), `Chunk::Forward`,
  `Position` and `Source`.
- Added `DeserializeDriver::transient` which lends a driver out for data
  that lives shorter than the data the driver's sinks can borrow (for
  instance the frame of a value in a stream buffer).

### Errors

- Errors carry the offset, line and column in the input and typed
  attachments (`Error::with_attachment`, `Error::attachment`), for
  instance the `Path` of `deser-path` or the `EnvVar` of `deser-env`.
  Errors of values report their location in all formats, also for
  buffered values.
- Errors can hold multiple errors.  With `State::set_collect_errors`
  derived structs and the standard collections recover from errors of
  their values (`Sink::recover`) and report all problems of the input at
  once, limited by `State::set_max_errors`.
- Improved error messages: unknown variants name the enum and list the
  expected variants, out of range integers report the value and type,
  names are quoted with backticks and messages are lowercase.

### Derive

- Added support for enums with data in all representations known from
  serde (externally, internally, adjacently tagged and untagged),
  `#[deser(untagged)]` on variants, `#[deser(other)]` on any variant (with
  a `#[deser(tag)]` field receiving the unknown tag), `#[deser(default)]`
  for variants used when the tag is missing, `#[deser(repr)]` to name
  variants by their discriminants, integer and boolean variant names,
  `tag_alias` and `content_alias`.  Enums can have lifetime, const and
  non-`'static` type parameters.
- Added support for tuple and unit structs, `#[deser(transparent)]` and
  skipped fields in tuple structs and variants.
- `#[deser(flatten)]` works for structs, maps, `Option`s, `Recording`s,
  internally tagged enums and the fields of struct variants without
  buffering.
- Added `skip`, `skip_serializing`, `skip_deserializing` (on fields and
  variants), `required`, `deny_unknown_fields`, `expecting`, `alias_all`,
  `rename_all_fields`, `crate`, `bound`, `serialize_bound` and
  `deserialize_bound` (on containers, fields and variants), and separate
  `rename(serialize = ..., deserialize = ...)`.  `rename`, `alias`, `tag`
  and `content` accept constants and macro invocations.
- **Breaking:** `#[deser(default = ...)]` takes an expression and
  `#[deser(skip_serializing_if = ...)]` a path instead of strings.
- Added adapters (`SerializeAs` and `DeserializeAs`) selected with
  `#[deser(as = ...)]`, `serialize_as` and `deserialize_as` on fields,
  variants and containers.  `_` stands for the derived implementation, so
  adapters can wrap it.  This covers serde's `with`, `from`, `try_from`
  and `into`.
- The derive explains unsupported and misplaced attributes, points serde
  attributes to their replacements and suggests fixes for typos.
- Derived structs and unit enums generate much less code and the derive
  itself compiles faster.

### Adapters and types

- Added the adapters `Same`, `As`, `DisplayFromStr`, `FromInto`,
  `TryFromInto`, `DefaultOnError`, `VecSkipError`, `MapSkipError`,
  `Borrowed`, `Flag`, `Separated`, `TrimWhitespace`, `SkipBlank` and the
  bytes adapters (`BytesFallback`, `IntSeq` and the base64 encodings).
  Hex and base32 encodings are in the new `deser-encoding` crate.
- Bytes are supported in all formats.  Formats without native bytes write
  base64 strings (configurable with `deser::BytesFormat`), types that
  expect bytes also accept strings and sequences of integers.
- Added support for most of the standard library: `Arc`, `Cow`,
  `Box<str>` and other unsized boxes, `VecDeque`, `LinkedList`,
  `BinaryHeap`, `PhantomData`, `NonZero`, `Wrapping`, `Saturating`,
  `Reverse`, `Result`, IP and socket addresses, paths, `OsString`,
  `CString`, atomics, ranges, `Bound`, `SystemTime`, `Duration`,
  `ManuallyDrop`, `OnceLock`, `Mutex`, `RwLock` and `Infallible`.
- Added features for the types of other crates: `jiff`, `chrono`, `time`,
  `uuid`, `rust_decimal`, `bigdecimal`, `num-bigint`, `indexmap`,
  `hashbrown`, `smallvec`, `arrayvec`, `bytes` and `bstr`.
- Collections collect all values of a repeated key and accept a single
  value as a collection of one.

### New crates

- `deser-value`: a dynamic `Value` that keeps all information of the data
  model including event data and source locations, with `to_value`,
  `from_value` and the `value!` macro.
- `deser-toml`: TOML 1.1, passes the toml-test suite.
- `deser-yaml`: YAML with serialization, flow style, scalar styles,
  `!!binary`, `!!timestamp` and implicit typing of plain scalars.
- `deser-cbor`: CBOR (RFC 8949) with tags, deterministic encoding and the
  well-known types.
- `deser-msgpack`: MessagePack with timestamps and extensions, passes the
  msgpack-test-suite.
- `deser-xml`: XML with attributes, repeated elements, namespaces,
  mixed content (`Mixed<T>`), the root element (`Root<T>`) and pretty
  printing.  Recordings and values keep the root element and the
  namespace declarations of all elements.
- `deser-plist`: property lists in the XML, binary and OpenStep formats
  with format detection, dates as `Timestamp` and keyed archive UIDs.
- `deser-jsonc`, `deser-json5` and `deser-hj`: the JSON dialects, with
  parsers generated from the one of `deser-json`.
- `deser-csv`: CSV, TSV and other delimited text with configurable
  dialects.
- `deser-urlencoded`: query strings and form data with nested keys.
- `deser-env`: environment variables with a prefix and nested keys.
- `deser-serde`: the `Serde` adapter to use serde implementations.
- `deser-validate`: validators (`Len`, `Range`, `Email`, `Each`,
  `validator!`), the `Check` adapter, `Validated<T, V>` and `Validation`
  which reports all problems of an input.
- `deser-transcode`: converts between formats without types in between.
- `deser-tokio`: reads and writes values with tokio's `AsyncRead` and
  `AsyncWrite`, with an optional tokio-util codec.
- `deser-location`: source locations for values with `Spanned<T>`.
- `deser-encoding`: hex and base32 encodings.

### Formats and IO

- **Breaking:** format options moved into `DeserializerConfig` and
  `SerializerConfig` types which are `const` constructible and reusable.
  Single values are serialized with `SerializerConfig::to_string` /
  `to_vec` and deserialized with `from_str` / `from_slice`.  The
  `Serializer` and `Deserializer` types of the formats are created from a
  configuration (`with_config`, `from_str_with_config`) and handle more
  than one value.
- **Breaking:** `deser-path` was rewritten as `PathLayer` which works in
  both directions and attaches a structured `Path` to errors.
- Added stream serializers and deserializers which do not do IO
  (sans-io).  The `Serializer` of every format that writes bytes (JSON,
  CBOR, MessagePack, YAML, TOML, XML, property lists, CSV and query
  strings) implements `ser::StreamSerializer`: it holds the state of a
  stream of values and its output, which is taken with `output` and
  `clear_output`.  Every format has a `StreamDeserializer` (implementing
  `de::StreamDeserializer`) which splits the input of a stream into
  frames that are deserialized with the regular parser (values can borrow
  from the frame) or deserializes values while their input is fed
  (JSON and its dialects, CBOR and MessagePack), only buffering incomplete
  tokens.  `deser::stream` holds the `InputBuffer` which buffers the input
  and invokes a stream deserializer, and the `ElementReader` for
  `Streamed<T>`.
- Added `deser::io` with `Reader` and `Writer` for `std::io`.  The
  configurations of the formats create them (`DeserializerConfig::reader`
  and `SerializerConfig::writer`).  A `Writer` is a `ser::Serializer` and
  a `Reader` is a `de::Deserializer` (with `Reader::is_end`), so generic
  code like `deser-transcode` can read from and write to streams.
- Stream serializers write values in parts (`StreamSerializer::drive_partial`):
  once the output of a value exceeds the buffer limit
  (`Writer::set_buffer_limit`, 8 KiB by default) it's written and the
  serialization continues, so the memory used for writing does not depend
  on the size of the values.  All formats support this except TOML and
  binary property lists, which need the complete value.  This also covers
  `to_writer`, `deser-tokio` and CSV documents (`csv::Serializer::document`).
  Values below the limit are still written at once.  A value that was
  abandoned partway through stays in progress
  (`StreamSerializer::in_progress`) and the stream refuses more values
  (with `Error::in_progress`).
- The state of a stream is created with the stream serializer or
  deserializer, for instance to continue a CSV file with known columns
  (`csv::Serializer::with_headers`, `csv::StreamDeserializer::with_headers`)
  or a JSON stream after a number of values (`json::Serializer::with_written`).
- Added `SerializeDriver::drive_until` and `PausableSink` to drive a
  serialization until the sink pauses it.  Large plain sequences are
  emitted in pieces so that the driver can pause in between.
- `deser-xml` supports streams: `from_reader`, `to_writer` and the
  readers and writers of `deser::io` (the `io` feature).
- The parameters of all values written with a `deser-urlencoded` writer
  are joined, like with its `Serializer`.
- `deser-json` reads from bytes, reads streams of values and JSON Lines
  (`Trailing`), can pretty print, keeps exact numbers
  (`deser::ext::Number`) and has source locations.
- JSON, YAML and TOML write floats with the shortest text for their
  precision, with the same output with and without the `speedups` feature
  (which uses `zmij` and `simdutf8`).

### Fixes

- Fixed multiple soundness issues in the deserialize driver and
  `OwnedSink`.
- Fixed integer range checks which accepted `u64::MAX` as `-1` for `i64`
  and `-1` as `u64::MAX`.
- Fixed `Option<T>` silently dropping structs, vectors, maps and boxes.
- Fixed `HashMap` deserialization and deriving `Deserialize` for generic
  structs.
- Fixed JSON serialization of `char` and a panic on trailing commas in
  arrays.

### Performance

- Deserializing and serializing is substantially faster, JSON is on par
  with `serde_json` in the included benchmark (it was 1.5 and 1.9 times
  slower).  The arena removes up to a quarter of the allocations of
  deserializations and up to three quarters of the allocations of
  serializations.

## 0.8.0

- Removed `for_each_event`.
- Added `speedups` feature for `serde-json` to use `ryu` and `itoa`.
- Added deserialization support for BTreeSet and HashSet.
- `Option<T>` is now automatically defaulted in structs, even if
  `#[deser(default)]` is not provided.  If this distinction between
  missing and null is needed `Option<Option<T>>` can be used.
- Removed non String serialization support for deser-json for now.

## 0.7.0

- Added support for `Box<T>`.
- Added newtype struct support for derive feature.
- `Driver` is now called `DeserializeDriver`.
- Added `SerializeDriver`.
- Added `deser-json`.

## 0.6.0

- Made `Atom` non exhaustive and added `unexpected_atom` to `Deserialize`.
- Removed `MapSink` and `SeqSink`.  The functionality of these is now
  directly on the `Sink`.
- The serializer state and deserializer state is now passed to `next_key`/
  `next_value` and `next` on the sinks and emitters.
- Added support for `#[deser(flatten)]`.

## 0.5.0

- Added support for `#[deser(default)]` in deriving.
- Added support for `#[deser(skip_serializing_optionals)]`.
- Removed `ignore` and replaced it with `SinkHandle::null`.
- Added tuple support.
- Added array support.
- Added support for `#[deser(alias)]`.
- Added support for characters to the data model.
- Added support for serializing references.

## 0.4.0

- Restructure serialization and deserialization to pass `Atom` values
  within `Event` and `Chunk`.  This changes the interface from invoking
  type specific methods on the sink to passing an entire `Atom` instead.
- Events are now passed by value rather than reference.
- Added basic support for `Option<T>`.
- Added basic support for deriving simple enums.
