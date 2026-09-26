# Changelog

All notable changes to deser are documented here.

## Unreleased

- JSON, YAML and TOML format floats with `zmij` with the `speedups`
  feature (`deser-json` used `ryu` before, YAML and TOML the standard
  library).  All three write the same shortest text now, exponents always
  have a sign (`1e+16`) and values between `1e-5` and `1e16` (`1e-6` and
  `1e13` for `f32`) are written without exponent.  The output does not
  depend on the feature.
- `deser-yaml` no longer allocates for every scalar it writes.
- Added `Atom::Lexical` for text whose type the format cannot express
  (like the values of query strings).  The sink decides what it means:
  integers and floats parse it, `bool` accepts `true`, `yes`, `on`, `1`,
  `false`, `no`, `off` and `0` (ignoring ASCII case), `()` the empty
  string, and all types that accept strings take it as string.  The
  default `Sink::unexpected_atom` passes it on as `Atom::Str`, so sinks
  that only handle strings keep working, sinks that borrow strings have
  to handle it themselves.  Serializers write it as string.  Lexical atoms
  are retained when values are buffered, so they also parse in flattened
  structs and internally tagged and untagged enums (which is where serde
  loses this information).  `Atom::as_str` returns the text of both.
  `deser-value` has `Kind::Lexical` (which compares and hashes like the
  same `Str`) and `deser-serde` parses lexical atoms with the type that
  serde asks for.
- The keys of JSON objects and TOML tables are lexical atoms.  Keys parse
  into the type of the key (`{"80": true}` into `HashMap<u16, bool>` like
  before, now also `bool` and other types that parse lexical atoms).
  Strings in key position are no longer parsed as integers
  (`State::is_map_key` is no longer consulted for this), which matters
  for maps built by hand (for instance `deser_value::value!` with string
  keys).  `deser-serde` parses lexical atoms instead of strings in key
  position.
- Added `deser::de::DuplicateKeys` which decides what happens if a key is
  given more than once for a single value (a field of a derived struct or
  an entry of a map): the last value wins (the default, like before), the
  first one wins or it's an error.  It's set on the state
  (`State::set_duplicate_keys`) and also applies to buffered values.
  `MapSkipError` skips duplicate entries if they are an error.  Values of
  fields that are containers (like `Vec`) are replaced, not merged.
- Sequences can be marked as the values of a key that was given more than
  once (`ContainerShape::with_repeated`), like `a=1&a=2` in a query
  string.  Types that accept sequences receive the values, for types that
  do not the driver picks a single value according to `DuplicateKeys`.
  `deser-value` keeps the flag on `Seq`.
- Sequences (`Vec`, `VecDeque`, sets, arrays, ...) accept a single lexical
  atom as a sequence of one element, like a key that is given once in a
  query string.  Byte buffers decode it as bytes like strings.
- Optionals are `None` for an empty lexical atom if their value rejects it
  (`?limit=` is `None` for an `Option<u32>` and `Some("")` for an
  `Option<String>`).
- Added `Atom::F32` for single precision floats.  `f32` values are no
  longer widened to `f64` when serialized, so the text formats write them
  with the shortest text for their precision (`0.1f32` as `0.1` instead of
  `0.10000000149011612`) in JSON, YAML and TOML, which also makes the
  output smaller and faster to write.  The default
  `Sink::unexpected_atom` widens `F32` into `F64`, so sinks that only
  handle `F64` keep working (`Atom::widen_float` does the same for other
  consumers).  Serializers have to handle the new atom.  Parsers keep
  producing `F64`.  `deser-value` has `Kind::F32` (which compares and
  hashes like the same `F64`), `deser-serde` maps it to serde's `f32` and
  `deser-debug` formats it like `Debug`.  `Decimal` and `Number` use the
  shortest text of `f32` values.
- Added the `io` feature (enabled by default in `deser` and the formats)
  for everything related to streams (`deser::io`).  Without it the formats
  have deserializers and serializers for in-memory data only.
- Added the `deser::io::Decoder` and `deser::io::Encoder` traits for data
  formats which deserialize from and serialize into streams of bytes.  The
  configurations of the formats implement them, which makes them usable in
  generic code: `Decoder::from_slice` and `Decoder::from_reader`,
  `Encoder::to_vec` and `Encoder::to_writer`.
- Added `deser::Streamed<T>`, a sequence whose elements are handed out
  while a value is read with `deser::io::Reader::read_next` (as
  `Next::Element`, followed by the value as `Next::Done`) instead of being
  collected.  Otherwise it behaves like a `Vec<T>`.  `ElementReader`
  implements this without IO, `deser-tokio` has `Reader::read_next` and
  `Reader::into_element_stream`.
- Added `OwnedDriver`, a `DeserializeDriver` which owns the value it
  deserializes.  It can be held across calls, for instance to deserialize
  a value from input which arrives over time.
