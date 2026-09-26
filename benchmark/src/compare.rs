//! Compares deserialized values.
//!
//! The JSON parsers (deser-json and serde_json by default) do not round all
//! floats with 17 significant digits correctly, the results can be off in
//! the last bit.  Values that only differ by that still count as equal.
use deser::Serialize;
use deser_value::{Kind, Value};

/// How a deserialized value compares to the expected one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equality {
    /// The values are equal.
    Exact,
    /// Floats differ in the last bits, the rest is equal.
    Floats,
    /// The values differ.
    Different,
}

pub fn compare<T: Serialize + PartialEq>(value: &T, expected: &T) -> Equality {
    if value == expected {
        return Equality::Exact;
    }
    let value = deser_value::to_value(value).unwrap();
    let expected = deser_value::to_value(expected).unwrap();
    if approx_eq(&value, &expected) {
        Equality::Floats
    } else {
        Equality::Different
    }
}

fn approx_eq(a: &Value, b: &Value) -> bool {
    match (a.kind(), b.kind()) {
        (Kind::F64(a), Kind::F64(b)) => {
            // f32 values are compared as f64, allow for the precision of f32
            a == b || (a - b).abs() <= a.abs().max(b.abs()) * 1e-7
        }
        (Kind::Seq(a), Kind::Seq(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| approx_eq(a, b))
        }
        // hash maps do not have the same order
        (Kind::Map(a), Kind::Map(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| approx_eq(a, b)))
        }
        _ => a == b,
    }
}
