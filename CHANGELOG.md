# Changelog

All notable changes to deser are documented here.

## Unreleased

- **Breaking:** `deser-jsonc`, `deser-json5` and `deser-hj` no longer
  depend on `deser-json`.  Their serializers are generated from the same
  template as their parsers, so `Serializer`, `SerializerConfig`,
  `Indent`, `InlinePolicy` and `Trailing` are types of their own instead of
  re-exports of `deser-json`.  The serializers of `deser-jsonc` and
  `deser-json5` write their own raw values (`RawJsonc`, `RawJson5`) as they
  are, `RawJson` values are encoded like values of other formats.
- **Breaking:** `State::take_raw_request` takes the `RawFormatId` of the
  format and discards requests for raw values of other formats.  This
  fixes a soundness issue: as the declared raw format can be changed by
  everything with access to the state, parsers could emit their input
  with the description of another format, for instance MessagePack or
  CBOR bytes that are not UTF-8 as raw JSON, which the JSON serializer
  and `RawJson::get` then handed out as `str`.
- Fixed a use after free when a flattened value forwards to a value
  which forwards again and the second forwarded value borrows from the
  first one: the forwarded values are now dropped from the last to the
  first.
- Fixed the order in which sinks and emitters are dropped when the drop
  of one of them panics: the drivers drop the others from the innermost
  to the outermost too, so values that borrow from the ones below them
  are dropped first.
- Fixed a panic ("chunk too small") when allocating sinks and emitters
  larger than the chunks of the arena whose size is not a multiple of
  eight.
- Fixed a leak of the field values of derived structs that receive
  another value after they finished (the driver delivers a second value
  to its root sink) or whose `finish` panics.
- The arena now parks its largest chunk for the next deserialization or
  serialization (up to 1 MiB) instead of the first one, so large
  documents do not grow it again every time.
- `deser-env`: the serializer rejects names that do not split into their
  keys again (a key ending in `_` before the separator `__`) and
  different keys with the same name (like `a` and `A`), and a
  `max_depth` of `usize::MAX` no longer overflows.
- `deser-urlencoded`: fixed a panic when a key-value pair at the top
  level has no value.  The serializer rejects keys that the deserializer
  would split differently into nested keys (like the key `a[b]`, or `[]`
  in a sequence with `ArrayFormat::Brackets`) and empty nested keys.
- `deser-csv`: the stream deserializer rejects UTF-16 input also if the
  first chunk it receives is a single byte.
- `deser-yaml`: the stream deserializer reports errors in what follows the
  last document (like directives without a document or comments that are
  not UTF-8) instead of ignoring it, like the deserializer does.

## 0.10.0

- **Breaking:** the APIs follow the same conventions everywhere: values
  are changed with setters (`set_x(&mut self, ...)`) and read with
  getters, there are no methods that return a changed copy anymore.
  Types that are configured in one expression have a separate builder.
  * The configurations of the formats (`DeserializerConfig` and
    `SerializerConfig`) have setters (`config.set_trailing(Trailing::Newline)`)
    and builders: `DeserializerConfig::builder().trailing(Trailing::Newline).build()`
    (also in constants) and `config.into_builder()` to start from an
    existing configuration.  The same goes for `Limits`
    (`Limits::builder().max_depth(64).build()`).
  * `ContainerShape`: `set_len`, `set_len_hint`, `set_order` and
    `set_multimap` replace the `with_` methods, `ContainerShape::with_len`
    and `ContainerShape::with_order` are constructors.
  * `Error`: `set_offset`, `set_position`, `set_attachment` and
    `set_source` replace the `with_` methods, `resolve_position` changes
    the error in place, `Error::with_offset` and `Error::with_position` are
    constructors.  `ErrorContext::add_context` and
    `State::attach_error_context` change the error in place.
  * `Context::with` is a constructor, values are added with
    `Context::set` (was `insert`).  `CollectErrors::with_max_errors` and
    `CollectErrors::set_max_errors`, `Validation::set_max_errors`,
    `LexicalRules::set_lenient_bools` and `set_empty_is_null` (with
    getters), `RawFormatInfo::set_data`.  `Bytes::with_fallback` was
    removed, the field `fallback` is public.
  * `deser-value`: the `with_` methods of `Map`, `Seq` and `Value` were
    removed, they have setters.
  * `State::set_collect_errors` no longer returns the previous setting,
    it's returned by `State::collect_errors`.  `State::set_raw_format` was
    renamed to `State::declare_raw_format` as it's not a setter.