- Decoders can deserialize values while their input arrives
  (`Decoder::feed`), JSON (except for JSON Lines) and CBOR support this.
  `Reader::read` (also in `deser-tokio`) uses it if possible, which only
  buffers incomplete tokens instead of the complete value.  The JSON and
  CBOR parsers were rewritten as state machines which can be suspended
  between tokens for this.  Values that fail in a sink are skipped so the
  stream continues.  `Decoder::is_text` controls if the positions of
  errors are resolved into lines and columns.
- Added the `deser::ser::Serializer` trait and a `Serializer` for all
  formats (JSON, CBOR, YAML and TOML) which serializes values into an
  in-memory output.  More than one value can be written (as JSON Lines
  with `Trailing::Newline`, a CBOR sequence or YAML documents).
- `deser_json::SerializerConfig::default()` returns the same configuration
  as `new()` (it was derived before which disabled compact output).
- Added `deser::io` to read values from and write values to streams with
  decoders and encoders: `Reader` and `Writer` (and `from_reader` and
  `to_writer`) use them with `std::io::Read` and `std::io::Write`,
  `Writer::write_with` supports layers.  Decoders split streams into the
  frames of values.  `DecodeBuffer` implements the framing without doing
  IO itself for other kinds of IO (such as async runtimes).  Errors of
  values refer to positions in the stream.
- `deser-json` reads and writes streams: `from_reader` and `to_writer`
  (also on the configurations) and the configurations for `deser::io`.
  Streams are split according to `Trailing`: a single value, JSON Lines or
  concatenated values.  `SerializerConfig::trailing` is the counterpart
  for writing (`Trailing::Newline` writes JSON Lines).
- `deser-cbor` reads and writes streams: `from_reader` and `to_writer`
  (also on the configurations) and the configurations for `deser::io`
  which read and write CBOR sequences.  Items are split by scanning their
  heads.
- `deser-toml` reads and writes streams: `from_reader` and `to_writer`
  (also on the configurations) and the configurations for `deser::io`.  A
  stream holds a single document.
- `deser-yaml` reads and writes streams: `from_reader` and `to_writer`
  (also on the configurations) and the configurations for `deser::io`
  which read and write streams of documents.  Documents are split at
  document markers.  `SerializerConfig::end_documents` ends every document
  with `...` for streams that stay open.
- Added `deser-tokio` which reads and writes values with tokio's
  `AsyncRead` and `AsyncWrite` using the configurations of the formats:
  `Reader` (also as a `Stream`), `Writer`, `from_reader` and `to_writer`.
  The futures are `Send` and reads are cancellation safe.  With the
  `codec` feature `Codec` implements the codec traits of tokio-util.
- Added `ErrorKind::Io` for failed reads and writes.  `std::io::Error`
  converts into `Error`.
- Ongoing serializations and deserializations can move between threads:
  `SerializeDriver` and `DeserializeDriver` are `Send`.  This allows them
  to be suspended across an `.await` in multi threaded runtimes.
  - `Serialize` requires `Sync` and the emitters (`StructEmitter`,
    `MapEmitter`, `SeqEmitter`) require `Send`.  Owned values in a
    `SerializeHandle` are `Send`.
  - `Deserialize`, `Sink` and `VariantBuilder` require `Send`.
  - Serialization and deserialization layers require `Send`.
  - Extension values in the `State` must be `Send + Sync`.
  - Derived implementations require `Sync` (for `Serialize`) and `Send`
    (for `Deserialize`) of type parameters which only appear in fields
    with adapters.
  - Types that are not thread safe (such as `Rc` or `RefCell`) can no
    longer be serialized or deserialized.
- Added the `deser-value` crate with a dynamic `Value` type.  Maps can have
  any value as key and keep the order of their entries, extension values
  keep their type, and maps and sequences keep their `Order`.  Event data
  (such as CBOR and YAML tags or formatting hints) and, if the format tracks
  locations, the span of every value are kept in its `Meta` data, so types
  deserialized from a value report errors at the original location.  Values
  are converted with `to_value` and `from_value` (which can borrow strings
  from the value), the `Serializer` and `Deserializer` configure the
  conversion (for instance with layers).  Values are built with the
  `value!` macro.  Values are processed
  without recursion, including dropping, cloning, comparing and formatting.
- Added `EventData` which holds event data detached from its event.
  `State::capture_event_data` captures the data of the current event and
  `State::attach_event_data` attaches it to another event.  Event data now
  has to be `Sync` (`State::event_mut` requires it).
