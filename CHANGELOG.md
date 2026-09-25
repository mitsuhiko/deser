# Changelog

All notable changes to deser are documented here.

## Unreleased

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
  `State::event_mut` and read with `State::event`.  Formats attach data with
  `DeserializeDriver::emit_with`, during serialization `Serialize`
  implementations attach data to their first event and the serialize driver
  detaches it after the event was delivered.  Recordings capture event data
  automatically.  Source locations and the CBOR and YAML tags are now event
  data, `Locations` is no longer replayable.
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
  - `deser_json::Deserializer::new` now takes a `&str`.
- Extension types now need to be `Send` and `Sync`.
- `Option<T>` now also treats extension values that fall back to null as
  null.
- Added `deser_json::Deserializer::drive` to deserialize into custom sinks.
- Added `deser-location` which provides source locations.  Formats install a
  `SourceMap` and publish the byte offsets of every event into the
  deserializer state, `Spanned<T>` picks them up.  `deser-json` supports this
  with the `locations` feature and `Deserializer::track_locations`.
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
- Made `derive` a default feature.
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