- Added `Context`: configuration (typed values) that is given to
  serializations and deserializations from the outside, created once and
  shared.  Its values are the defaults of the extension values of the
  `State` (`State::get` returns the value of the state or the one of the
  context).  The drivers, `io::Reader`, `io::Writer`,
  `stream::InputBuffer`, the readers, writers and codec of `deser-tokio`
  and the deserializer and serializer of `deser-value` have `set_context`
  and `context`.  The deserializer and serializer configurations of all
  formats hold a context (`set_context` and `context` on the
  configurations and `context` on their builders), so `config.from_str(...)`,
  `config.to_string(...)` and the deserializers, serializers, readers and
  writers created from a configuration use it.  CBOR and MessagePack
  gained a `DeserializerConfigBuilder` and TOML a `SerializerConfigBuilder`
  for this.  Stream deserializers report the
  context of their configuration (`StreamDeserializer::context`), which
  the buffers and readers start with.  A context set on the driver (for
  instance in the setup callback of `deserialize_with`) takes precedence
  over the one of the deserializer or serializer, which adds the values
  of the types the context of the driver has no value for (see
  `DeserializeDriver::set_default_context` and
  `SerializeDriver::set_default_context`).  Contexts are equal if they
  share their values.
- **Breaking:** the deserializers, serializers and stream deserializers
  of the formats take their configuration by value instead of by
  reference (`Deserializer::from_str_with_config(input, config)`,
  `Serializer::with_config(config)`, `StreamDeserializer::with_config(config)`
  and the other constructors that take a configuration).
- **Breaking:** configuration given from the outside moved into the
  context.  The `bytes` options of the deserializer and serializer
  configurations of the formats (JSON, JSONC, JSON5, Hjson, TOML, YAML,
  XML, CSV, query strings and environment variables) and the
  `duplicate_keys` options (XML, query strings and environment variables)
  were removed: put a `BytesFormat` or a `DuplicateKeys` policy into the
  context instead, the same context configures writing and reading.  The
  serializers write bytes in the `BytesFormat` of the state (or context).
  Formats with other defaults apply them unless the context has a value
  (`State::set_default`, `DuplicateKeys::set_default` and
  `LexicalRules::set_default`), query strings and environment variables
  still use the last of repeated keys by default.  The lenient
  `LexicalRules` of query strings, environment variables, CSV, XML and
  OpenStep property lists are such defaults now, `LexicalRules` in the
  context override them.
- **Breaking:** `Limits` moved into the context: it's no longer a layer
  but a value of the context, which the `DeserializeDriver` enforces with
  a layer of its own after all other layers (so that the errors of the
  limits get the path of `deser-path`).  Use
  `Context::with(Limits::builder().max_depth(64).build())` instead of
  `driver.push_layer(Limits::builder().max_depth(64).build())`.  `Limits`
  has getters and `into_builder`.
- **Breaking:** location tracking is requested with `TrackLocations(true)`
  in the context instead of the `track_locations` options of the
  deserializer configurations (JSON, JSONC, JSON5, Hjson, TOML, YAML, XML,
  CSV, query strings and property lists).  XML no longer tracks locations
  by default.  The deserializer configurations of TOML and property lists
  have no other options than their context anymore.
- Added `CollectErrors`, a value of the context that makes the whole
  deserialization collect errors (like `State::set_collect_errors`, with
  an optional limit).