- Removed `Descriptor`.  The information it carried moved to where it
  belongs:
  - Bytes carry the format for formats without native bytes as
    `Bytes::fallback`: `Atom::Bytes` holds a `Bytes` (which dereferences to
    `[u8]`) instead of a `Cow<[u8]>`.  `BytesFallback` sets it.
  - The start events of maps and sequences carry a `ContainerShape`:
    `Event::MapStart(ContainerShape)` and `Event::SeqStart(ContainerShape)`
    (`Event::map_start()` and `Event::seq_start()` create the default).  It
    holds the `Order` of the elements (`HashMap` and `HashSet` are
    `Order::Arbitrary`, `BTreeMap` and `BTreeSet` `Order::Sorted`) and the
    number of elements if known.  Types report it with
    `Serialize::container_shape` (`SerializeAs::container_shape_as`), sinks
    read it with `State::container_shape`.
  - Names are only used for error messages: sinks provide them with
    `Sink::expecting`, `Sink::descriptor` is gone.
  - The precision of numbers is gone.  Floats are `f64`, `f32` values are
    written as the `f64` they widen to (`0.1f32` is written as
    `0.10000000149011612` in JSON and TOML).
  - The serialize driver callback, `Layer::event` and `Next::emit` no
    longer receive a descriptor, `SerializeDriver::next` returns the event,
    the value and the state, `State::top_descriptor` is gone.

  Serialization got 4-12% faster.
- Replaced the path of errors with typed attachments: `Error::with_path`
  and `Error::path` are gone, `Error::with_attachment`,
  `Error::attachment`, `Error::attachment_mut` and `Error::attachments`
  attach and retrieve values of types implementing the new
  `ErrorAttachment` trait, which can contribute to the error message.
  The location of errors stays built in.  `PathLayer` attaches the
  structured `Path` (`err.attachment::<Path>()`) instead of a string.
- Added `Serialize::describe` and `deser::ser::Describe` with which values
  describe their Rust shape: structs, newtypes, enum variants (with their
  kind and representation), `Option`, tuples and sets.  The derive and the
  standard types implement it.  Formats that want the description use
  `SerializeDriver::drive_described` which passes the value of every event.
  `deser-debug` uses it and formats values like `#[derive(Debug)]` (including
  struct and newtype names).
- `deser-yaml` can serialize: `to_string`, `SerializerConfig` (indentation
  with `Indent`, where `Indent::None` writes documents on a single line in
  flow style, indented or indentless sequences, quote style, multi-line
  strings as literal block scalars or quoted, null style, `!!binary` or a
  bytes format, timestamps, document markers).  Strings are quoted if readers of YAML 1.1 or 1.2 would read
  them as something else (`SerializerConfig::compat`).  `Tagged` writes its
  tag, `set_tag` sets the tag of a value and tags survive a `Recording`.
  `DeserializerConfig::bytes` configures how strings are decoded into
  bytes.
- `deser-yaml` writes collections in flow style if they have the
  `Layout::Compact` hint or, with `FlowPolicy::LeafIfFits`, if they only
  contain scalars and fit into the width.  The style of strings can be
  requested with the `ScalarStyle` hint of the new `deser_yaml::style`
  module (and its adapters `Plain`, `SingleQuoted`, `DoubleQuoted`,
  `Literal` and `Folded`), long strings can be folded
  (`SerializerConfig::fold_width`).  Flow collections are reported as
  compact when reading.
- `deser-json` can pretty print: `SerializerConfig::indent` sets the
  indentation (`Indent::Spaces(n)` or `Indent::Tab`),
  `SerializerConfig::compact(false)` writes spaces after separators and
  `SerializerConfig::pretty` does both.  In indented output maps and
  sequences with the `Layout::Compact` hint are written on a single line,
  with `SerializerConfig::inline(InlinePolicy::LeafIfFits(width))` also
  the ones that only contain scalars and fit into the width.
- `deser-json` writes floats the same way with and without the `speedups`
  feature.  Without it floats were written like `Display` does, without
  exponent and without fraction (`1.0` as `1`, `1e300` with 301 digits).
- Added `deser::hints` with well-known formatting hints.  `Layout` asks
  formats to lay out a map or sequence compact (inline) or expanded, the
  `Compact` and `Expanded` adapters set it (`#[deser(as = Compact)]`) and
  layers can set it by path.  `Hint` and `Hinted` allow formats to define
  adapters for their own hints.  `deser-toml` writes compact tables and arrays
  of tables inline and reports inline tables as compact when reading, so
  they stay inline through a `Recording`.  It also reports the lengths of
  tables and arrays.
- `deser-cbor` uses the same event data for tags when reading and writing,
  values that capture event data (such as `Recording`) keep the tags when
  they are serialized again.
- `deser-cbor` writes definite lengths directly if the length of a
  container is known and fails if the number of items does not match.  It
  passes the declared lengths of its input on, which `Vec`, `HashMap` and
  `HashSet` use to preallocate (at most 1 MiB).
