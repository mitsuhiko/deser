//! Support for automatic serializer and deserializer deriving.
//!
//! When the `derive` feature is enabled basic automatic deriving of [`Serialize`](crate::Serialize)
//! and [`Deserialize`](crate::Deserialize) is provided.  This feature is modelled after [`serde`](https://serde.rs/)
//! so if you are coming from there you should find many of the functionality to be similar.
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
//! ## Enum Attributes
//!
//! * `#[deser(rename_all = "...")]`: renames all variants at once to a
//!   specific name style.  The possible values are `"lowercase"`, `"UPPERCASE"`,
//!   `"PascalCase"`, `"camelCase"`, `"snake_case"`, `"SCREAMING_SNAKE_CASE"`,
//!   `"kebab-case"`, and `"SCREAMING-KEBAB-CASE"`.
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

// these exist as explicit aliases only

/// Provides automatic deriving for [`Serialize`](crate::Serialize).
pub use deser_derive::Serialize;

/// Provides automatic deriving for [`Deserialize`](crate::Deserialize).
pub use deser_derive::Deserialize;