- Added open enums (behind the `open-enums` feature, off by default): a
  trait marked with `#[deser::open_enum]` is an enum whose variants are
  the implementations marked with `#[deser::variant]`, which can be in any
  crate.  `Box<dyn Trait>` and `Arc<dyn Trait>` are serialized and
  deserialized like the enums of the derive (externally, internally or
  adjacently tagged or untagged, with `rename_all`, `alias_all`, aliases
  and names that are not strings).  The variants are registered explicitly
  in an `OpenEnums` registry which is given to deserializations in the
  context, the variants of untagged open enums are tried in the order they
  are registered.
  Registering two variants with the same name and deserializing without
  registry fail with the new `ErrorKind::Configuration` (in the `Usage`
  category).
  `Arc<T>` is deserialized through a hidden trait (`de::DeserializeArc`)
  so that it works for the trait objects of open enums.
- Newtype variants of internally tagged enums can contain unit structs,
  they are the tag alone (`{"type": "A"}`) like with serde.  Serializing
  them was an error.  Whether the content is a unit struct is decided by
  its description (`Describe::unit_struct`), so types implemented by hand
  can be unit structs as well.
- Added `Deserialize::describe_type`, the counterpart of
  `Serialize::describe` for what is known about a type without a value.
  The derive describes unit structs, wrappers like `Box<T>` and adapters
  forward the description of their value.
- **Breaking:** merged `DeserializeAs` into `Deserialize`, which has a
  type parameter for the type of the value: `Deserialize<'de, T = Self>`.
  Adapters implement `Deserialize<'de, T>` with the methods of
  `Deserialize` (`deserialize_into_as`, `initial_value_as` and
  `deserialize_update_as` lost their suffix).  The containers implement it
  once for their adapters (`Vec<A>` for `Vec<T>`, which includes `Vec<T>`
  itself), so containers with adapters now update in place like the ones
  without.  As a type can deserialize other types,
  `Deserialize::deserialize_into(&mut slot, state)` needs the type
  (`String::deserialize_into(&mut slot, state)`).  The map and set
  adapters (like `HashMap<KA, VA>`) accept maps with any hasher.
- **Breaking:** merged `SerializeAs` into `Serialize`, which has a type
  parameter for the type of the value as well: `Serialize<T: ?Sized =
  Self>`.  Its methods take the value instead of `self`:
  `fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error>`
  (and `value.serialize(state)` is `T::serialize(value, state)`).
- **Breaking:** renamed `deser::ser::Chunk` to `deser::ser::Emit` (and
  `Begin::chunk` to `Begin::emit`), as it describes how a value is
  emitted rather than a piece of data.  The variants and the
  constructors (`Emit::seq`, `Emit::map` and `Emit::structure`) did not
  change.
- **Breaking:** as `Serialize` cannot be a trait object anymore, values
  are passed on as `deser::ser::SerializeRef` instead of
  `&dyn Serialize` (created with `SerializeRef::new(&value)` or
  `SerializeRef::serialize_as::<A, _>(&value)`), for instance to
  `EventSink::event`, `Next::value`, `SerializeDriver::next` and
  `FlattenedStruct::new`.  `SerializeHandle` is opaque (convert from a
  `SerializeRef` and back with `SerializeHandle::get`).  The functions
  which took `&dyn Serialize` (like `to_string`, `to_writer`,
  `Serializer::serialize` and `Writer::write`) are generic, which also
  allows unsized values (`to_string("x")`).  `Serializer::serialize_ref`
  and `SerializeDriver::from_ref` take a `SerializeRef`.
- **Breaking:** `BytesEncoding` requires `Send + Sync`.
- **Breaking:** renamed `StreamDeserializer::supports_feed` and
  `StreamDeserializer::feed` to `supports_partial` and `drive_partial`
  (and the same methods of `InputBuffer`), matching
  `StreamSerializer::supports_partial` and `drive_partial`.
- **Breaking:** renamed `OwnedSink::borrow` and `OwnedSink::borrow_mut`
  to `get` and `get_mut`.
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
- **Breaking:** removed `make_slot_wrapper!` and `deser::de::SlotWrapper`.
  Values that are deserialized from atoms implement
  `Deserialize::deserialize_atom` (and `Deserialize::expecting`) instead
  of a sink:
  `deserialize_into` has a default implementation which returns the slot
  as sink (a `deser::de::Slot`, which dereferences to the `Option<T>`),
  so no macro, no sink and no `deserialize_into` are needed.
  `deserialize_borrowed_atom` receives borrowed atoms.
