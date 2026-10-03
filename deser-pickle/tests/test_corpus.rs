//! Compares the deserializer and serializer with CPython.
//!
//! The cases in `data/pickle` hold pickles and what CPython's unpickler
//! made of them (see `scripts/update-pickle-test-data.sh`): the value as
//! this crate emits it or the error.  The pickles are the handwritten cases
//! of the script, values pickled with every protocol and the bytes literals
//! of CPython's pickle tests.  The deserializer has to accept what CPython
//! accepts (and reject what it rejects) and read the same values.  What
//! the serializer writes has to read back as the same value.
//!
//! Some differences are intended:
//!
//! * Python ignores data after the `STOP` opcode, here it's an error
//!   unless the pickles are read one after another.
//! * Strings with surrogates cannot be held by Rust strings.
//! * Python checks that keys and set items are hashable and merges equal
//!   ones (the comparison merges equal ones, it does not know that `1` and
//!   `True` are the same key).
//! * Huge memo indexes make CPython run out of memory.
use std::collections::HashMap;

use deser_value::Value;

use crate::common::{Py, hex};
use deser_pickle::{Deserializer, Kind, SerializerConfig};

const CASES: &str = include_str!("data/pickle/cases.json");

#[derive(Debug, deser::Deserialize)]
struct Case {
    sources: Vec<String>,
    input: String,
    error: Option<String>,
    value: Option<Value>,
    #[deser(default)]
    value_omitted: bool,
    #[deser(default)]
    trailing: bool,
}

impl Case {
    fn name(&self) -> &str {
        &self.sources[0]
    }

    fn input(&self) -> Vec<u8> {
        hex(&self.input)
    }
}

fn load() -> Vec<Case> {
    deser_json::from_str(CASES).unwrap()
}

fn items(value: &Value) -> &[Value] {
    value.as_seq().unwrap()
}

/// Returns `true` if the expected value has a string with surrogates.
fn has_surrogates(expected: &Value) -> bool {
    let Some(items) = expected.as_seq() else {
        return false;
    };
    items.first().and_then(|x| x.as_str()) == Some("strsur") || items.iter().any(has_surrogates)
}

/// Maps the ids of the expected values to the ids of the actual ones.
#[derive(Clone, Default)]
struct Ids {
    forward: HashMap<i64, u64>,
    backward: HashMap<u64, i64>,
}

impl Ids {
    fn pair(&mut self, expected: i64, actual: u64) -> bool {
        match (self.forward.get(&expected), self.backward.get(&actual)) {
            (Some(&a), Some(&e)) => a == actual && e == expected,
            (None, None) => {
                self.forward.insert(expected, actual);
                self.backward.insert(actual, expected);
                true
            }
            _ => false,
        }
    }
}

/// Merges repeated keys like Python: the last value at the position of the
/// first key.
fn merge_duplicates(entries: &[(Py, Py)]) -> Vec<(&Py, &Py)> {
    let mut merged: Vec<(&Py, &Py)> = Vec::new();
    for (key, value) in entries {
        match merged.iter_mut().find(|(other, _)| *other == key) {
            Some(entry) => entry.1 = value,
            None => merged.push((key, value)),
        }
    }
    merged
}

fn float_matches(text: &str, value: f64) -> bool {
    let expected: f64 = text.parse().unwrap();
    expected.to_bits() == value.to_bits() || expected.is_nan() && value.is_nan()
}