- Raised the minimum supported Rust version to 1.88 and moved all crates to
  the 2024 edition.  The minimum version is now declared as `rust-version`
  and tested on CI.
- Added layers: `deser::de::Layer` and `deser::ser::Layer` sit between a
  format and the types and see every event.  They are added to the drivers
  with `push_layer` and can observe, reject, change, drop and insert events.
  Deserialization layers know the position of an event (`State::is_map_key`
  and `State::depth`) and replayed values do not pass through them again.
  Serialization layers are only applied by `SerializeDriver::drive`,
  `SerializeDriver::next` panics if layers were added.
- Added `deser::de::Limits`, a layer which limits the depth, the number of
  events, the number of items of maps and sequences and the length of strings
  and bytes.  The `max_depth` options of `deser-yaml` and `deser-cbor` add
  this layer and no longer have their own implementation.
- Added the `deser::de::Deserializer` trait which is implemented by the
  deserializers of all formats.  `Deserializer::deserialize_with` (also an
  inherent method of the deserializers) allows configuring the driver, for
  instance to add layers or to wrap the sink with the new
  `DeserializeDriver::wrap_sink`.  The formats' `Deserializer::deserialize`
  forwards to it.  The serializer configurations have new `to_string_with`
  (`to_vec_with` for CBOR) methods to configure the serialize driver.
- Errors carry context: the offset, line and column in the input
  (`Error::offset`, `Error::line`, `Error::column`) and the path of the value
  (`Error::path`), which are part of the `Display` output.  The deserialize
  driver attaches the start of the input range of an event to the errors of
  that event, also for replayed values, and types implementing the new
  `ErrorContext` trait registered with `State::add_error_context` add further
  context (the serialize driver runs them as well).  The formats resolve offsets into lines and columns (except
  CBOR which reports offsets), so errors of values (for instance type errors)
  now report their location in all formats.  Syntax errors of `deser-json`
  now have locations too.  The syntax errors of `deser-yaml` and
  `deser-cbor` read `syntax error: ... at line L column C` and
  `syntax error: ... at offset N`.
- `deser-json` can read streams of values like the CBOR and YAML
  deserializers: `Deserializer::deserialize` reads the next value and
  `is_end`, `end`, `iter` and `offset` were added.  What may follow a value
  is controlled by `DeserializerConfig::trailing`: `Trailing::Strict` (the
  default) only allows whitespace, `Trailing::Newline` reads JSON Lines
  (NDJSON) where errors only skip their line and `Trailing::Stop` stops
  after the value regardless of what follows.
- `deser-cbor` publishes the byte ranges of data items as input ranges.
- `deser-path` was rewritten as a layer: `PathLayer` replaces `PathSink` and
  `PathSerializable` and works in both directions.  It adds the path to
  errors, `Path` formats as `servers[1].port` and during serialization the
  format sees keys with the path of their map.
- Added the `layers` example with serialization layers that rename keys,
  skip null values and redact values.  The `located` example uses layers.
- Added the `adapters`, `borrowing`, `bytes`, `config-errors`,
  `deep-nesting`, `formats`, `json-lines` and `optionals` examples.  The
  `derive` example was merged into the `json` example.  The examples are
  listed in `examples/README.md`.
- Format options moved from the deserializers and serializers into new
  `DeserializerConfig` and `SerializerConfig` types in all formats.  They
  do not borrow the input, can be created in constants (the constructors
  and setters are `const fn`) and reused.  Their `from_str` / `from_slice` and `to_string` /
  `to_vec` methods work like the functions of the same name, which use the
  default configuration.
  - `Deserializer::new` was replaced by `Deserializer::from_str` and
    `Deserializer::from_slice` (only `from_slice` for CBOR), and
    `from_str_with_config` / `from_slice_with_config` create a deserializer
    with a configuration which is available as `Deserializer::config`.  The
    deserializer only holds the parsing state and has no setters anymore.
  - The `Serializer` types were removed: `Serializer::new().serialize(&v)`
    becomes `SerializerConfig::new().to_string(&v)` (`to_vec` for CBOR).
    `deser_cbor::to_canonical_vec` was removed in favor of
    `SerializerConfig::new().canonical(true).to_vec(&v)`.
  - `max_depth` takes a `usize` instead of an `Option<usize>`.