- **Breaking:** `ErrorKind` is `#[non_exhaustive]` and replaced
  `Unexpected` with precise kinds: `Syntax` (input that is not
  well-formed), `LimitExceeded`, `InvalidType`, `InvalidValue`,
  `UnknownField`, `UnknownVariant`, `DuplicateKey`, `InvalidState` (APIs
  used in an unsupported way) and `Custom`.  The kind is part of the
  `Display` output (like `InvalidType: unexpected string, expected u16`).
  Values which cannot borrow from the input fail with `UnsupportedType`,
  integers and floats which TOML cannot represent with `Syntax`.  #46
- Added `Error::category` and `ErrorCategory`, which tell apart input
  that is not well-formed (`Syntax`, `Eof`) from input that does not fit
  the values (`Data`), for instance to answer HTTP requests with 400 or
  422.  The category follows from the kind, errors of the `Custom` kind
  are in the `Data` category if a value failed with them and in the
  `Syntax` category if the format did.  #46
- Added `ContainerShape::cautious_capacity`, the number of elements to
  preallocate for the length that the input declares, capped at about a
  megabyte.  The standard containers and `deser_value::Value` use it, so
  input cannot request large allocations it does not fill.
- Added `ContainerShape::with_len_hint`, an estimate of the number of
  elements for formats that cannot know it upfront.  It's only used to
  preallocate (`cautious_capacity`), `len` remains unknown so serializers
  that write lengths do not rely on it.  `deser-csv` estimates the number
  of records from the first one.
- Added `Deserialize::expecting`, what a value expects in error messages
  (like `Sink::expecting` of its sink, which reports it).  The derive
  implements it with the name of the type (or `#[deser(expecting)]`),
  wrappers like `Option<T>` forward it, the default is the name of the
  type.  The maps that `MapSkipError` deserializes report their type
  instead of `compatible type`.
- **Breaking:** replaced `Sink::unexpected_atom` with the function
  `deser::de::default_atom(sink, atom, state)`, the default handling of
  atoms that sinks (and `deserialize_atom`, with the slot as sink) pass the
  atoms they do not accept to.  Overriding the method had no effect (the
  driver does not invoke it).
- **Breaking:** the value type of `Deserialize<'de, T>` has to be `Send`
  (`T: Send`), which it had to be in practice already.  Generic adapters
  need the bound (`impl<'de, T: Send, A: Deserialize<'de, T>>`).
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
  `Atom::widen_float` (sinks receive floats as `F64` through the default
  handling of atoms) and `OwnedDriver::driver` (use
  `OwnedDriver::with`).
- **Breaking:** removed `LexicalRules::lenient_bools` and
  `LexicalRules::empty_is_null`.  Types that want to honor the rules pass
  lexical atoms on to the types of deser (like `bool` or `Option<T>`).
- **Breaking:** removed `BigInt::from_i128` and `BigInt::from_u128`, use
  `BigInt::from`.
- **Breaking:** renamed `Number::into_static` to `Number::into_owned`,
  like `Raw::into_owned` and `RawInput::into_owned`.
- **Breaking:** the `tag` modules of `deser-cbor` and `deser-yaml` are
  private, their items are available at the root of the crates
  (`deser_cbor::Tagged`, `deser_yaml::take_tag`, ...).
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
  them with the new
  `deser::ext::{Raw, RawFormat, RawFormatId, RawFormatInfo, RawInput}`,
  `State::declare_raw_format`, `State::take_raw_request` and
  `Error::is_raw_request`.  Formats declare their raw values with a
  `RawFormatId` and get the `RawFormatInfo` (with the functions of the
  format) from the request, so programs that do not use raw values do
  not contain them (like the JSON parser in a program that only writes
  JSON).  See the new `raw-values` example.
- Added `deser::de::deserialize_value` to implement functions like
  `from_str` of formats so that only the code that depends on the type
  of the value exists once per type.  The formats no longer use hidden
  helpers of `deser-core`.
