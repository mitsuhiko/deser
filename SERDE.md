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

Formats where all values are text (like query strings) are hit the hardest:
`limit=10` deserializes into a `u64` field until the struct is flattened or
becomes the variant of an internally tagged enum, then it fails with the same
error.

Deser represents text whose type the format cannot express (the keys of JSON
objects, the values of query strings) as lexical atoms which the sinks parse
(`Atom::Lexical`).  They are atoms like all others and retained when values
are buffered.

**Related issues:**

* [serde: Internal buffering disrupts format-specific deserialization features #1183 ](https://github.com/serde-rs/serde/issues/1183)
* [serde_urlencoded: using `#[serde(flatten)]` breaks deserializing #33](https://github.com/nox/serde_urlencoded/issues/33)
* [serde_qs: Improper deserialization for #[serde(flatten)] field #159](https://github.com/samscott89/serde_qs/issues/159)

## Internal Data Format

Serde is based on both a complex and limiting internal data format.  For
instance during serialization and deserialization serde makes a distinction
between `u8` and `u64` for instance.  While this is useful in a lot of cases,
it creates additional complexity for the serializers and deserializers and
results in not insignificant amounts of code bloat.  Additionally though this
data format is very hard to extend.  There has only been one extension to the
data format in serde 1.0 and that was the addition of `i128` and `u128` as
adding new values is a semver hazard.

Deser addresses this by keeping the core data model small and allowing it to be
extended with arbitrary types through extension atoms.  Every extension value
carries a fallback into the core data model so that serializers and
deserializers which do not know about an extension can still process it.  This
is also how `u128` and `i128` are supported, without having to extend the core
data model.

The one width that the core data model keeps is the precision of floats.  An
`f32` widened to `f64` is the same value, but not the same text: `0.1f32` would
be written as `0.10000000149011612`.  Integers do not have this problem, their
text does not depend on their width.  Sinks that do not care receive `f32`
values as `f64`, the same way extension values fall back.

## Mandatory Buffering

Serde currently requires mandatory internal buffering even to implement features
that do not necessarily require it.  For instance to support flattening with
`#[serde(flatten)]` it needs to buffer a part of the stream.

## Recursion for Serialization and Deserialization

Serde depends on recursion for serialization as well as deserialization. E very
level of nesting in your data means more stack usage until eventually you
overflow the stack. Some formats set a cap on nesting depth to prevent stack
overflows and just refuse to deserialize deeply nested data.

## Composable Field Customization

Serde customizes fields with `#[serde(with = "module")]` and
`#[serde(deserialize_with = "function")]`.  Functions cannot be passed as type
parameters, so this does not compose: a custom deserializer for `T` cannot be
used for an `Option<T>`, a `Vec<T>` or the values of a map without writing
another function.  Using `deserialize_with` on an optional field also makes the
field required unless `#[serde(default)]` is added as well.

Deser's `Deserialize` trait never takes `self`, it creates a sink for a slot.
This makes it possible to express customizations as adapter types
(`SerializeAs` and `DeserializeAs`) that create sinks for slots of other
types.  The standard containers are adapters for the same containers holding
other types, so `#[deser(as = Option<Vec<DisplayFromStr>>)]` works without
extra code, and missing fields are handled by the adapter.

**Related issues:**

* [serde: Using de/serialize_with inside of an Option, Map, Vec #723](https://github.com/serde-rs/serde/issues/723)

## Catch-All Variants

Serde's `#[serde(other)]` only supports unit variants and does not work for
the map form of externally tagged enums.  The unknown tag is lost which means
that values do not round trip.

In deser the `#[deser(other)]` variant can be any variant.  A field marked
with `#[deser(tag)]` receives the unknown tag (which does not need to be a
string) and is used as tag when serializing, the remaining fields receive the
content.  Together with `Recording` as raw value, unknown variants can be
passed through losslessly.  `#[deser(default)]` marks the variant used when
the tag is missing.

**Related issues:**

* [serde: Tagged enums should support #[serde(other)] #912](https://github.com/serde-rs/serde/issues/912)

## Unknown Fields

Serde's `#[serde(deny_unknown_fields)]` is decided by every struct on its
own.  A flattened struct does not know which keys the struct it's flattened
into takes, which is why `deny_unknown_fields` does not work together with
`flatten` and why the tag of internally tagged enums shows up as an unknown
field.

In deser only the struct a key is given to decides if it's unknown, after
asking its flattened fields if they take it.  Internally tagged enums
report the keys they buffered until the tag was known but which the
variant does not use back to that struct.  In addition to the attribute
there is a policy (`UnknownFields`) that applies to all structs of a
deserialization and can collect unknown keys with their location and path
instead of rejecting them.

**Related issues:**

* [serde: Combination of flattened internally-tagged enum and deny_unknown_fields results in unsatisfiable requirements #1358](https://github.com/serde-rs/serde/issues/1358)
* [serde: Structs with nested flattens cannot be deserialized if deny_unknown_fields is set #1547](https://github.com/serde-rs/serde/issues/1547)
* [serde: Struct with `tag` and `deny_unknown_fields` cannot deserialize #2666](https://github.com/serde-rs/serde/issues/2666)
* [serde: `#![serde(deny_unknown_fields)]` does not work as expected on unit variants of tagged enum #2294](https://github.com/serde-rs/serde/issues/2294)

## Validation

Serde has no validation hooks, values are validated with `try_from` (which
requires a second type) or after deserialization, where the location of the
value in the input is lost.  Deser has `#[deser(validate = path)]` on
fields and types.  The validator runs when the value is complete and its
errors point at the value (location and path), like the errors of the
format.

**Related issues:**

* [serde: Support a #[serde(validate = "some_function")] attribute on fields #939](https://github.com/serde-rs/serde/issues/939)
* [serde: Add finalizer attribute hook to validate a deserialized structure #642](https://github.com/serde-rs/serde/issues/642)

## Tags That Are Not Strings

In serde the names of variants are `&'static str` which is why protocols
with integer or boolean tags (`{"version": 1, ...}`) need hand written
implementations.  In deser variants can be named by integers and booleans
(`#[deser(rename = 1)]`).  They are written as integers and booleans and
compared by type, except for text of unknown type (such as query strings)
which is parsed like the tag it's compared with.

**Related issues:**

* [serde: Allow integer tags for internally tagged enums #745](https://github.com/serde-rs/serde/issues/745)
* [serde: Allow integers (or custom types) to be used as names/keys #1773](https://github.com/serde-rs/serde/issues/1773)

## Names

Serde only takes string literals as names, which is why names cannot be
shared through constants or built with `concat!`.  Deser takes paths to
constants and macro invocations as well, also for the tag and content keys
of tagged enums.  `#[deser(alias_all = "...")]` adds aliases in a name style
to all fields or variants, for instance to read data written with a
different convention.  The tag and content keys can have aliases too
(`#[deser(tag_alias = "...")]`, `#[deser(content_alias = "...")]`).

**Related issues:**

* [serde: Add alias_all attribute for containers similar to rename_all #1530](https://github.com/serde-rs/serde/issues/1530)
* [serde: Allow rename of container with &'static str #2485](https://github.com/serde-rs/serde/issues/2485)
* [serde: Rename With Expressions #1964](https://github.com/serde-rs/serde/issues/1964)
* [serde: Consider supporting concat! macro in attributes #1636](https://github.com/serde-rs/serde/issues/1636)
* [serde: Enum tag alias #2324](https://github.com/serde-rs/serde/issues/2324)

## Skipping and Required Fields

Serde infers `T: Default` for type parameters of skipped fields, even if
the field is an `Option<T>` which has a default for any `T`.  Deser
requires the field type to have a default (`Option<T>: Default`) and type
parameters that only appear in skipped fields need neither `Serialize` nor
`Deserialize`.  This also applies to skipped fields of enum variants and
to variants that are skipped as a whole.

Optional fields (`Option<T>` and other types with a value for missing
fields) can be made required with `#[deser(required)]`: `null` is still
accepted, a missing key is not.

**Related issues:**

* [serde: #[serde(skip_deserializing)] for Option<T> wrongly requires T to implement Default #2759](https://github.com/serde-rs/serde/issues/2759)
* [serde: `Option<T>` defaults to `None` when missing fields #2753](https://github.com/serde-rs/serde/issues/2753)

## Error Messages

Serde's errors for enums lose information: unknown variants do not name
the enum, and enums with only unit variants that receive a value of the
wrong type report `expected value`.  Deser names the enum in errors about
unknown variants (``unknown variant `D` of Kind, expected `A` or `B` ``)
and reports the type of the value and the enum otherwise (`unexpected
float, expected Level`).

**Related issues:**

* [serde: More descriptive unknown variant deserialize error #1481](https://github.com/serde-rs/serde/issues/1481)
* [serde: Confusing error message when deserializing a simple enum from JSON #2702](https://github.com/serde-rs/serde/issues/2702)

## Standard Library Types

Deser implements `Serialize` and `Deserialize` for some types that serde
does not support: `ManuallyDrop` (like its value), `OnceLock` (like an
`Option`) and `Infallible` (which cannot be deserialized, useful for enums
with impossible variants).

**Related issues:**

* [serde: impl Serialize and Deserialize for ManuallyDrop #1507](https://github.com/serde-rs/serde/issues/1507)
* [serde: impl Serialize for OnceCell #1952](https://github.com/serde-rs/serde/issues/1952)
* [serde: Implement Serialize and Deserialize for core::convert::Infaillible #2740](https://github.com/serde-rs/serde/issues/2740)

## Updating Values

Serde has a hidden `deserialize_in_place` which reuses allocations but
replaces the whole value.  Deser has `Deserialize::deserialize_update`
which applies data on top of an existing value: derived structs update the
fields that are given and keep the others, nested structs (also flattened
ones), options, boxes and maps are merged.  This is useful to layer
configuration files over defaults.

**Related issues:**

* [serde: Consider unhiding `deserialize_in_place` #2204](https://github.com/serde-rs/serde/issues/2204)