- `Deserialize`, `Sink`, `SinkHandle`, `DeserializeDriver`, `OwnedSink` and
  `DeserializeAs` have a lifetime `'de` for the data that is deserialized.
  Types can borrow from it: `&str` and `&[u8]` borrow, `Cow<str>` and
  `Cow<[u8]>` borrow with the new `Borrowed` adapter and the derive supports
  structs with lifetimes.  `DeserializeOwned` is implemented for types which
  do not borrow.
  - Formats emit borrowed data with `DeserializeDriver::emit_borrowed`,
    sinks receive it in
    `Sink::borrowed_atom` (and `borrowed_key_atom` / `borrowed_value_atom`)
    which default to the regular methods.  Data emitted with `emit` is only
    valid for the call and cannot be borrowed.
  - `deser-json`, `deser-cbor`, `deser-yaml` and `deser-toml` pass on strings
    (and CBOR byte strings) that are slices of the input borrowed.  Their
    `from_str` / `from_slice` functions and `Deserializer::deserialize` tie
    `'de` to the input.
  - To migrate, `impl Deserialize for T` becomes
    `impl<'de> Deserialize<'de> for T` and `impl Sink for S` becomes
    `impl<'de> Sink<'de> for S`, `SinkHandle<'_>` becomes
    `SinkHandle<'_, 'de>`.  Bounds of `#[deser(bound(...))]` which also apply
    to `Serialize` use `DeserializeOwned`, `deserialize_bound` can use
    `Deserialize<'de>`.
  - Recordings are detached from the data, so buffered values (for instance
    in internally tagged or untagged enums) cannot be borrowed.
- Added the well-known `deser::ext::Number` extension: a number literal
  of a text format (in the syntax of JSON numbers) together with its value
  as `f64` which is the fallback.  `Decimal`, `BigInt` and the bridged
  `rust_decimal`, `bigdecimal` and `num-bigint` types use the text, floats
  use the value.  `deser-json` emits floats whose text cannot be recovered
  from their value as `f64` (and integers that do not fit into 128 bits) as
  numbers borrowing the text, so decimals are deserialized exactly.  This
  can be disabled with `DeserializerConfig::exact_numbers`.  `deser-json` writes
  numbers verbatim, `deser-toml` writes the text of non-integer numbers.
- Extension values can borrow data: extensions implement the new
  `BorrowedExtension` trait on a `'static` key type which defines the type
  of the values for a lifetime.  They are created with
  `ExtValue::borrowed_value` and `ExtValue::owned_value` and looked up with
  `ExtValue::downcast_value_ref`.  Owned extension values are now reference
  counted which makes cloning them cheap.  `ExtValue::owned` no longer
  returns an `ExtValue<'static>`.
- Added well-known extension types to `deser::ext`: `Datetime` (with
  `Date`, `Time` and `Offset`), `Timestamp`, `Duration`, `Uuid`, `Decimal`
  and `BigInt`.  They are dependency free representations of common types
  that data formats can support natively, with string fallbacks for formats
  that do not.  `std::time::SystemTime` and `std::time::Duration` now
  implement `Serialize` and `Deserialize` through them, as do the types of
  `jiff`, `chrono`, `time`, `uuid`, `rust_decimal`, `bigdecimal` and
  `num-bigint` with the new features of the same names.
- `deser-cbor` supports the well-known types: date/time strings (tag 0),
  epoch based date/times (tag 1, read as tagged numbers), decimal fractions
  (tag 4), UUIDs (tag 37) and full-date strings (tag 1004).  Valid date/time
  strings, decimal fractions, UUIDs and full-dates are no longer passed on
  as tagged values but as well-known types, and bignums that do not fit into
  128 bits are passed on as `BigInt` instead of tagged byte strings.
- `deser-json` writes `BigInt` and `Decimal` as numbers.
- `deser-yaml` supports the `!!timestamp` tag (as `Datetime`).
- Added `deser-serde` with the `Serde` adapter which serializes and
  deserializes values with their serde implementations
  (`#[deser(as = Serde)]`).  Compound values are buffered.
- Added `deser-toml` which implements TOML 1.1 from scratch.  It passes the
  toml-test suite, supports date-times through the well-known `Datetime`
  type, writes maps as tables and sequences of maps as arrays of tables and
  supports source locations.
- Merged `DeserializerState` and `SerializerState` into a single
  `deser::State` without a lifetime parameter.  `is_map_key` and
  `set_replayable` are available in both directions and the serialize driver
  now sets `is_map_key` while map and struct keys are serialized.
- `SerializeDriver::drive` and `SerializeDriver::next` now hand out
  `&mut State` so that formats can consume information from the state.
  `deser_cbor::push_tag` now takes `&mut State`.
- Extension values in the state now need to be `Send` and `Descriptor`
  requires `Sync`.  This makes `State` `Send`.
- Replayed recordings now continue on the state of the ongoing
  deserialization.  `State::depth` and `State::top_descriptor` report the
  same values for buffered values (for instance in internally tagged enums)
  as for values that are not buffered, and maps and sequences remain the
  current container while their sink is finished.
