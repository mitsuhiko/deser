use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use deser_value::{Map, Seq, Value, from_value, to_value, value};

// deep enough to overflow the stack of test threads with any recursion
const DEPTH: usize = if cfg!(miri) { 300 } else { 100_000 };

fn nested_json() -> String {
    format!("{}null{}", "[{\"a\":".repeat(DEPTH), "}]".repeat(DEPTH))
}

#[test]
fn test_deep_values() {
    let json = nested_json();
    let value: Value = deser_json::from_str(&json).unwrap();

    let cloned = value.clone();
    assert_eq!(value, cloned);

    let mut a = DefaultHasher::new();
    value.hash(&mut a);
    let mut b = DefaultHasher::new();
    cloned.hash(&mut b);
    assert_eq!(a.finish(), b.finish());

    let debug = format!("{:?}", value);
    assert_eq!(debug.len(), json.len() + DEPTH);
    // (pretty printing is not tested as the indentation makes the output
    // quadratic in size)

    assert_eq!(deser_json::to_string(&value).unwrap(), json);

    let converted: Value = from_value(&value).unwrap();
    assert_eq!(converted, value);
    assert_eq!(to_value(&value).unwrap(), value);

    // values that differ deep down
    let other: Value = deser_json::from_str(&json.replace("null", "1")).unwrap();
    assert_ne!(value, other);
}

#[test]
fn test_deep_keys() {
    // containers as keys are nested too
    let mut key = value!(null);
    for _ in 0..DEPTH {
        let mut map = Map::new();
        map.insert(key, 1);
        key = Value::from(map);
    }
    let cloned = key.clone();
    assert_eq!(key, cloned);
    let _ = format!("{:?}", key);
}

#[test]
fn test_deep_drop() {
    let mut value = value!(null);
    for _ in 0..DEPTH {
        let mut seq = Seq::new();
        seq.push(value);
        value = Value::from(seq);
    }
    drop(value);

    let mut value = value!(null);
    for _ in 0..DEPTH {
        let mut map = Map::new();
        map.insert("a", value);
        value = Value::from(map);
    }
    let mut map = value.as_map().unwrap().clone();
    map.clear();
    drop(value);

    let mut value = value!(null);
    for _ in 0..DEPTH {
        value = Value::from(vec![value]);
    }
    let seq = value.into_kind().as_seq().unwrap().clone();
    drop(seq.into_iter());
}
