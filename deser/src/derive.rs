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
//! * `#[deser(default)]`: Instructs the deserializer to fill in all missing fields from [`Default`].
//!   Default will be lazily invoked if any of the fields is not filled in.
//! * `#[deser(default = expr)]`: like `default` but fills in from the given
//!   expression instead, for instance `#[deser(default = Config::new())]`.
//!   See [default expressions](#default-expressions).
//! * `#[deser(skip_serializing_optionals)]`: when this is set the struct serializer will automatically
//!   skip over all optional values that are currently not set.  This uses the
//!   [`is_optional`](crate::ser::Serialize::is_optional) serialize method to figure out if a
//!   a field is optional.  At the moment only `None` and `()` are considered optional.
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
//!   struct or map).
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
//! ## Enum Attributes
//!
//! * `#[deser(rename = "...")]`: renames the type name hint for this enum.
//! * `#[deser(rename_all = "...")]`: renames all variants at once to a
//!   specific name style.  The possible values are `"lowercase"`, `"UPPERCASE"`,
//!   `"PascalCase"`, `"camelCase"`, `"snake_case"`, `"SCREAMING_SNAKE_CASE"`,
//!   `"kebab-case"`, and `"SCREAMING-KEBAB-CASE"`.
//! * `#[deser(tag = "...")]`: makes the enum internally tagged with the given
//!   tag field.
//! * `#[deser(tag = "...", content = "...")]`: makes the enum adjacently
//!   tagged with the given tag and content fields.
//! * `#[deser(untagged)]`: makes the enum untagged.
//! * `#[deser(skip_serializing_optionals)]`: skips optional values that are not
//!   set in struct variants when serializing.
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
//! * `#[deser(alias = "...")]`: provides an alias for the field name for deserialization.  This is ignored
//!   for serialization.
//! * `#[deser(flatten)]`: when added to a nested struct field causes that field to be flattened into the
//!   parent struct.  Note that flattening only works with structs (more specifically with string) keys.
//!   This feature is enabled by [`value_for_key`](crate::de::Sink::value_for_key).
//! * `#[deser(as = Adapter)]`: serializes and deserializes the field with an
//!   adapter instead of the field type's own implementation.  `_` in the
//!   adapter stands for the type's own implementation.  See
//!   [adapters](#adapters).
//!
//! The field of newtype structs and the fields of newtype and tuple variants
//! support `as` as well.
//!
//! ## Enum Variant Attributes
//!
//! The following attributes can be added to enum variants:
//!
//! * `#[deser(rename = "...")]`: renames the enum variant.
//! * `#[deser(alias = "...")]`: provides an alias for the variant name for deserialization.  This is ignored
//!   for serialization.
//! * `#[deser(other)]`: marks a variant as catch-all for unknown tags during
//!   deserialization (not supported for untagged enums).  See
//!   [other variants](#other-variants).
//! * `#[deser(default)]`: marks the variant that is used if the tag is missing.
//!   This is only supported for internally and adjacently tagged enums.  The
//!   variant can also be marked as `other`.
//!
//! The fields of struct variants support the same attributes as struct fields,
//! except for `flatten` which is only supported for deserialization.
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
//! ## Bounds
//!
//! By default the derive requires every type parameter to implement the
//! derived trait (`T: Serialize` or `T: Deserialize`, for enums also
//! `T: 'static` when deserializing).  This is wrong when a type parameter
//! is not serialized itself, for instance when only an associated type is.
//! The bounds can be replaced with a list of where predicates:
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

// these exist as explicit aliases only

/// Provides automatic deriving for [`Serialize`](crate::Serialize).
pub use deser_derive::Serialize;

/// Provides automatic deriving for [`Deserialize`](crate::Deserialize).
pub use deser_derive::Deserialize;