- Added `DeserializeDriver::multimap_value` and
  `deser::de::missing_multimap_value` for formats that read a single
  value of a key of a multimap (like `deser_env::var`): collections take
  the value as their only item and are empty if the key is missing.
- `deser-xml`: added `DeserializerConfig::bytes` so that bytes written
  in another format than base64 (with `SerializerConfig::bytes`) can be
  read back.
- `deser-xml`: added `Deserializer::config` like the deserializers of
  the other formats have.
- Added `ContainerShape::set_ambiguous_empty` for empty containers that
  could just as well be the other kind of container (like the empty array
  of PHP, which is an empty list and an empty map).  Values that reject
  such a container receive an empty container of the other kind instead.
  Dynamic values of `deser-value` keep the flag (`Seq::is_ambiguous_empty`
  and `Map::is_ambiguous_empty`) and serde types used through
  `deser-serde` receive the kind they ask for.
- Added `deser-php`: PHP's serialization format (`serialize` and
  `unserialize`) with classes and property visibility as event data
  (`Object<T>`), enum cases and custom serialized objects.  Empty arrays
  deserialize into sequences and maps.  References (`r:` and `R:`) are
  passed through as `Reference` markers and are not resolved.  Tested
  against PHP with inputs from php-src.
- Added `deser-pickle`: Python's pickle format (protocols 0 to 5 are read,
  2 to 5 written).  Pickles are run without importing or calling
  anything: objects are emitted as their state, items or arguments with
  their class and the form they are created in as event data
  (`Object<T>`), classes and functions are `Global`s.  Values that are
  reached more than once are emitted at every place with an id as event
  data, values that contain themselves are cut with `Reference`s (which
  are `null` to types that do not know them).  The serializer writes
  shared values once and refers to them, so cycles survive a round trip
  through `deser_value::Value`.  Tested against CPython with inputs from
  its pickle tests.
- Added `deser-ini`: INI files.  As INI has no specification, the default
  dialect follows a survey of INI files on GitHub: `;` and `#` comments
  (also after values, after whitespace), `=` and `:` delimiters, values
  continued on indented lines, quoted values and keys without values.
  Comments after values, delimiters, continuation lines, quotes, keys
  without values and lowercased names can be configured, with presets for
  Python's `configparser` and for git's config files (`Syntax::Git`, with
  subsections, escapes and git's quoting).  Files are multimaps like query
  strings: repeated keys are collected by collections, sections that repeat
  are merged.  Tested against inih, Python's `configparser` and git with a
  corpus of the test inputs of INI parsers and real world files
  (`scripts/update-ini-test-data.sh`).
- `deser-json`: added `SerializerConfig::non_finite_floats` which writes
  NaN and infinite floats as `NaN`, `Infinity` and `-Infinity` instead of
  `null`.  `deser_json5::to_string` and `deser_json5::to_writer` enable
  it, so these floats roundtrip through JSON5.
- `deser-csv`: reading is 17% faster and writing twice as fast.  Fields
  are found with masks of the special characters of 64 bytes (with SIMD
  on aarch64 and x86_64), floats are formatted with `zmij` and numbers,
  short fields and the names of columns are written and compared without
  `memcpy` and `memcmp`.  Empty fields of optional numbers and booleans
  no longer create an error that is thrown away, and derived structs do
  not ask every field of a record if it collects repeated columns.
- **Breaking:** the formats always format floats with `zmij`, the `zmij`
  feature and the fallback for builds without it are gone.  `deser-env`,
  `deser-ini`, `deser-plist`, `deser-urlencoded` and `deser-xml` format
  floats with `zmij` as well, the output is the same as before.
  `deser-json`, `deser-toml`, `deser-yaml` and `deser-csv` format
  integers with `itoa` instead of a copy of the same formatter each.
- The `speedups` feature is enabled by default and exists in every format
  that writes floats as text or validates UTF-8.  It validates UTF-8 with
  `simdutf8` in `deser-json` and its dialects, `deser-toml`, `deser-yaml`,
  `deser-cbor` and `deser-msgpack`, in the other formats it has no effect
  yet.

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
