//! Adapters to use [serde](https://serde.rs/) types with deser.
//!
//! This crate provides the [`Serde`] adapter which serializes and
//! deserializes values with their serde implementations.  It's useful for
//! types from crates which only support serde:
//!
//! ```
//! use deser::{Deserialize, Serialize};
//! use deser_serde::Serde;
//!
//! #[derive(serde::Serialize, serde::Deserialize)]
//! struct Point {
//!     x: i32,
//!     y: i32,
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! struct Shape {
//!     name: String,
//!     #[deser(as = Vec<Serde>)]
//!     points: Vec<Point>,
//!     #[deser(as = Serde)]
//!     extra: serde_json::Value,
//! }
//!
//! let shape: Shape = deser_json::from_str(
//!     r#"{
//!         "name": "line",
//!         "points": [{"x": 1, "y": 2}, {"x": 3, "y": 4}],
//!         "extra": [true]
//!     }"#,
//! )
//! .unwrap();
//! assert_eq!(shape.points[1].y, 4);
//! assert_eq!(shape.extra, serde_json::json!([true]));
//! ```
//!
//! Adapters compose with containers (`Vec<Serde>`, `Option<Serde>`, ...),
//! for more information see [`deser::adapters`](deser_core::adapters).  To use the adapter
//! outside of the derive, wrap values in [`As`](deser_core::adapters::As).
//!
//! # Data Model
//!
//! serde values are mapped to the deser data model like this:
//!
//! * Integers, floats, booleans, chars, strings and bytes map to the
//!   respective atoms.  128 bit integers are extension values like the ones
//!   of deser.
//! * `None`, `()` and unit structs are null, `Some` and newtype structs are
//!   the value they hold.
//! * Sequences and tuples are sequences, maps and structs are maps.
//! * Enums are externally tagged (the default in serde and deser): unit
//!   variants are strings, all others are maps with the variant name as
//!   single key.
//!
//! When deserializing, extension values that serde does not know (like
//! date-times) are passed to serde as their fallback atom (for instance a
//! string).  Map keys are also parsed from strings if serde asks for a
//! number or boolean, so `HashMap<u32, _>` works with JSON.  Borrowing is
//! supported: serde types which borrow (like `&'de str`) can borrow from
//! the data if the format passes it on borrowed.
//!
//! [`is_human_readable`](serde::Serializer::is_human_readable) is always
//! `true`.
//!
//! Missing struct fields are handled like serde: they are `None` if the
//! type deserializes a missing value as option, which is the case for
//! `Option<T>`.
//!
//! # Buffering
//!
//! serde and deser drive values in opposite directions: with serde the
//! value is serialized into a serializer by nested calls and pulls
//! from a deserializer, with deser the value is walked by the driver and
//! events are pushed into deserializers.  So [`Serde`] buffers the events
//! of compound values.  For atoms (the typical case, like `Url` or
//! `IpAddr`) there is no buffering.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

use std::borrow::Cow;

use deser_core::Deserialize;
use deser_core::Serialize;
use deser_core::State;
use deser_core::de::SinkHandle;
use deser_core::ser::Emit;

mod buffered;
mod de;
mod error;
mod ser;
mod sink;

use crate::de::MissingDe;
use crate::sink::RootSink;

/// Returns the value for a missing field like serde does.
fn missing_value<'de, T: serde::Deserialize<'de>>() -> Option<T> {
    T::deserialize(MissingDe).ok()
}

/// Adapter that uses the serde implementations of a type.
///
/// Compound values are buffered, see the [crate documentation](crate) for
/// more information.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::adapters::As;
/// use deser_serde::Serde;
///
/// let value: As<BTreeMap<u32, String>, Serde> =
///     deser_json::from_str(r#"{"1": "a"}"#).unwrap();
/// assert_eq!(value[&1], "a");
/// assert_eq!(deser_json::to_string(&value).unwrap(), r#"{"1":"a"}"#);
/// ```
pub struct Serde;

impl<T: serde::Serialize + ?Sized> Serialize<T> for Serde {
    fn serialize<'a>(value: &'a T, state: &mut State) -> Result<Emit<'a>, deser_core::Error> {
        buffered::serialize(value, state)
    }

    fn is_optional(value: &T) -> bool {
        ser::is_none(value)
    }
}

impl<'de, T: serde::Deserialize<'de> + Send> Deserialize<'de, T> for Serde {
    fn deserialize_into<'out>(
        out: &'out mut Option<T>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(RootSink::new(out, buffered::Buffer::default()), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("serde value")
    }

    fn initial_value() -> Option<T> {
        missing_value()
    }
}
