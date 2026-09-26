//! A dynamic value type for deser.
//!
//! [`Value`] can hold any value of the deser data model.  It's useful for
//! data whose structure is not known up front, to inspect or transform data
//! before it's deserialized into a type, or to convert between formats:
//!
//! ```
//! use deser_value::{Value, value};
//!
//! let mut config: Value = deser_json::from_str(r#"{"name": "app", "port": 8080}"#).unwrap();
//! config["port"] = value!(9090);
//! config["tags"] = value!(["web", "prod"]);
//! assert_eq!(
//!     deser_json::to_string(&config).unwrap(),
//!     r#"{"name":"app","port":9090,"tags":["web","prod"]}"#
//! );
//! ```
//!
//! Values are converted from and into other types with [`to_value`] and
//! [`from_value`].  Values can be built with the [`value!`] macro.
//!
//! # Retained Information
//!
//! Values try to retain as much information as possible, which means that
//! a value that is deserialized and serialized again comes out the same:
//!
//! * Map keys can be any value (like integers in CBOR) and maps retain the
//!   order of their entries.
//! * Values that extend the data model (like date-times, UUIDs or exact
//!   numbers, see [`deser::ext`]) retain their type.
//! * Maps and sequences retain their [`Order`](deser::Order).
//! * Bytes retain their [fallback](deser::Bytes::fallback).
//! * [Event data](deser::State::event), which is information that is
//!   attached to values but not part of the data model (for instance CBOR
//!   tags or formatting hints), is retained in the [`Meta`] data of values.
//! * If the format tracks locations, values retain their [`Span`] in the
//!   input.  Types that are deserialized from such values report errors at
//!   the original location:
//!
//! ```
//! use deser::Deserialize;
//! use deser_json::DeserializerConfig;
//! use deser_value::{Value, from_value};
//!
//! #[derive(Debug, Deserialize)]
//! struct Config {
//!     port: u16,
//! }
//!
//! let value: Value = DeserializerConfig::new()
//!     .track_locations(true)
//!     .from_str("{\n  \"port\": \"80\"\n}")
//!     .unwrap();
//! let err = from_value::<Config>(&value).unwrap_err();
//! assert_eq!((err.line(), err.column()), (Some(2), Some(11)));
//! ```
//!
//! # Duplicate Keys
//!
//! Map keys are unique.  If a map with duplicate keys is deserialized into
//! a value, the deserialization fails.
mod de;
mod format;
mod index;
mod macros;
mod map;
mod seq;
mod ser;
mod tree;
mod value;

pub use self::format::{Deserializer, from_value, to_value};
pub use self::index::ValueIndex;
pub use self::map::{IntoIter, Iter, IterMut, Keys, Map, MapKey, Values, ValuesMut};
pub use self::seq::Seq;
pub use self::value::{Kind, Meta, Span, Value};

// values must never prevent data from being shared between threads.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Value>();
};