/// Compares the expected value with a value.
fn matches(expected: &Value, actual: &Py, ids: &mut Ids) -> bool {
    let items = items(expected);
    let kind = items[0].as_str().unwrap();
    match (kind, actual) {
        ("shared", Py::Shared(id, inner)) => {
            ids.pair(items[1].as_i64().unwrap(), *id) && matches(&items[2], inner, ids)
        }
        ("ref", Py::Ref(id)) => ids.pair(items[1].as_i64().unwrap(), *id),
        ("object", Py::Object(class, _, inner)) => {
            items[1].as_str() == Some(class.module())
                && items[2].as_str() == Some(class.name())
                && matches(&items[3], inner, ids)
        }
        ("none", Py::Null) => true,
        ("bool", Py::Bool(value)) => items[1].as_bool() == Some(*value),
        ("int", Py::Int(value)) => items[1].as_str() == Some(value.as_str()),
        ("float", Py::Float(value)) => float_matches(items[1].as_str().unwrap(), *value),
        ("str", Py::Str(value)) => items[1].as_str() == Some(value.as_str()),
        ("bytes", Py::Bytes(value)) => hex(items[1].as_str().unwrap()) == *value,
        // the strings of Python 2 are bytes in the dump, text here
        ("bytes", Py::Str(value)) => hex(items[1].as_str().unwrap()) == value.as_bytes(),
        ("bytearray", Py::ByteArray(value)) => hex(items[1].as_str().unwrap()) == *value,
        ("global", Py::Global(global)) => {
            items[1].as_str() == Some(global.module()) && items[2].as_str() == Some(global.name())
        }
        ("list", Py::Seq(None, values)) | ("tuple", Py::Seq(Some(Kind::Tuple), values)) => {
            let expected = self::items(&items[1]);
            expected.len() == values.len()
                && expected.iter().zip(values).all(|(e, a)| matches(e, a, ids))
        }
        ("set", Py::Seq(Some(Kind::Set), values))
        | ("frozenset", Py::Seq(Some(Kind::FrozenSet), values)) => {
            let mut unique: Vec<&Py> = Vec::new();
            for value in values {
                if !unique.contains(&value) {
                    unique.push(value);
                }
            }
            unordered_matches(self::items(&items[1]), &unique, ids)
        }
        ("dict", Py::Map(entries)) => {
            let expected = self::items(&items[1]);
            let entries = merge_duplicates(entries);
            expected.len() == entries.len()
                && expected.iter().zip(entries).all(|(e, (k, v))| {
                    let e = self::items(e);
                    matches(&e[0], k, ids) && matches(&e[1], v, ids)
                })
        }
        _ => false,
    }
}

/// Compares the items of a set in any order.
fn unordered_matches(expected: &[Value], actual: &[&Py], ids: &mut Ids) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    let Some((first, rest)) = expected.split_first() else {
        return true;
    };
    for (idx, candidate) in actual.iter().enumerate() {
        let mut attempt = ids.clone();
        if matches(first, candidate, &mut attempt) {
            let mut others = actual.to_vec();
            others.remove(idx);
            if unordered_matches(rest, &others, &mut attempt) {
                *ids = attempt;
                return true;
            }
        }
    }
    false
}

/// Cases with known differences and why.
const KNOWN: &[(&str, &str)] = &[(
    "deser:dict-duplicate-keys-int-bool",
    "Python merges the keys `1` and `True`",
)];

fn is_known(case: &Case) -> bool {
    KNOWN
        .iter()
        .any(|(name, _)| case.sources.iter().any(|x| x == name))
}

/// Returns `true` for the errors of CPython that are not about the format.
fn is_python_specific_error(error: &str) -> bool {
    error.contains("unhashable") || error.starts_with("MemoryError") || error == "RecursionError"
}

/// Deserializes the first pickle of the input.
fn first(input: &[u8]) -> Result<Py, deser::Error> {
    Deserializer::from_slice(input).deserialize()
}

