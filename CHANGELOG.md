# Changelog

All notable changes to deser are documented here.

## Unreleased

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
