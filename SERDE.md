# Serde Learnings

Deser has been built based on the experience with [Serde](https://serde.rs/).  It's
important to know that Deser does not want to replace Serde but it wants to try an
alternative design that addresses certain shortcomings identified in the current serde
design at the cost of some features.  This document wants to share some reasons
for why deser exists in the light of the already existing serde library.

Serde as it exists today has strong stability guarantees which make it a very
stable system to target, but has caused some limitations that are impossible to
resolve without a major revision.

## Self Describing Formats Only

Deser limits itself to self describing formats.  While it's absolutely possible to
implement non self-describing formats on top of deser it's not a defined goal.  The
reason for this is that using the same serialization trait for both self describing
and non self describing formats creates some challenges.

### Challenge 1: Automatic Sequence Deserialization

Serde for instance implements structs through the deriving feature for both maps
and sequences.  This means that the following structure:

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct Point {
    x: f32,
    y: f32,
}
```

Automatically implements the following two formats for JSON:

```json
[10.0, 20.0]
```

```json
{
    "x": 10.0,
    "y": 20.0
}
```

This can be a very surprising feature.  For instance if one uses Serde to
provide a public RESTful API in JSON format most developers are not aware that
they are secretly providing a secondary format in addition to objects in the
form of arrays.  This also poses a hazard as users who might be starting this
format could bypass application firewalls or start sending data in a format that
will break as new fields are added.

### Challenge 2: Runtime Deserialization Failures

Serde's desire to support both self-describing and non self-describing formats
creates the odd situation that an implementation of `Deserialize` might only
start failing at runtime once the user attempts to use it with a specific
format.  In particular because you cannot "inspect" a deserializer you don't
quite know if this will work or not.

This is for instance a challenge when using `bincode` where some types that use
certain features cannot be used with bincode.  For instance the `flatten` feature
in serde requires internal buffering which does not work with bincode.  As a
result if a type uses this feature, it will not work with `bincode`.

**Related issues:**

* [bincode: #[serde(flatten)] causes error SequenceMustHaveLength #245](https://github.com/bincode-org/bincode/issues/245)
* [bincode:  Support serializing to Vec<u8> with unknown seq/map length #167](https://github.com/bincode-org/bincode/issues/167)
* [postcard: #[serde(flatten)] causes serialization to fail #29](https://github.com/jamesmunns/postcard/issues/29)

### Challenge 3: Buffering Limitations

Serde requires falling back to internal buffering for a range of cases, some of
which might not need buffering.  This buffering however changes the behavior of
some serializers and deserializers.

The most trivial example is that `serde_json` is happy to deserialize an object
containing numeric string keys (`HashMap<u32, u32>`) under normal circumstances
in the form ``{"42": 23}`` but it fails to do so, when internal buffering is
used (it will report `invalid type: string "42", expected u32`).

**Related issues:**

* [serde: Internal buffering disrupts format-specific deserialization features #1183 ](https://github.com/serde-rs/serde/issues/1183)

## Internal Data Format

Serde is based on both a complex and limiting internal data format.  For
instance during serialization and deserialization serde makes a distinction
between `u8` and `u64` for instance.  While this is useful in a lot of cases,
it creates additional complexity for the serializers and deserializers and
results in not insignificant amounts of code bloat.  Additionally though this
data format is very hard to extend.  There has only been one extension to the
data format in serde 1.0 and that was the addition of `i128` and `u128` as
adding new values is a semver hazard.

This is currently unresolved in deser but it's a space that requires exploration.

## Mandatory Buffering

Serde currently requires mandatory internal buffering even to implement features
that do not necessarily require it.  For instance to support flattening with
`#[serde(flatten)]` it needs to buffer a part of the stream.

## Recursion for Serialization and Deserialization

Serde depends on recursion for serialization as well as deserialization. E very
level of nesting in your data means more stack usage until eventually you
overflow the stack. Some formats set a cap on nesting depth to prevent stack
overflows and just refuse to deserialize deeply nested data.