#[test]
fn test_corpus_deserialize() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in load() {
        if is_known(&case) {
            continue;
        }
        let input = case.input();
        let rv = deser_pickle::from_slice::<Py>(&input);
        if let Some(ref error) = case.error {
            if is_python_specific_error(error) {
                continue;
            }
            checked += 1;
            if let Ok(value) = rv {
                failures.push(format!(
                    "{}: accepted ({})\n  got: {:?}",
                    case.name(),
                    error,
                    value
                ));
            }
            continue;
        }
        checked += 1;
        if case.value.as_ref().is_some_and(has_surrogates) {
            if rv.is_ok() {
                failures.push(format!("{}: accepted string with surrogates", case.name()));
            }
            continue;
        }
        if case.trailing && rv.is_ok() {
            failures.push(format!("{}: accepted trailing data", case.name()));
        }
        let rv = match case.trailing {
            true => first(&input),
            false => rv,
        };
        match rv {
            Err(err) => failures.push(format!("{}: rejected: {}", case.name(), err)),
            Ok(value) => {
                if let Some(ref expected) = case.value
                    && !matches(expected, &value, &mut Ids::default())
                {
                    failures.push(format!(
                        "{}: different value\n  expected: {:?}\n  got: {:?}",
                        case.name(),
                        expected,
                        value
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // the cases are not skipped by accident
    assert!(checked > 2600, "only {} cases were checked", checked);
}

#[test]
fn test_corpus_roundtrip() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in load() {
        if case.error.is_some() || is_known(&case) {
            continue;
        }
        let Ok(value) = first(&case.input()) else {
            continue;
        };
        for protocol in 2..=5 {
            let config = SerializerConfig::builder().protocol(protocol).build();
            let output = match config.to_vec(&value) {
                Ok(output) => output,
                Err(_) if value.has_late_cycle(protocol) => continue,
                Err(err) if protocol < 4 && err.message().contains("protocol 4") => continue,
                Err(err) => {
                    failures.push(format!(
                        "{} ({}): not written: {}",
                        case.name(),
                        protocol,
                        err
                    ));
                    continue;
                }
            };
            checked += 1;
            match deser_pickle::from_slice::<Py>(&output) {
                Ok(again) => {
                    let (expected, again) = (normalize(&value), normalize(&again));
                    if expected != again {
                        failures.push(format!(
                            "{} ({}): different value\n  expected: {:?}\n  got:      {:?}",
                            case.name(),
                            protocol,
                            expected,
                            again
                        ));
                    }
                }
                Err(err) => failures.push(format!(
                    "{} ({}): not read back: {}",
                    case.name(),
                    protocol,
                    err
                )),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(checked > 3000, "only {} cases were checked", checked);
}

/// Normalizes the differences that a round trip makes: ids are renumbered.
fn normalize(value: &Py) -> Py {
    fn walk(value: &Py) -> Py {
        match value {
            Py::Object(class, form, inner) => {
                Py::Object(class.clone(), *form, Box::new(walk(inner)))
            }
            Py::Shared(id, inner) => Py::Shared(*id, Box::new(walk(inner))),
            Py::Seq(kind, items) => Py::Seq(*kind, items.iter().map(walk).collect()),
            Py::Map(entries) => Py::Map(entries.iter().map(|(k, v)| (walk(k), walk(v))).collect()),
            other => other.clone(),
        }
    }
    walk(value).normalized()
}

/// Writes what the serializer writes for the cases to the file in
/// `DESER_PICKLE_DUMP` (for `scripts/pickle/check-serializer.py`, which
/// checks that CPython reads it as the same value).
#[test]
fn test_corpus_dump_serialized() {
    let Some(path) = std::env::var_os("DESER_PICKLE_DUMP") else {
        return;
    };
    let mut lines = Vec::new();
    for case in load() {
        if case.error.is_some() || is_known(&case) {
            continue;
        }
        let input = case.input();
        let Ok(value) = first(&input) else {
            continue;
        };
        for protocol in 2..=5 {
            let config = SerializerConfig::builder().protocol(protocol).build();
            if let Ok(output) = config.to_vec(&value) {
                lines.push(format!(
                    "{{\"name\":{:?},\"protocol\":{},\"input\":\"{}\",\"output\":\"{}\"}}",
                    case.name(),
                    protocol,
                    crate::common::to_hex(&input),
                    crate::common::to_hex(&output)
                ));
            }
        }
    }
    std::fs::write(path, lines.join("\n")).unwrap();
}
