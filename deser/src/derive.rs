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
//! * `#[deser(default = "...")]`: like `default` but fills in from a function with the given name instead.
//! * `#[deser(skip_serializing_optionals)]`: when this is set the struct serializer will automatically
//!   skip over all optional values that are currently not set.  This uses the
//!   [`is_optional`](crate::ser::Serialize::is_optional) serialize method to figure out if a
//!   a field is optional.  At the moment only `None` and `()` are considered optional.
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
//!
//! ## Struct Field Attributes
//!
//! The following attributes can be added to fields:
//!
//! * `#[deser(rename = "...")]`: renames the field.
//! * `#[deser(default)]`: fills in the field default value from [`Default`].
//! * `#[deser(default = "...")]`: like `default` but fills in from a function with the given name instead.
//! * `#[deser(skip_serializing_if = "...")]`: invokes the provided callback with the value to check
//!   if it should be skipped during serialization.
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

// these exist as explicit aliases only

/// Provides automatic deriving for [`Serialize`](crate::Serialize).
pub use deser_derive::Serialize;

/// Provides automatic deriving for [`Deserialize`](crate::Deserialize).
pub use deser_derive::Deserialize;
