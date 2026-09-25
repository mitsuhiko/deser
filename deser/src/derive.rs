//! Support for automatic serializer and deserializer deriving.
//!
//! When the `derive` feature is enabled (which is the default) basic automatic
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
//!
//! ## Enum Variant Attributes
//!
//! The following attributes can be added to enum variants:
//!
//! * `#[deser(rename = "...")]`: renames the enum variant.
//! * `#[deser(alias = "...")]`: provides an alias for the variant name for deserialization.  This is ignored
//!   for serialization.
//! * `#[deser(other)]`: marks a unit variant as catch-all for unknown variant
//!   names during deserialization (not supported for untagged enums).
//!
//! The fields of struct variants support the same attributes as struct fields,
//! except for `flatten` which is only supported for deserialization.
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
//! is not supported.
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
//!     deserialize_bound(K::Value: Deserialize),
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
