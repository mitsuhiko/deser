//! Support for automatic serializer and deserializer deriving.
//!
//! When the `derive` feature is enabled basic automatic
//! deriving of [`Serialize`](crate::Serialize) and
//! [`Deserialize`](crate::Deserialize) is provided.  This feature is modelled
//! after [`serde`](https://serde.rs/) so if you are coming from there you
//! should find many of the functionality to be similar.
//!
//! # Example
//!
//! ```
//! use deser::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct User {
//!     id: u64,
//!     username: String,
//!     kind: UserKind,
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(rename_all = "UPPERCASE")]
//! pub enum UserKind {
//!     User,
//!     Admin,
//!     Bot,
//! }
//! ```
//!
//! # Supported Types
//!
//! Currently the following types can be derived:
//!
//! * Structs
//! * Newtype structs
//! * Basic enums
//!
//! # Borrowing
//!
//! Structs (and newtype structs) can borrow from the data they are
//! deserialized from.  The derive implements `Deserialize<'de>` with `'de`
//! outliving all lifetimes of the type.  References (`&str` and `&[u8]`)
//! always borrow, `Cow` borrows with the
//! [`Borrowed`](crate::adapters::Borrowed) adapter:
//!
//! ```
//! use std::borrow::Cow;
//! use deser::Deserialize;
//! use deser::adapters::Borrowed;
//!
//! #[derive(Deserialize)]
//! pub struct Message<'a> {
//!     id: &'a str,
//!     #[deser(as = Borrowed)]
//!     text: Cow<'a, str>,
//! }
//! ```
//!
//! Data can only be borrowed if the data format passes it on borrowed.  If
//! the data is not borrowed (for instance because a string had escape
//! sequences) references fail to deserialize while `Cow` holds owned data.
//! Enums with data cannot have lifetime parameters.  The lifetime `'de` is
//! reserved for the derive.
//!
//! # Customization
//!
//! The automatically derived features can be customized via attributes:
//!
//! ## Struct Attributes
//!
//! The following attributes can be added to structs:
//!
//! * `#[deser(rename = "...")]`: renames the type name hint for this struct.
//! * `#[deser(rename_all = "...")]`: renames all fields at once to a
//!   specific name style.  The possible values are `"lowercase"`, `"UPPERCASE"`,
//!   `"PascalCase"`, `"camelCase"`, `"snake_case"`, `"SCREAMING_SNAKE_CASE"`,
//!   `"kebab-case"`, and `"SCREAMING-KEBAB-CASE"`.
//! * `#[deser(alias_all = "...")]`: adds an alias in a name style to all
//!   fields.  It takes the same styles as `rename_all`, is applied to the
//!   names of the fields in Rust (independent of renames) and can be given
//!   more than once.
//! * `#[deser(default)]`: Instructs the deserializer to fill in all missing fields from [`Default`].
//!   Default will be lazily invoked if any of the fields is not filled in.
//! * `#[deser(default = expr)]`: like `default` but fills in from the given
//!   expression instead, for instance `#[deser(default = Config::new())]`.
//!   See [default expressions](#default-expressions).
//! * `#[deser(deny_unknown_fields)]`: rejects keys that neither a field nor
//!   a flattened field takes.  By default they are ignored, unless the
//!   [`UnknownFields`](crate::de::UnknownFields) policy of the
//!   deserialization says otherwise.  See [unknown fields](#unknown-fields).
//! * `#[deser(validate = path)]`: validates the struct once it was
//!   deserialized.  See [validation](#validation).
//! * `#[deser(skip_serializing_optionals)]`: when this is set the struct serializer will automatically
//!   skip over all optional values that are currently not set.  This uses the
//!   [`is_optional`](crate::ser::Serialize::is_optional) serialize method to figure out if a
//!   a field is optional.  At the moment only `None` and `()` are considered optional.
//! * `#[deser(as = Adapter)]`, `#[deser(serialize_as = Adapter)]` and
//!   `#[deser(deserialize_as = Adapter)]`: serializes and deserializes the
//!   struct with an adapter instead of its fields.  See [container
//!   adapters](#container-adapters).
//! * `#[deser(bound(...))]`, `#[deser(serialize_bound(...))]` and
//!   `#[deser(deserialize_bound(...))]`: see [bounds](#bounds).
//! * `#[deser(crate = path)]`: see [crate path](#crate-path).
//!
//! ## Enums
//!
//! Enums can have unit variants, newtype variants (`A(T)`), tuple variants
//! (`A(T, U)`) and struct variants (`A { x: T }`).  The content of a unit
//! variant is null, of a newtype variant the inner value, of a tuple variant a
//! sequence and of a struct variant a map.  How the variant is identified is
//! controlled by the representation, which follows serde:
//!
//! * externally tagged (the default): unit variants are strings (`"A"`), all
//!   other variants are maps with a single key: `{"A": content}`.
//! * internally tagged (`#[deser(tag = "type")]`): `{"type": "A", ...fields}`.
//!   Supports unit, struct and newtype variants (the inner value must be a
//!   struct or map).  Newtype variants of `()` (`A(())`) are unit variants.
//! * adjacently tagged (`#[deser(tag = "t", content = "c")]`):
//!   `{"t": "A", "c": content}`.
//! * untagged (`#[deser(untagged)]`): just the content.  The variants are
//!   tried in order and the first one that accepts the value wins.
//!
//! The tag does not need to come first in the tagged representations and
//! untagged enums need to look at the value multiple times.  In these cases
//! values are recorded and replayed (see [`Recording`](crate::de::Recording)).
//! Format specific information such as map key handling, extension values,
//! source locations and paths is retained.
//!
//! ```
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(tag = "type", rename_all = "snake_case")]
//! pub enum Shape {
//!     Circle { radius: f64 },
//!     Rect { width: f64, height: f64 },
//!     Empty,
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(untagged)]
//! pub enum NumberOrText<T> {
//!     Number(T),
//!     Text(String),
//! }
//! ```
//!
//! ### Tags
//!
//! The names of variants are their tags.  Besides strings they can be
//! integers or booleans, which are then written as such:
//!
//! ```
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(tag = "version")]
//! pub enum Message {
//!     // {"version": 1, "text": "..."}
//!     #[deser(rename = 1)]
//!     V1 { text: String },
//!     #[deser(rename = 2)]
//!     V2 { text: String, lang: String },
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! pub enum Level {
//!     // 0
//!     #[deser(rename = 0)]
//!     Off,
//!     #[deser(rename = 1, alias = "low")]
//!     Low,
//! }
//! ```
//!
//! Tags are compared by type: the string `"1"` does not match a variant
//! named `1`.  Text of unknown type (the keys of JSON objects, the values of
//! query strings) matches both, so `version=1` in a query string selects
//! `Message::V1`.  Integers are compared by value, independent of their
//! width.
//!
//! With `#[deser(repr)]` the variants are named by their discriminants.
//! The discriminants have to be integer literals (or not given):
//!
//! ```
//! use deser::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(repr)]
//! #[repr(u8)]
//! pub enum Priority {
//!     // 1
//!     Low = 1,
//!     // 2
//!     Normal,
//!     // 10
//!     High = 10,
//! }
//! ```
//!
//! ## Enum Attributes
//!
//! * `#[deser(rename = "...")]`: renames the type name hint for this enum.
//! * `#[deser(rename_all = "...")]`: renames all variants at once to a
//!   specific name style.  The possible values are `"lowercase"`, `"UPPERCASE"`,
//!   `"PascalCase"`, `"camelCase"`, `"snake_case"`, `"SCREAMING_SNAKE_CASE"`,
//!   `"kebab-case"`, and `"SCREAMING-KEBAB-CASE"`.
//! * `#[deser(alias_all = "...")]`: adds an alias in a name style to all
//!   variants, like on structs.
//! * `#[deser(repr)]`: names the variants by their discriminants (see
//!   [tags](#tags)).  This cannot be combined with `rename_all`, `alias_all`
//!   and `rename` on variants.
//! * `#[deser(tag = "...")]`: makes the enum internally tagged with the given
//!   tag field.
//! * `#[deser(tag = "...", content = "...")]`: makes the enum adjacently
//!   tagged with the given tag and content fields.
//! * `#[deser(tag_alias = "...")]` and `#[deser(content_alias = "...")]`:
//!   accepts other keys for the tag and the content when deserializing.  They
//!   can be given more than once.  The tag is an error if it's given more
//!   than once (under any of its keys).
//! * `#[deser(untagged)]`: makes the enum untagged.
//! * `#[deser(deny_unknown_fields)]`: rejects unknown keys in struct variants
//!   (and the unit variants of internally tagged enums) and keys other than
//!   the tag and the content of adjacently tagged enums.  See [unknown
//!   fields](#unknown-fields).
//! * `#[deser(validate = path)]`: validates the enum once it was
//!   deserialized.  See [validation](#validation).
//! * `#[deser(skip_serializing_optionals)]`: skips optional values that are not
//!   set in struct variants when serializing.
//! * `#[deser(as = Adapter)]`, `#[deser(serialize_as = Adapter)]` and
//!   `#[deser(deserialize_as = Adapter)]`: serializes and deserializes the
//!   enum with an adapter instead of its variants.  See [container
//!   adapters](#container-adapters).
//! * `#[deser(bound(...))]`, `#[deser(serialize_bound(...))]` and
//!   `#[deser(deserialize_bound(...))]`: see [bounds](#bounds).
//! * `#[deser(crate = path)]`: see [crate path](#crate-path).
//!
//! ## Struct Field Attributes
//!
//! The following attributes can be added to fields:
//!
//! * `#[deser(rename = "...")]`: renames the field.
//! * `#[deser(default)]`: fills in the field default value from [`Default`].
//! * `#[deser(default = expr)]`: like `default` but fills in from the given
//!   expression instead, for instance `#[deser(default = 42)]`.  See
//!   [default expressions](#default-expressions).
//! * `#[deser(skip_serializing_if = path)]`: invokes the function at the given
//!   path with a reference to the value to check if it should be skipped
//!   during serialization, for instance
//!   `#[deser(skip_serializing_if = Option::is_none)]`.
//! * `#[deser(skip)]`: the field is neither serialized nor deserialized.
//!   When deserializing, its value is the `default` of the field, or the
//!   one of the container default, or [`Default`].  The key of the field is
//!   an unknown key.  The type of the field does not need to be
//!   serializable.
//! * `#[deser(skip_serializing)]` and `#[deser(skip_deserializing)]`: skip
//!   the field in one direction only.
//! * `#[deser(required)]`: the field has to be given even if its type has a
//!   value for missing fields, for instance `None` for `Option`.
//! * `#[deser(alias = "...")]`: provides an alias for the field name for deserialization.  This is ignored
//!   for serialization.
//! * `#[deser(flatten)]`: when added to a nested struct field causes that field to be flattened into the
//!   parent struct.  Note that flattening only works with string keys.
//!   This feature is enabled by [`value_for_key`](crate::de::Sink::value_for_key).
//!   Internally tagged enums can be flattened too.  Until their tag was seen
//!   they take all keys that the struct and the flattened fields before them
//!   do not take, so they should come after other flattened fields.
//!   Maps (and `deser_value::Value`) take all keys that the struct and the
//!   flattened fields before them do not take, the keys are parsed into the
//!   key type like the keys of JSON objects.  A flattened
//!   [`Recording`](crate::de::Recording) records them as a map.  When serializing, the keys of
//!   the map become fields.  A flattened `Option` is `None` if the value did
//!   not take any key (unlike serde, errors in the value are not turned into
//!   `None`), when serializing `None` has no fields.
//! * `#[deser(validate = path)]`: validates the value of the field once it
//!   was deserialized.  See [validation](#validation).
//! * `#[deser(as = Adapter)]`: serializes and deserializes the field with an
//!   adapter instead of the field type's own implementation.  `_` in the
//!   adapter stands for the type's own implementation.  See
//!   [adapters](#adapters).
//! * `#[deser(serialize_as = Adapter)]` and `#[deser(deserialize_as = Adapter)]`:
//!   like `as` but only for serialization or deserialization, the other
//!   direction uses the field type's own implementation.  Both can be used
//!   together to use different adapters, but not together with `as`.
//!
//! The field of newtype structs and the fields of newtype and tuple variants
//! support `as`, `serialize_as` and `deserialize_as` as well.
//!
//! ## Enum Variant Attributes
//!
//! The following attributes can be added to enum variants:
//!
//! * `#[deser(rename = "...")]`: renames the enum variant.  Variants can
//!   also be named by integers and booleans (`#[deser(rename = 1)]`,
//!   `#[deser(rename = true)]`), see [tags](#tags).
//! * `#[deser(alias = "...")]`: provides an alias for the variant name for deserialization.  This is ignored
//!   for serialization.  Like `rename` it takes strings, integers and
//!   booleans.
//! * `#[deser(other)]`: marks a variant as catch-all for unknown tags during
//!   deserialization (not supported for untagged enums).  See
//!   [other variants](#other-variants).
//! * `#[deser(default)]`: marks the variant that is used if the tag is missing.
//!   This is only supported for internally and adjacently tagged enums.  The
//!   variant can also be marked as `other`.
//! * `#[deser(skip)]`: the variant is neither serialized nor deserialized.
//!   Serializing it is an error and its name is an unknown variant when
//!   deserializing.  The types of its fields do not need to be serializable
//!   or deserializable.
//! * `#[deser(skip_serializing)]` and `#[deser(skip_deserializing)]`: skip
//!   the variant in one direction only.
//!
//! The fields of struct variants support the same attributes as struct fields,
//! except for `flatten`.
//!
//! ## Adapters
//!
//! Adapters customize how a field is serialized and deserialized (see
//! [`adapters`](crate::adapters)).  They compose with containers: to use an
//! adapter for the values of an optional map, the adapter is wrapped in the
//! same containers:
//!
//! ```
//! use std::collections::BTreeMap;
//! use std::net::IpAddr;
//! use deser::{Deserialize, Serialize};
//! use deser::adapters::DisplayFromStr;
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct Hosts {
//!     #[deser(as = DisplayFromStr)]
//!     primary: IpAddr,
//!     #[deser(as = Option<BTreeMap<_, DisplayFromStr>>)]
//!     named: Option<BTreeMap<String, IpAddr>>,
//! }
//! ```
//!
//! Missing values are handled by the adapter, `Option<U>` makes missing
//! fields `None` like `Option<T>` does.  Type parameters that only appear in
//! fields with adapters do not need to implement [`Serialize`](crate::Serialize)
//! or [`Deserialize`](crate::Deserialize), instead the adapter needs to
//! support the field type.
//!
//! `serialize_as` and `deserialize_as` use an adapter for one direction only,
//! for instance to read values in a legacy format which are written in the
//! regular one.  Nothing checks that the two directions agree: values that
//! are written with one representation and read with another might not
//! round trip.
//!
//! ## Container Adapters
//!
//! Adapters can also be placed on structs, enums and unions.  The derived
//! implementations then forward to the adapter and the fields and variants
//! are not used at all.  This works for all shapes of types (including tuple
//! structs and unit structs) and neither the fields nor the type parameters
//! need to be serializable, only the adapter has to support the type:
//!
//! ```
//! use deser::{Deserialize, Serialize};
//! use deser::adapters::TryFromInto;
//!
//! #[derive(Clone, Serialize, Deserialize)]
//! #[deser(as = TryFromInto<String>)]
//! pub struct Email {
//!     user: String,
//!     domain: String,
//! }
//!
//! impl TryFrom<String> for Email {
//!     type Error = &'static str;
//!
//!     fn try_from(value: String) -> Result<Email, Self::Error> {
//!         match value.split_once('@') {
//!             Some((user, domain)) => Ok(Email { user: user.into(), domain: domain.into() }),
//!             None => Err("missing @"),
//!         }
//!     }
//! }
//!
//! impl From<Email> for String {
//!     fn from(value: Email) -> String {
//!         format!("{}@{}", value.user, value.domain)
//!     }
//! }
//! ```
//!
//! `serialize_as` and `deserialize_as` forward only one direction, the other
//! one is derived as usual.  A common use is to convert values when they are
//! read while writing them with the derived implementation (to only check
//! values, [validation](#validation) is simpler):
//!
//! ```
//! use deser::{Deserialize, Serialize};
//! use deser::adapters::TryFromInto;
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(deserialize_as = TryFromInto<RawPorts>)]
//! pub struct Ports {
//!     min: u16,
//!     max: u16,
//! }
//!
//! #[derive(Deserialize)]
//! struct RawPorts {
//!     min: u16,
//!     max: u16,
//! }
//!
//! impl TryFrom<RawPorts> for Ports {
//!     type Error = &'static str;
//!
//!     fn try_from(value: RawPorts) -> Result<Ports, Self::Error> {
//!         if value.min > value.max {
//!             return Err("min is larger than max");
//!         }
//!         Ok(Ports { min: value.min, max: value.max })
//!     }
//! }
//! ```
//!
//! Some things to be aware of:
//!
//! * Attributes that only affect the directions which forward to the adapter
//!   would have no effect and are rejected.  With `as` this is every attribute
//!   on fields and variants and all attributes on the container except for
//!   `rename` (which renames the type in its description), the bounds and the
//!   crate path.  With `deserialize_as` for instance `alias` and `default`
//!   are rejected but `rename` and `skip_serializing_if` are fine.
//! * The adapter cannot use the implementation of the type itself as that
//!   implementation forwards to the adapter: `_`, `Same` and the type are
//!   rejected as adapter and as its direct type arguments (as in
//!   `FromInto<Self>`).  Adapters with a default for the inner adapter such
//!   as a plain [`DefaultOnError`](crate::adapters::DefaultOnError) use
//!   `Same` implicitly which is not detected.  The type can be used indirectly,
//!   for instance a tree can be `FromInto<Vec<Tree>>`.
//! * Missing values and optional values are handled by the adapter, as for
//!   fields with adapters.
//! * Values can be flattened if the adapter serializes them as a struct or
//!   map, for instance with `TryFromInto<RawStruct>`.
//! * Adapters are `'static` which means that type parameters used in the
//!   adapter need to be `'static` and adapters cannot convert from borrowed
//!   data.  For types with type parameters the derive requires the adapter
//!   to support the type.  For recursive types this cannot be proven by the
//!   compiler, custom [bounds](#bounds) are needed there.
//!
//! Serde's container attributes map to adapters like this:
//!
//! | serde | deser |
//! |---|---|
//! | `#[serde(from = "U")]` | `#[deser(deserialize_as = FromInto<U>)]` |
//! | `#[serde(try_from = "U")]` | `#[deser(deserialize_as = TryFromInto<U>)]` |
//! | `#[serde(into = "U")]` | `#[deser(serialize_as = FromInto<U>)]` |
//! | `#[serde(from = "U", into = "U")]` | `#[deser(as = FromInto<U>)]` |
//! | `#[serde(transparent)]` | newtype structs are transparent |
//!
//! The field attributes `serialize_with` and `deserialize_with` correspond
//! to `serialize_as` and `deserialize_as` with an adapter.
//!
//! ## Other Variants
//!
//! The variant marked with `#[deser(other)]` receives all tags that do not
//! belong to a known variant.  This includes tags that are not strings.  The
//! variant can capture the tag in a field marked with `#[deser(tag)]`, the
//! tag field can be of any type that can be deserialized from the tag and it
//! can use an adapter.  All other fields make up the content of the variant
//! which follows the regular rules: a single remaining unnamed field is a
//! newtype, multiple are a tuple and named fields are a struct.  If there are
//! no remaining fields, the content is ignored.
//!
//! When serialized, the value of the tag field is used as tag which means
//! that such values round trip.  To capture content without interpreting it,
//! [`Recording`](crate::de::Recording) can be used:
//!
//! ```
//! use deser::{Deserialize, Serialize};
//! use deser::de::Recording;
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(rename_all = "snake_case")]
//! pub enum Kind {
//!     Bash,
//!     Zsh,
//!     #[deser(other)]
//!     Other(#[deser(tag)] String),
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(tag = "type", rename_all = "snake_case")]
//! pub enum Event {
//!     Click { x: u32, y: u32 },
//!     #[deser(other)]
//!     Unknown(#[deser(tag)] String, Recording),
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(tag = "type", rename_all = "snake_case")]
//! pub enum Bind {
//!     // used if the type is missing
//!     #[deser(default)]
//!     Http { address: String },
//!     Tls { address: String, cert: String },
//! }
//! ```
//!
//! Only unknown and missing tags go to the other and default variants: known
//! tags with invalid content are errors.  Variants with content that are
//! represented by their tag alone (for instance a string for an externally
//! tagged enum) receive null as content.
//!
//! ## Names
//!
//! `rename`, `alias`, `tag`, `content` and their aliases take string
//! literals or expressions that are strings at compile time: paths to
//! constants and macro invocations such as `concat!(...)`.  This is useful
//! for names that are shared with other code:
//!
//! ```
//! use deser::{Deserialize, Serialize};
//!
//! mod keys {
//!     pub const ID: &str = "@id";
//!     pub const TYPE: &str = "@type";
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(rename = concat!(module_path!(), "::Node"))]
//! pub struct Node {
//!     #[deser(rename = keys::ID)]
//!     id: String,
//!     #[deser(rename = concat!("x-", "parent"))]
//!     parent: Option<String>,
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(tag = keys::TYPE, tag_alias = "type")]
//! pub enum Resource {
//!     Node(Node),
//! }
//! ```
//!
//! Names that are expressions are not checked for duplicates by the
//! derive.
//!
//! ## Validation
//!
//! `#[deser(validate = path)]` invokes a function with a reference to the
//! value once it was deserialized.  It can be placed on fields, structs,
//! newtype structs and enums.  The function returns a `Result<(), E>` where
//! `E` implements [`Display`](std::fmt::Display).  If it returns an error,
//! deserialization fails with ``invalid value: {error}``.  The error points
//! at the start of the value (the location and, with
//! [`deser-path`](https://docs.rs/deser-path), the path), also for compound
//! values which can only be validated once they are complete.
//!
//! ```
//! use deser::Deserialize;
//!
//! fn non_zero(value: &u16) -> Result<(), &'static str> {
//!     if *value == 0 { Err("port must not be zero") } else { Ok(()) }
//! }
//!
//! fn ordered(value: &Ports) -> Result<(), String> {
//!     if value.min > value.max {
//!         return Err(format!("min {} is larger than max {}", value.min, value.max));
//!     }
//!     Ok(())
//! }
//!
//! #[derive(Deserialize)]
//! #[deser(validate = ordered)]
//! pub struct Ports {
//!     #[deser(validate = non_zero)]
//!     min: u16,
//!     max: u16,
//! }
//! ```
//!
//! Values that are not in the data (missing fields that are `None` or
//! filled in with defaults) are not validated.  A validator on a field
//! receives the field's type, for an `Option<T>` that's `&Option<T>`.  To
//! validate every value of a type, place the validator on the type instead
//! of the fields.  Validators run for every value that is deserialized,
//! also in values that are replayed (for instance for untagged enums).
//!
//! ## Updating Values
//!
//! Derived structs can update an existing value in place (see
//! [`Deserialize::deserialize_update`](crate::Deserialize::deserialize_update)):
//! the fields that are given are updated, all others keep their values.
//! Fields are updated the same way which means that nested structs are
//! merged, `Option`s which are set and `Box`es update their value (null
//! clears options) and maps (`HashMap`, `BTreeMap` and the maps of
//! `deser-value`) are merged: the entries that are given are inserted,
//! replacing the values of keys that exist (the values are not merged).  All other values (sequences, enums)
//! are replaced.  This is useful to layer configuration files:
//!
//! ```
//! use deser::Deserialize;
//! use deser::de::DeserializeDriver;
//!
//! #[derive(Deserialize)]
//! pub struct Config {
//!     server: Server,
//!     debug: bool,
//! }
//!
//! #[derive(Deserialize)]
//! pub struct Server {
//!     host: String,
//!     port: u16,
//! }
//!
//! let mut config = Config {
//!     server: Server { host: "localhost".into(), port: 80 },
//!     debug: false,
//! };
//! // {"server": {"port": 8080}}
//! let mut driver = DeserializeDriver::update(&mut config);
//! driver.emit(deser::Event::map_start()).unwrap();
//! driver.emit("server").unwrap();
//! driver.emit(deser::Event::map_start()).unwrap();
//! driver.emit("port").unwrap();
//! driver.emit(8080u64).unwrap();
//! driver.emit(deser::Event::MapEnd).unwrap();
//! driver.emit(deser::Event::MapEnd).unwrap();
//! drop(driver);
//! assert_eq!(config.server.host, "localhost");
//! assert_eq!(config.server.port, 8080);
//! ```
//!
//! With a data format this is [`Deserializer::update`](crate::de::Deserializer::update)
//! (for instance `deser_toml::Deserializer::from_str(s).update(&mut config)`).
//! Some things to be aware of:
//!
//! * Fields with adapters or validators are replaced (after validating the
//!   new value).  Validators of the struct run after the update.
//! * Flattened fields are updated with the keys they take, flattened fields
//!   that take no key keep their values.
//! * If the update fails, the value might be partially updated.
//!
//! ## Unknown Fields
//!
//! Keys of a struct that no field takes are ignored by default.  They can be
//! rejected for a type with `#[deser(deny_unknown_fields)]` or for all types
//! of a deserialization with the [`UnknownFields`](crate::de::UnknownFields)
//! policy in the state, which can also collect them (for instance to warn
//! about typos in config files).  Errors point to the key and carry the path
//! if [`deser-path`](https://docs.rs/deser-path) is used.
//!
//! Keys are only unknown if no flattened field takes them either: only the
//! struct the key is given to decides, the attribute on flattened types has
//! no effect.  This means that `deny_unknown_fields` works with flattened
//! structs and internally tagged enums, a flattened map takes all keys.
//! The tag of internally tagged enums is never an unknown key, untagged
//! enums with `deny_unknown_fields` do not match maps with keys that a
//! variant does not know.
//!
//! ```
//! use deser::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! #[deser(deny_unknown_fields)]
//! pub struct Server {
//!     host: String,
//!     #[deser(flatten)]
//!     kind: Kind,
//! }
//!
//! #[derive(Debug, Deserialize)]
//! #[deser(tag = "type", rename_all = "lowercase")]
//! pub enum Kind {
//!     Http { port: u16 },
//!     Unix { path: String },
//! }
//!
//! // {"host": "a", "type": "http", "port": 80, "path": "/"}
//! let mut out = None::<Server>;
//! let mut driver = deser::de::DeserializeDriver::new(&mut out);
//! driver.emit(deser::Event::map_start()).unwrap();
//! for (key, value) in [("host", "a"), ("type", "http")] {
//!     driver.emit(key).unwrap();
//!     driver.emit(value).unwrap();
//! }
//! driver.emit("port").unwrap();
//! driver.emit(80u64).unwrap();
//! driver.emit("path").unwrap();
//! let err = driver.emit("/").unwrap_err();
//! assert_eq!(err.message(), "unknown field `path`");
//! ```
//!
//! ## Bounds
//!
//! By default the derive requires every type parameter to implement the
//! derived trait (`T: Serialize` or `T: Deserialize`, for enums also
//! `T: 'static` when deserializing).  Type parameters which only appear in
//! fields with adapters instead need to be `Sync` for `Serialize` and
//! `Send` for `Deserialize` (serializables are `Sync` and deserializables
//! are `Send`).  Types with [container adapters](#container-adapters)
//! instead require the adapter to support the type and the type to be
//! `Sync` or `Send`.  This is wrong when a type parameter is not serialized
//! itself, for instance when only an associated type is.  The bounds can
//! be replaced with a list of where predicates:
//!
//! * `#[deser(bound(...))]` replaces the bounds for both derives.
//! * `#[deser(serialize_bound(...))]` and `#[deser(deserialize_bound(...))]`
//!   replace them for one derive and take precedence over `bound`.
//!
//! The predicates are added to the where clause of the type.  `bound()`
//! removes the inferred bounds entirely.  As with other attributes, `Self`
//! is not supported.  In deserialize bounds the lifetime of the data is
//! available as `'de`.  As `bound` also applies to `Serialize` where there
//! is no such lifetime, use
//! [`DeserializeOwned`](crate::de::DeserializeOwned) there.
//!
//! ```
//! use deser::{Deserialize, Serialize};
//!
//! pub trait Kind {
//!     type Value;
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! #[deser(
//!     serialize_bound(K::Value: Serialize),
//!     deserialize_bound(K::Value: Deserialize<'de>),
//! )]
//! pub struct Holder<K: Kind> {
//!     value: K::Value,
//! }
//! ```
//!
//! ## Crate Path
//!
//! The generated code refers to the deser crate as `deser`.  If it is
//! available under a different name, because it was renamed in `Cargo.toml`
//! or is re-exported by another crate, the path can be set with
//! `#[deser(crate = path)]`:
//!
//! ```
//! # mod framework { pub mod serialization { pub use deser::*; } }
//! #[derive(framework::serialization::Serialize)]
//! #[deser(crate = framework::serialization)]
//! pub struct User {
//!     name: String,
//! }
//! ```
//!
//! ## Default Expressions
//!
//! `default = expr` takes an expression which is evaluated every time a
//! default is needed, and only then.  On a field it has to produce a value of
//! the field's type, on a container a value of the container type.
//!
//! * String literals are converted with [`Into`], so `default = "localhost"`
//!   works for `String` fields and all other types that implement
//!   `From<&str>`.
//! * Functions need to be called: `default = make_default()`, not
//!   `default = make_default`.
//! * Closures and blocks are not supported, move such logic into a function.
//! * `Self` is not supported in default expressions and `skip_serializing_if`
//!   paths as the generated code does not live in an `impl` block of the
//!   type.  Use the name of the type instead.
//!
//! ```
//! use deser::Deserialize;
//!
//! fn default_tags() -> Vec<String> {
//!     vec!["default".into()]
//! }
//!
//! #[derive(Deserialize)]
//! pub struct Config {
//!     #[deser(default = "localhost")]
//!     host: String,
//!     #[deser(default = 8080)]
//!     port: u16,
//!     #[deser(default = default_tags())]
//!     tags: Vec<String>,
//! }
//! ```