- Improved the performance of serializing CBOR tags.
- Added event data to the `State`: values attached to a single event with
  `State::event_mut` and read with `State::event`.  Formats attach data
  before they emit an event and the deserialize driver detaches it after
  every event, during serialization `Serialize` implementations attach data
  to their first event and the serialize driver detaches it after the event
  was delivered.  Recordings capture event data
  automatically.  The CBOR and YAML tags are now event data, `Locations` is
  no longer replayable.
- Added input ranges to the `State`: formats set the byte range in the input
  of the next event with `State::set_input_range` (the driver detaches it
  after the event) and sinks read it with `State::input_range`.  Recordings capture the range of every event.
  The source the ranges refer to is available as `State::source` if the
  format provides it.  `deser-json`, `deser-toml` and `deser-yaml` always
  publish input ranges, which has no measurable overhead, and
  `track_locations` sets the source.  The formats no longer depend on
  `deser-location` and their `locations` features were removed.
- `deser-location` resolves spans from the input range and builds its
  source map from `State::source` on first use.  `Locations::set_current`
  and `Locations::set_source_map` were removed and
  `Locations::current_span` and `Locations::source_map` now take
  `&mut State`.
- Added adapters in `deser::adapters` which serialize and deserialize values
  on behalf of other types through the `SerializeAs` and `DeserializeAs`
  traits.  The derive selects them with `#[deser(as = Adapter)]` on fields
  (including newtype structs and enum variant fields) where `_` stands for
  the type's own implementation.  Adapters compose with the standard
  containers (`#[deser(as = Option<BTreeMap<_, Vec<DisplayFromStr>>>)]`),
  handle missing fields themselves so optional adapters keep fields optional,
  and type parameters only used in fields with adapters no longer need to
  implement `Serialize` or `Deserialize`.  Provided are `Same`,
  `DisplayFromStr`, `FromInto`, `TryFromInto`, `DefaultOnError`,
  `VecSkipError` and `MapSkipError`, as well as the `As` wrapper to use
  adapters outside of the derive.
- Added `Chunk::Forward` to serialize another (possibly owned) value in
  place of a value.
- `Deserialize::initial_value` (previously the hidden
  `__private_initial_value`) and `SinkHandle::ignore_null` are now public.
  Added `OwnedSink::deserialize_as`.
- `Recording` now implements `Deserialize`, `Serialize` and `PartialEq` so
  that it can be used as a raw value.  Serializing a recording emits the
  recorded events together with their event data.
- `#[deser(other)]` is now supported on all variants.  A field marked with
  `#[deser(tag)]` receives the unknown tag, which can be any value (not only
  strings), and is used as tag when serializing.  The remaining fields are
  the content of the variant.
- Added `#[deser(default)]` for variants of internally and adjacently tagged
  enums which is used if the tag is missing.
- Variants with content of externally tagged enums that are represented by
  their name alone now receive null as content instead of failing, and plain
  enums with an `other` variant use it for values which are not strings.
- Newtype variants of internally tagged enums can now serialize maps with
  string keys and values that forward to maps or structs.

- Changed `#[deser(default = ...)]` to take an expression instead of a
  function name in a string, and `#[deser(skip_serializing_if = ...)]` to
  take a path instead of a string.  String literals given as defaults are
  converted with `Into`, and `Self` is not supported in either attribute:

  ```rust
  // before
  #[deser(default = "default_port", skip_serializing_if = "Option::is_none")]
  // after
  #[deser(default = default_port(), skip_serializing_if = Option::is_none)]
  #[deser(default = 8080)]
  #[deser(default = "localhost")]
  ```
- Added `#[deser(bound(...))]`, `#[deser(serialize_bound(...))]` and
  `#[deser(deserialize_bound(...))]` to replace the bounds the derive
  infers for type parameters.
- Added `#[deser(crate = path)]` to use the derive when deser is renamed or
  re-exported.
- Moved `deser-derive` to `syn` 3.  This requires Rust 1.71 or later.
- Added `deser_json::from_slice` and `Deserializer::from_slice` which parse
  JSON from bytes and validate the strings as UTF-8 while parsing.
- The `speedups` feature of `deser-json` and `deser-cbor` validates UTF-8
  with `simdutf8`.
- Added `deser-cbor` which implements CBOR (RFC 8949).  It reads all
  well-formed CBOR (including indefinite length items), writes the preferred
  serialization, optionally with deterministic map ordering, maps bignums
  onto `u128` / `i128` and exposes tags through the state and the `Tagged`
  wrapper.
- Improved performance substantially.  Deserializing and serializing JSON is
  now on par with `serde_json` in the included benchmark (previously about
  1.5 and 1.9 times slower):
  - Added `Sink::key_atom` and `Sink::value_atom` which receive atoms in
    containers without creating intermediate sinks.  The default
    implementations use `next_key` and `next_value`.
  - Added `SerializeDriver::drive` which invokes a callback for every event
    and is faster than calling `next` repeatedly.
  - Derived structs, vectors, slices, arrays and tuples serialize without
    allocating emitters and most values need a single dynamic call.
  - Boxed sinks reuse memory through a per thread cache.
  - State extensions are looked up without hashing.
  - `deser-json` parses with a direct state machine and writes output
    through a buffer optimized for small writes.
