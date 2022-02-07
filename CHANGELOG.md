# Changelog

All notable changes to deser are documented here.

## 0.5.0

- Added support for `#[deser(default)]` in deriving.
- Added support for `#[deser(skip_serializing_optionals)]`.
- Removed `ignore` and replaced it with `SinkHandle::null`.
- Added tuple support.
- Added array support.

## 0.4.0

- Restructure serialization and deserialization to pass `Atom` values
  within `Event` and `Chunk`.  This changes the interface from invoking
  type specific methods on the sink to passing an entire `Atom` instead.
- Events are now passed by value rather than reference.
- Added basic support for `Option<T>`.
- Added basic support for deriving simple enums.
