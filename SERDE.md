# Serde Learnings

Deser has been built based on the experience with [Serde](https://serde.rs/).
Deser does not want to replace Serde.  It tries an alternative design that
addresses shortcomings of the current serde design, at the cost of some
features.  This document explains why deser exists alongside serde.

Serde has strong stability guarantees which make it a very stable system to
target.  The flip side is that some of its limitations cannot be resolved
without a major revision.  Most of the problems below are consequences of three
decisions: one set of traits for self describing and non self describing
formats, a fixed data model that is lossy when values have to be buffered, and
recursion through the call stack.  Many of the related issues have been open for
years.

Unless marked otherwise, all issues linked below were open when this document
was last updated (September 2026).  They are listed as a record of real problems
people ran into, not as criticism of serde.  Some are feature requests which
serde could accept, but most of them are hard to fix without changing the data
model.

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
* [serde: Feedback request: How to handle unsupported "self-describing-only" attributes #2674](https://github.com/serde-rs/serde/issues/2674)

## Buffering Loses Information

Serde has to buffer values for internally tagged enums (the tag might not
come first), for untagged enums (every variant is tried) and for
`#[serde(flatten)]`.  The buffer (serde's private `Content` type) is a copy
of the data model, and everything that is not part of the data model is
lost in it.  This changes the behavior of serializers and deserializers as
soon as a type is flattened or becomes the variant of an enum.

The most trivial example is that `serde_json` is happy to deserialize an object
containing numeric string keys (`HashMap<u32, u32>`) under normal circumstances
in the form ``{"42": 23}`` but it fails to do so, when internal buffering is
used (it will report `invalid type: string "42", expected u32`).  The same
happens to:

* 128 bit integers, which the buffer does not support in all cases and
  which serde_json only delivers to types that ask for them.
* Numbers with `arbitrary_precision`, which serde_json passes as a map with
  a magic key (see [in-band signalling](#internal-data-format)).  Once
  buffered, a float field reports `invalid type: map, expected f64`.
* Formats where all values are text (like query strings) are hit the
  hardest: `limit=10` deserializes into a `u64` field until the struct is
  flattened or becomes the variant of an internally tagged enum, then it
  fails with the same error.
* Source locations: errors in buffered values point at the wrong place or
  have no location at all.

Deser does not flatten through a buffer at all (see below).  Where it has
to buffer, values are recorded as events together with the state the
format published for them (such as the source location) and replayed as
such.  Text whose type the format cannot express (the keys of JSON objects,
the values of query strings) is a lexical atom which the sinks parse
(`Atom::Lexical`), 128 bit integers and exact numbers are extension values
with a fallback.  They are atoms like all others and retained when values
are buffered.

**Related issues:**

* [serde: Internal buffering disrupts format-specific deserialization features #1183](https://github.com/serde-rs/serde/issues/1183)
* [serde: Unexpected behaviour with untagged enums and numeric keys #2724](https://github.com/serde-rs/serde/issues/2724)
* [serde: Untagged enum does not play nice with u128 #1682](https://github.com/serde-rs/serde/issues/1682)
* [serde: ContentDeserializer does not support 128-bit integers #2576](https://github.com/serde-rs/serde/issues/2576)
* [serde: Flattened nested structs throw deserialize error "invalid type: map, expected f64" when inner type contains a float #2748](https://github.com/serde-rs/serde/issues/2748)
* [serde: Failed to deserialize tagged enum when inner struct has a number with trailing zeros #2903](https://github.com/serde-rs/serde/issues/2903)
* [serde_json: The `#[serde(flatten)]` syntax is not supported with the feature `arbitrary_precision` #721](https://github.com/serde-rs/json/issues/721)
* [serde_json: `arbitrary_precision` breaks float deserialisation in untagged enum #1108](https://github.com/serde-rs/json/issues/1108)
* [serde_json: u128 is not supported with `#[serde(flatten)]` #625](https://github.com/serde-rs/json/issues/625)
* [serde_json: Deserialization of 128 bit integers fail when used with untagged variants #740](https://github.com/serde-rs/json/issues/740)
* [serde_json: Bug: f64 within flattened HashMap throws error on deserialization #1157](https://github.com/serde-rs/json/issues/1157)
* [serde_json: Flatten causes maps with integer keys to fail deserialization #989](https://github.com/serde-rs/json/issues/989)
* [serde_json: Bug: untagged union fails to deserialize hashmap with usize as keys #1103](https://github.com/serde-rs/json/issues/1103)
* [serde_json: Error message points to wrong line when using attribute flatten #622](https://github.com/serde-rs/json/issues/622)
* [serde_json: Incorrect line/column info when using tag setting with nested enums #870](https://github.com/serde-rs/json/issues/870)
* [serde_urlencoded: using `#[serde(flatten)]` breaks deserializing #33](https://github.com/nox/serde_urlencoded/issues/33)
* [serde_qs: Improper deserialization for #[serde(flatten)] field #159](https://github.com/samscott89/serde_qs/issues/159)

## Flattening

To support `#[serde(flatten)]` serde buffers all keys the struct does not
know and then deserializes the flattened fields from that buffer.  Besides
the information that gets lost (see above) this means that allocations
are needed where none would be necessary, and that the flattened fields do
not know about each other: a flattened map after a flattened struct
receives all keys (including the ones the struct took), and the keys a
flattened enum used show up again in other flattened fields.

Deser's sinks have native support for flattening
(`Sink::value_for_key`): a struct asks its flattened fields for every key
it does not take itself, and the first one that takes it gets the value.
No buffering is required.  This also means:

* A flattened map receives exactly the keys that nobody else took.
* A flattened `Option` is `None` if its value took no key.  If it took some
  keys, errors (such as missing fields) are reported instead of turning
  the value into `None`, and duplicate keys are detected.
* Attributes that would be silently ignored on a flattened field (such as
  `default`) are rejected by the derive.

**Related issues:**

* [serde: Avoid lossy buffering in #[serde(flatten)] #2186](https://github.com/serde-rs/serde/issues/2186)
* [serde: Zero-alloc #[serde(flatten)] deserialization appears to be possible - should we finish the work? #2363](https://github.com/serde-rs/serde/issues/2363)
* [serde: Flattening an Enum doesn't "consume" the fields #2200](https://github.com/serde-rs/serde/issues/2200)
* [serde: #[serde(flatten)] on BTreeMap<String, Value> does not capture unknown/remaining fields #2176](https://github.com/serde-rs/serde/issues/2176)
* [serde: Unexpected interaction of Option/flatten with duplicate key checking #2416](https://github.com/serde-rs/serde/issues/2416)
* [serde: Feature request: #[flatten] on Option<SomeStruct> should raise error when only some fields are set #2793](https://github.com/serde-rs/serde/issues/2793)
* [serde_json: A flattened Option masks parse errors inside the Option #644](https://github.com/serde-rs/json/issues/644)
* [serde: default is ignored with flatten #2707](https://github.com/serde-rs/serde/issues/2707)

## Internal Data Format

Serde is based on both a complex and limiting internal data format.  For
instance during serialization and deserialization serde makes a distinction
between `u8` and `u64` for instance.  While this is useful in a lot of cases,
it creates additional complexity for the serializers and deserializers and
results in not insignificant amounts of code bloat.  Additionally though this
data format is very hard to extend.  There has only been one extension to the
data format in serde 1.0 and that was the addition of `i128` and `u128` as
adding new values is a semver hazard.

Because the data model cannot be extended, formats resort to in-band
signalling to pass values it cannot express: `serde_json`'s
`arbitrary_precision` passes numbers as a map with a magic
`$serde_json::private::Number` key, `serde_json::RawValue` and `toml`'s
date-times do the same.  Every other type (value types, buffers, other
formats) that does not know about the magic key breaks.

Deser addresses this by keeping the core data model small and allowing it to be
extended with arbitrary types through extension atoms.  Every extension value
carries a fallback into the core data model so that serializers and
deserializers which do not know about an extension can still process it.  This
is also how `u128` and `i128` are supported, without having to extend the core
data model.  Deser also defines well-known extension types (date-times,
timestamps, durations, UUIDs, decimals, big integers and exact numbers) so
that formats can support them natively and the types of crates like `jiff`,
`chrono`, `time`, `uuid` and `rust_decimal` can be written as such.

The one width that the core data model keeps is the precision of floats.  An
`f32` widened to `f64` is the same value, but not the same text: `0.1f32` would
be written as `0.10000000149011612`.  Integers do not have this problem, their
text does not depend on their width.  Sinks that do not care receive `f32`
values as `f64`, the same way extension values fall back.

**Related issues:**

* [serde: Replacement API for Deserializer in-band Signalling #1463](https://github.com/serde-rs/serde/issues/1463)
* [serde: Implement `visit_i128` and `visit_u128` for ContentVisitor #2230](https://github.com/serde-rs/serde/issues/2230)

## Recursion for Serialization and Deserialization

Serde depends on recursion for serialization as well as deserialization.
Every level of nesting in your data means more stack usage until eventually
you overflow the stack.  Some formats set a cap on nesting depth to prevent
stack overflows and just refuse to deserialize deeply nested data
(`serde_json` stops at a depth of 128), code that does not go through such
a format (for instance deserializing from a `serde_json::Value`) can still
overflow the stack.

Deser's sinks and emitters return the sinks and emitters of nested values
to a driver which keeps them on the heap.  Nesting does not use the call
stack, a million levels of nesting deserialize, serialize and are skipped
without problems.  For untrusted input the depth and size can be limited
with the `Limits` layer, to any value.

**Related issues:**

* [serde: stack overflow in IgnoredAny when deserializing deeply nested serde_json::Value #3023](https://github.com/serde-rs/serde/issues/3023)
* [serde_json: Expose a setter for Deserializer::remaining_depth? #1262](https://github.com/serde-rs/json/issues/1262)

## Bytes

Serde's data model has bytes, but `Vec<u8>` and `[u8; N]` are serialized as
sequences of integers unless every field is marked with `serde_bytes`.
This is slow for binary formats and verbose for text formats.  Types that
are deserialized from bytes also cannot count on getting them back in the
same form, a `Cow<[u8]>` that was written as a sequence cannot be read
again.

In deser `Vec<u8>`, `[u8; N]`, `&[u8]` and `Cow<[u8]>` are bytes.  Formats
with native bytes (CBOR) write them as such, text formats write base64 (or
another encoding that can be picked per format or per field) and all of
them accept bytes, strings in the encoding and sequences of integers.

**Related issues:**

* [serde: Mention serde_bytes #1912](https://github.com/serde-rs/serde/issues/1912)
* [serde: Serialization of byte arrays is slow #2680](https://github.com/serde-rs/serde/issues/2680)
* [serde: Derived serialization of `Cow<'a, [u8]>` is not always reversible #2940](https://github.com/serde-rs/serde/issues/2940)

## Attributes Are Rust

Serde's derive predates the stabilization of arbitrary tokens in attributes,
which is why most of its attributes take strings: `default = "path"`,
`with = "module"`, `bound = "T: Trait"`, `crate = "path"`.  Names are
string literals only, which is why they cannot be shared through
constants or built with `concat!`.

Deser's attributes take Rust syntax that the compiler checks and editors
can navigate:

| serde | deser |
|---|---|
| `#[serde(default = "default_port")]` with a function | `#[deser(default = 8080)]` |
| `#[serde(rename = "@id")]` | `#[deser(rename = keys::ID)]` or `rename = concat!(...)` |
| `#[serde(with = "module")]` | `#[deser(as = Option<Vec<DisplayFromStr>>)]` |
| `#[serde(skip_serializing_if = "Option::is_none")]` | `#[deser(skip_serializing_if = Option::is_none)]` |
| `#[serde(bound = "T: Trait")]` | `#[deser(bound(T: Trait))]` |
| `#[serde(crate = "path")]` | `#[deser(crate = path)]` |
| `#[serde(try_from = "U")]` | `#[deser(deserialize_as = TryFromInto<U>)]`, also on fields |
| no validation hook | `#[deser(validate = path)]` |

Names take paths to constants and macro invocations, also for the tag and
content keys of tagged enums.  `#[deser(alias_all = "...")]` adds aliases in a name style
to all fields or variants, for instance to read data written with a
different convention.  The tag and content keys can have aliases too
(`#[deser(tag_alias = "...")]`, `#[deser(content_alias = "...")]`).
Attributes that would have no effect (such as field attributes on a type
that is serialized through an adapter) are rejected.

**Related issues:**

* [serde: Support default literals #368](https://github.com/serde-rs/serde/issues/368)
* [serde: Support non-string-literal wrapped paths #2862](https://github.com/serde-rs/serde/issues/2862)
* [serde: rename field attribute, passing a path instead of literal #2725](https://github.com/serde-rs/serde/issues/2725)
* [serde: Rename With Expressions #1964](https://github.com/serde-rs/serde/issues/1964)
* [serde: Allow rename of container with &'static str #2485](https://github.com/serde-rs/serde/issues/2485)
* [serde: Consider supporting concat! macro in attributes #1636](https://github.com/serde-rs/serde/issues/1636)
* [serde: Add alias_all attribute for containers similar to rename_all #1530](https://github.com/serde-rs/serde/issues/1530)
* [serde: Enum tag alias #2324](https://github.com/serde-rs/serde/issues/2324)
* [serde: #[serde(try_from)] should error if there are any field-level attributes #2882](https://github.com/serde-rs/serde/issues/2882)

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
extra code, and missing fields are handled by the adapter.  Conversions
that serde only has as container attributes (`from`, `try_from`, `into`)
are adapters too (`FromInto` and `TryFromInto`) and work on fields.

**Related issues:**

* [serde: Using de/serialize_with inside of an Option, Map, Vec #723](https://github.com/serde-rs/serde/issues/723)
* [serde: `Option` fields require explicit `default` attribute if `with` attribute specified #2878](https://github.com/serde-rs/serde/issues/2878)
* [serde: Add `from`, `try_from`, `from_str`, ... as field attributes #2610](https://github.com/serde-rs/serde/issues/2610)
* [serde: Document how to serialize using Display, deserialize using FromStr #1316](https://github.com/serde-rs/serde/issues/1316)

## Catch-All and Default Variants

Serde's `#[serde(other)]` only supports unit variants and does not work for
the map form of externally tagged enums (where it compiles, but has no
effect).  The unknown tag is lost which means that values do not round
trip.  There is also no way to pick a variant if the tag is missing.

In deser the `#[deser(other)]` variant can be any variant.  A field marked
with `#[deser(tag)]` receives the unknown tag (which does not need to be a
string) and is used as tag when serializing, the remaining fields receive the
content.  Together with `Recording` as raw value, unknown variants can be
passed through losslessly.  `#[deser(default)]` marks the variant used when
the tag is missing, known tags are still validated.

**Related issues:**

* [serde: Tagged enums should support #[serde(other)] #912](https://github.com/serde-rs/serde/issues/912)
* [serde: `#[serde(other)]` and externally tagged enum #2010](https://github.com/serde-rs/serde/issues/2010)
* [serde: #[serde(other)] with forwarding raw string #1701](https://github.com/serde-rs/serde/issues/1701)
* [serde: Optional tag for internally tagged enum #2231](https://github.com/serde-rs/serde/issues/2231)

## Tags That Are Not Strings

In serde the names of variants are `&'static str` which is why protocols
with integer or boolean tags (`{"version": 1, ...}`) need hand written
implementations.  In deser variants can be named by integers and booleans
(`#[deser(rename = 1)]`).  They are written as integers and booleans and
compared by type, except for text of unknown type (such as query strings)
which is parsed like the tag it's compared with.  `#[deser(repr)]` names
the variants by their discriminants, which serde needs the `serde_repr`
crate for (which only supports enums without data).

**Related issues:**

* [serde: Allow integer tags for internally tagged enums #745](https://github.com/serde-rs/serde/issues/745)
* [serde: Allow integers (or custom types) to be used as names/keys #1773](https://github.com/serde-rs/serde/issues/1773)

## Internally Tagged Enums

Internally tagged enums share their map with the content of the variant,
which causes some surprises in serde: the content of a newtype variant can
have a field with the name of the tag (which is then written twice), and
unit variants ignore `deny_unknown_fields` while newtype variants of `()`
reject keys even without it.

Deser keeps track of which keys belong to the tag:

* The tag is never an unknown key, and a tag that is given again after
  the variant is known is an error (``duplicate tag `type` ``).
* Newtype variants fail to serialize if their content has a field with the
  name of the tag, instead of writing it twice.
* Newtype variants of `()` are unit variants, other keys are ignored unless
  unknown fields are denied.  `deny_unknown_fields` applies to unit
  variants as well.

**Related issues:**

* [serde: Internally tagged enums do not correctly handle field collision with the discriminator #2949](https://github.com/serde-rs/serde/issues/2949)
* [serde: Internally tagged enum with `deny_unknown_fields` accepts unknown fields #2123](https://github.com/serde-rs/serde/issues/2123)
* [serde: Unknown fields are denied for tagged newtype variant with unit struct or unit type #2304](https://github.com/serde-rs/serde/issues/2304)

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
* [serde: deny_unknown_fields incorrectly fails with flattened untagged enum #1600](https://github.com/serde-rs/serde/issues/1600)
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

## Missing Values, Defaults and Skipping

Serde deserializes a `null` for an `Option<Option<T>>` as `None`, which
makes it impossible to tell a missing value apart from an explicit null
without a custom function.  Deser's options are optional natively: a
missing field is `None` and `null` is `Some(None)`, which is what partial
updates ("patches") need.  `#[deser(skip_serializing_optionals)]` skips
all unset optional fields of a struct with a single attribute.

Serde infers `T: Default` for type parameters of skipped fields, even if
the field is an `Option<T>` which has a default for any `T`.  Deser
requires the field type to have a default (`Option<T>: Default`) and type
parameters that only appear in skipped fields need neither `Serialize` nor
`Deserialize`.  This also applies to skipped fields of enum variants and
to variants that are skipped as a whole.

Optional fields (`Option<T>` and other types with a value for missing
fields) can be made required with `#[deser(required)]`: `null` is still
accepted, a missing key is not.  `#[deser(default)]` on a struct is only
invoked if a field is actually missing.

**Related issues:**

* [serde: Support, or at least document the "double option" pattern #1042](https://github.com/serde-rs/serde/issues/1042)
* [serde: #[serde(skip_deserializing)] for Option<T> wrongly requires T to implement Default #2759](https://github.com/serde-rs/serde/issues/2759)
* [serde: `Option<T>` defaults to `None` when missing fields #2753](https://github.com/serde-rs/serde/issues/2753)
* [serde: `#[serde(default)]` on structs could be lazily evaluated #2345](https://github.com/serde-rs/serde/issues/2345)

## Error Messages

Serde's errors for enums lose information: unknown variants do not name
the enum, and enums with only unit variants that receive a value of the
wrong type report `expected value`.  Deser names the enum in errors about
unknown variants (``unknown variant `D` of Kind, expected `A` or `B` ``)
and reports the type of the value and the enum otherwise (`unexpected
float, expected Level`).

Serde's `de::Error` trait only knows how to create errors, the kind of
error is lost and there is no way to carry structured information.
Deser's errors have a kind, a source location, a source error and typed
attachments (the path of `deser-path` is one), which wrapping layers and
applications can inspect.

**Related issues:**

* [serde: More descriptive unknown variant deserialize error #1481](https://github.com/serde-rs/serde/issues/1481)
* [serde: Improve error output for unknown_variant #3018](https://github.com/serde-rs/serde/issues/3018)
* [serde: Confusing error message when deserializing a simple enum from JSON #2702](https://github.com/serde-rs/serde/issues/2702)
* [serde: Create a de::ErrorKind and a kind method to de::Error #3082](https://github.com/serde-rs/serde/issues/3082)
* [serde: Allow passing extra information in deserialization error #2621](https://github.com/serde-rs/serde/issues/2621)

## Standard Library Types

Serde implements arrays up to a length of 32 as it predates const
generics.  Deser implements arrays of any length and `NonZero<T>` for all
integer types.  It also implements some types that serde does not
support: `ManuallyDrop` (like its value), `OnceLock` (like an `Option`)
and `Infallible` (which cannot be deserialized, useful for enums with
impossible variants).  `OsString` and `OsStr` are strings like paths, serde
uses a platform specific representation.

**Related issues:**

* [serde: Const generics support #1937](https://github.com/serde-rs/serde/issues/1937)
* [serde: impl Serialize and Deserialize for ManuallyDrop #1507](https://github.com/serde-rs/serde/issues/1507)
* [serde: impl Serialize for OnceCell #1952](https://github.com/serde-rs/serde/issues/1952)
* [serde: Implement Serialize and Deserialize for core::convert::Infaillible #2740](https://github.com/serde-rs/serde/issues/2740)
* [serde: OsStr and OsString platform differentiation does not match Rust standard library #2864](https://github.com/serde-rs/serde/issues/2864)

## Updating Values

Serde has a hidden `deserialize_in_place` which reuses allocations but
replaces the whole value (and fills skipped fields with their default).
Deser has `Deserialize::deserialize_update` which applies data on top of
an existing value: derived structs update the fields that are given and
keep the others (including skipped fields), nested structs (also flattened
ones), options, boxes and maps are merged.  This is useful to layer
configuration files over defaults.

**Related issues:**

* [serde: Consider unhiding `deserialize_in_place` #2204](https://github.com/serde-rs/serde/issues/2204)
* [serde: deserialize_in_place fills skipped fields with default #2512](https://github.com/serde-rs/serde/issues/2512)

## Streaming and Async

Serde's deserializers pull values from their input and drive the visitors
of the types recursively, so a deserialization cannot be suspended while
the input is not there yet.  Reading from async streams means reading the
complete value into memory first.

Deser inverts this: the format pushes events into a driver, and the
driver (and all sinks in it) can be held across calls and moved between
threads.  JSON and CBOR can be fed while the input arrives, only
incomplete tokens are buffered.  `deser-tokio` reads and writes streams of
values (JSON Lines, CBOR sequences, ...) with tokio.

**Related issues:**

* [serde: AsyncDeserializer #2739](https://github.com/serde-rs/serde/issues/2739)

## Compile Times and Code Size

Serde's traits are generic over the serializer and deserializer, which
means that the derived code is monomorphized for every format it is used
with.  This makes serde very fast at runtime but produces a lot of code for
the compiler to process and for the binary to contain.  Deser uses dynamic
dispatch for the sinks and emitters instead and moves everything that does
not depend on the types of the fields out of the derived code.  Release
builds of derived code are about 1.6 times as fast as with serde (see
[compile-times](https://github.com/mitsuhiko/deser/tree/main/compile-times)),
at some cost of runtime performance (see
[benchmark](https://github.com/mitsuhiko/deser/tree/main/benchmark)).