- The deserializer and serializer states are now passed as
  `&mut DeserializerState` and `&mut SerializerState` to all methods of
  `Sink`, `Serialize` and the emitters.  Added
  `DeserializeDriver::state_mut` for formats.
- State extensions no longer use a `RefCell`.  `get_mut` now takes the state
  mutably and returns a `&mut T`, `get` returns an `Option<&T>` which is
  `None` if the value was never set.  `set_replayable` takes the state
  mutably.  Conflicting borrows of extension values are now compile time
  instead of runtime errors.
- `Serialize::descriptor` now returns a `&'static dyn Descriptor` like
  `Sink::descriptor`.  `SerializerState` no longer has a lifetime parameter,
  `SerializeDriver::next` and `top_descriptor` on both states return
  `'static` descriptors.  Added `SerializeDriver::state_mut` to place
  extension values into the state before or during serialization.
- Fixed multiple soundness issues:
  - `DeserializeDriver::from_sink` now ties the sink to the driver's lifetime.
  - The deserialize driver now drops child sinks before it uses or drops
    their parent sinks.
  - `Sink::descriptor` now returns a `&'static dyn Descriptor` as the
    deserializer state holds on to descriptors while sinks are in use.
  - `OwnedSink::borrow` and `OwnedSink::borrow_mut` now return `&dyn Sink` and
    `&mut dyn Sink`.  `OwnedSink::take` drops the sink, after which it ignores
    all values.
  - The byte specialization for `Vec<u8>` and `[u8; N]` no longer relies on
    an unsafe hook on `Deserialize`.
- Extension types now need to be `Send` and `Sync`.
- `Option<T>` now also treats extension values that fall back to null as
  null.
- Added `deser_json::Deserializer::drive` to deserialize into custom sinks.
- Added `deser-location` which provides source locations.  Formats install a
  `SourceMap` and publish the byte offsets of every event into the
  deserializer state, `Spanned<T>` picks them up.  `deser-json` supports this
  with the `locations` feature and `DeserializerConfig::track_locations`.
- Improved the performance of `deser_path::PathSink`.
- Improved the performance of deserializer state extensions.
- Added `deser::de::Recording` to record values and replay them into sinks
  later.  Extensions in the deserializer state can be marked as replayable
  with `DeserializerState::set_replayable`, recordings capture and restore
  them per event.  `deser-location` and `deser-path` register their state.
- Added support for enums with data to the derive: newtype, tuple and struct
  variants in all representations known from serde (externally tagged,
  internally tagged with `#[deser(tag = "...")]`, adjacently tagged with
  `#[deser(tag = "...", content = "...")]` and `#[deser(untagged)]`), catch-all
  variants with `#[deser(other)]` and generic enums.
- Added `SinkHandle::shorten` and `Atom::as_borrowed` / `Event::as_borrowed`.
- Fixed `deser_path::PathSink` not removing path segments when leaving
  containers.
- Added an extensible data model.  `Atom::Ext` carries values implementing
  the new `deser::ext::Extension` trait which provide a fallback into the
  core data model for consumers that do not understand them.
- Added support for `u128` and `i128` via extension atoms.  `deser-json`
  serializes and parses them natively.
- Descriptors, `finish` and `is_optional` are now forwarded through `Option`,
  references and boxes.  `Box<dyn Serialize>` is now serializable.
- `deser-json` now serializes `f32` values with `f32` precision.
- Fixed integer range checks which accepted `u64::MAX` as `-1` for `i64`
  and `-1` as `u64::MAX` for `u64`.
- Removed number serialization support in JSON serializer.
- Fixed `Option<T>` silently dropping structs, vectors, maps and boxes.
- Fixed `HashMap` deserialization.
- Fixed deriving `Deserialize` for generic structs and newtypes.
- Fixed JSON serialization of `char` which emitted the code point.
- Fixed a panic in the JSON parser on trailing commas in arrays.
- `SinkHandle` is now an opaque type which implements `Sink`.  It is created
  with `SinkHandle::to`, `SinkHandle::boxed` and `SinkHandle::null`.
- Reduced the time spent on deserialization and serialization by about 40%.  Derived struct keys no longer allocate, `Option<T>` no longer
  allocates a wrapper sink, errors are boxed and the JSON parser and serializer
  scan strings and whitespace a word at a time.
- Added `DeserializerState::is_map_key`.  Integer sinks now accept
  stringified integers in map key position which enables integer keyed
  maps in JSON in both directions.
- Bytes are supported in JSON and TOML.  Formats without native bytes write
  them as base64 strings and types that expect bytes (`Vec<u8>`, `[u8; N]`
  and `Cow<[u8]>`) accept strings which are decoded as lenient base64 (both
  alphabets, optional padding) in addition to sequences of integers.  The
  new `deser::adapters::bytes` module has the encodings (`Base64`,
  `Base64Url`, `Hex` and more, base32 with the new `bytes-encoding` feature)
  and `BytesFormat`, which the serializer and deserializer configurations of
  `deser-json` and `deser-toml` accept with `bytes`.  Values can request a
  format with the new `Descriptor::bytes_format` which formats with native
  bytes (like CBOR) ignore.  The encodings are adapters which write strings
  in all formats (for instance `#[deser(as = Hex)]`) and the new
  `BytesFallback<F>` adapter keeps bytes in formats with native bytes and
  requests `F` otherwise (for instance `BytesFallback<Hex>` or
  `BytesFallback<IntSeq>` for sequences of integers).  Custom encodings
  implement `BytesEncoding`.
- Added support for more standard library types:
  - `str`, `CStr` and `Path` implement `Serialize` which makes `Box<str>`,
    `Arc<str>`, `Box<CStr>` and `Box<Path>` serializable.  `Box<str>`,
    `Arc<str>`, `Box<[T]>` and `Arc<[T]>` serialize and deserialize.
  - `Arc<T>` serializes and deserializes like `Box<T>`.  Shared values are
    serialized once per reference and deserialized into separate
    allocations.
  - `Cow<'a, T>` is supported for all `T: ToOwned` (for instance
    `Cow<Path>` and `Cow<[T]>`), deserialization goes through `T::Owned`.
    `String` and `Cow<str>` accept `Atom::Char`, `Cow<[u8]>` also accepts
    sequences of integers.
  - `VecDeque`, `LinkedList` and `BinaryHeap` serialize as sequences.
    `VecDeque<u8>` and `BinaryHeap<u8>` are bytes like `Vec<u8>`.
  - `PhantomData<T>` serializes as null and is optional.
  - `NonZero<T>` of all integer types (zero is rejected with
    `ErrorKind::OutOfRange`), `Wrapping<T>`, `Saturating<T>` and
    `Reverse<T>` serialize as the value they wrap.
  - `Result<T, E>` is externally tagged: `{"Ok": value}` or
    `{"Err": error}`.
  - `IpAddr`, `Ipv4Addr`, `Ipv6Addr`, `SocketAddr`, `SocketAddrV4` and
    `SocketAddrV6` are strings.
  - `PathBuf` and `Box<Path>` are strings.  Paths that are not valid UTF-8
    fail to serialize.
  - The atomic integers and `AtomicBool` serialize their value (loaded with
    relaxed ordering).
  - `CString` and `Box<CStr>` are bytes (without the nul terminator).
    Interior nul bytes fail to deserialize.
  - `HashSet<T, H>` serializes with custom hashers.
  - The new containers are adapters as well: `Arc<U>`, `Box<[U]>`,
    `Arc<[U]>`, `VecDeque<U>`, `LinkedList<U>`, `BinaryHeap<U>` and
    `Result<U, V>`.
- The `speedups` feature of the formats no longer exposes the optional
  dependencies as features: `simdutf8`, `itoa` and `ryu` cannot be enabled
  on their own anymore, enable `speedups` instead.
- Added `deser::Position` (offset, line and column) with `Position::of` and
  `Position::advance`, which counts positions the same way as errors do.
  `deser_location::Position` is a re-export of it and the spans of
  `deser-value` return it from `Span::start` and `Span::end`.
- The derive macros are no longer re-exported from `deser::derive` (which
  only holds their documentation), use `deser::Serialize` and
  `deser::Deserialize`.
- Added adapters on containers: `#[deser(as = Adapter)]` on structs, enums
  and unions forwards the derived implementations to the adapter instead of
  using the fields and variants, which also works for tuple structs, unit
  structs and unions.  This covers serde's `from`, `try_from` and `into`,
  for instance `#[deser(as = TryFromInto<String>)]`.  Attributes which have
  no effect because of the adapter are rejected, as are adapters that would
  use the type's own implementation (`_`, `Same` or the type itself).
- Added `serialize_as` and `deserialize_as` to use an adapter for one
  direction only, on containers (the other direction is derived as usual,
  for instance to validate values with `deserialize_as = TryFromInto<Raw>`
  while serializing the fields) and on fields.
- Flattened fields can hold values that forward to other values
  (`Chunk::Forward`) when serializing, for instance types with container
  adapters that serialize as a struct.
- `As` forwards the specialization for bytes to its adapter, so for
  instance a `Vec<As<u8, Same>>` serializes as bytes.

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
