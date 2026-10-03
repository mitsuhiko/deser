//! Compares the deserializer and serializer with PHP.
//!
//! The cases in `data/php-serialize` hold inputs and what PHP's
//! `unserialize` made of them (see `scripts/update-php-test-data.sh`):
//! the value, the error or the warnings, and what `serialize` writes for
//! the value.  The deserializer has to accept what PHP accepts (and reject
//! what it rejects) and read the same values, the serializer has to write
//! what PHP writes.
//!
//! Some differences are intended:
//!
//! * PHP ignores data after the value and clamps integers that are out of
//!   range (with a warning), both are errors here.
//! * References are not resolved, they are compared with what they
//!   refer to only by their position.
//! * PHP fails for enum cases of classes that do not exist and for values
//!   nested deeper than 4096 levels, which is not about the format.
use deser_value::Value;

use crate::common::{Key, Php, hex};
use deser_php::Visibility;

const CASES: &str = include_str!("data/php-serialize/cases.json");

#[derive(Debug, deser::Deserialize)]
struct Case {
    sources: Vec<String>,
    input: Option<String>,
    input_hex: Option<String>,
    #[deser(default)]
    error: bool,
    value: Option<Value>,
    #[deser(default)]
    value_omitted: bool,
    #[deser(default)]
    canonical: bool,
    reserialized: Option<String>,
    reserialized_hex: Option<String>,
    #[deser(default)]
    diagnostics: Vec<String>,
    fatal: Option<String>,
    #[allow(dead_code)]
    error_offset: Option<u64>,
}

impl Case {
    fn name(&self) -> &str {
        &self.sources[0]
    }

    fn input(&self) -> Vec<u8> {
        match (&self.input, &self.input_hex) {
            (Some(input), _) => input.as_bytes().to_vec(),
            (None, Some(input)) => hex(input),
            _ => panic!("case without input"),
        }
    }

    /// Returns what `serialize` writes for the value.
    fn serialized(&self) -> Vec<u8> {
        if self.canonical {
            return self.input();
        }
        match (&self.reserialized, &self.reserialized_hex) {
            (Some(output), _) => output.as_bytes().to_vec(),
            (None, Some(output)) => hex(output),
            _ => panic!("case without output"),
        }
    }

    fn has_diagnostic(&self, text: &str) -> bool {
        self.diagnostics.iter().any(|x| x.contains(text))
    }
}

fn load() -> Vec<Case> {
    deser_json::from_str(CASES).unwrap()
}

fn items(value: &Value) -> &[Value] {
    value.as_seq().unwrap()
}

/// Returns the key and the visibility of a property name.
fn demangle(name: &str) -> (&str, Option<Visibility>) {
    if let Some(rest) = name.strip_prefix('\0')
        && let Some((class, name)) = rest.split_once('\0')
        && !class.is_empty()
    {
        let visibility = match class {
            "*" => Visibility::Protected,
            class => Visibility::Private(class.into()),
        };
        return (name, Some(visibility));
    }
    (name, None)
}

/// Compares a key of the dump of PHP with a key.
fn key_matches(expected: &Value, key: &Key, is_object: bool) -> bool {
    let items = items(expected);
    match (items[0].as_str().unwrap(), &key.name) {
        ("int", Php::Int(value)) => items[1].as_i64() == Some(*value) && key.visibility.is_none(),
        ("string", Php::Str(name)) => {
            let (expected, visibility) = match is_object {
                true => demangle(items[1].as_str().unwrap()),
                false => (items[1].as_str().unwrap(), None),
            };
            expected == name && visibility == key.visibility
        }
        ("bytes", Php::Bytes(name)) => hex(items[1].as_str().unwrap()) == *name,
        _ => false,
    }
}

/// Merges the entries with the same key like PHP: the last value is used
/// at the position of the first.
fn merge_duplicates<'a>(entries: Vec<(&'a Key, &'a Php)>) -> Vec<(&'a Key, &'a Php)> {
    let mut merged: Vec<(&Key, &Php)> = Vec::new();
    for (key, value) in entries {
        match merged.iter_mut().find(|(other, _)| *other == key) {
            Some(entry) => entry.1 = value,
            None => merged.push((key, value)),
        }
    }
    merged
}

/// Compares the dump of a value of PHP with a value.
fn matches(expected: &Value, actual: &Php) -> bool {
    let items = items(expected);
    let kind = items[0].as_str().unwrap();
    // references are not resolved
    match (kind, actual) {
        (_, Php::Ref(_)) => return true,
        ("ref" | "objref", _) => return false,
        ("refdef", _) => return matches(&items[2], actual),
        _ => {}
    }
    match (kind, actual) {
        ("null", Php::Null) => true,
        ("bool", Php::Bool(value)) => items[1].as_bool() == Some(*value),
        ("int", Php::Int(value)) => items[1].as_i64() == Some(*value),
        ("float", Php::Float(value)) => {
            let text = items[1].as_str().unwrap();
            let expected: f64 = match text {
                "INF" => f64::INFINITY,
                "-INF" => f64::NEG_INFINITY,
                "NAN" => f64::NAN,
                text => text.parse().unwrap(),
            };
            expected.to_bits() == value.to_bits() || expected.is_nan() && value.is_nan()
        }
        ("string", Php::Str(value)) => items[1].as_str() == Some(value.as_str()),
        ("bytes", Php::Bytes(value)) => hex(items[1].as_str().unwrap()) == *value,
        ("array", Php::List(values)) => {
            let entries = self::items(&items[1]);
            entries.len() == values.len()
                && entries
                    .iter()
                    .zip(values)
                    .enumerate()
                    .all(|(idx, (entry, value))| {
                        let entry = self::items(entry);
                        self::items(&entry[0])[1].as_i64() == Some(idx as i64)
                            && matches(&entry[1], value)
                    })
        }
        ("array", Php::Map(None, entries)) => entries_match(&items[1], entries, false),
        ("object", Php::Map(Some(class), entries)) => {
            items[2].as_str() == Some(class.as_str()) && entries_match(&items[3], entries, true)
        }
        // PHP does not show the payload of custom serialized objects
        ("object", Php::Classed(class, payload)) => {
            items[2].as_str() == Some(class.as_str())
                && self::items(&items[3]).is_empty()
                && matches!(**payload, Php::Bytes(_))
        }
        ("enum", Php::Classed(class, case)) => {
            items[1].as_str() == Some(class.as_str())
                && matches!(**case, Php::Str(ref case) if items[2].as_str() == Some(case.as_str()))
        }
        _ => false,
    }
}

fn entries_match(expected: &Value, entries: &[(Key, Php)], is_object: bool) -> bool {
    let expected = items(expected);
    let entries = merge_duplicates(entries.iter().map(|(key, value)| (key, value)).collect());
    expected.len() == entries.len()
        && expected.iter().zip(entries).all(|(entry, (key, value))| {
            let entry = items(entry);
            key_matches(&entry[0], key, is_object) && matches(&entry[1], value)
        })
}

/// Cases with known differences and why.
const KNOWN: &[(&str, &str)] = &[
    (
        "deser:object-incomplete-class-name-property",
        "PHP keeps the class of objects it cannot load in this property",
    ),
    (
        "php-src/ext/standard/tests/serialize/bug69152.phpt",
        "PHP keeps the class of objects it cannot load in this property",
    ),
    (
        "deser:object-class-high-byte",
        "class names that are not valid UTF-8 are not supported",
    ),
    (
        "php-src/ext/standard/tests/serialize/006.phpt",
        "class names that are not valid UTF-8 are not supported",
    ),
    (
        "deser:ref-upper-to-overwritten",
        "references to values that were overwritten by a duplicate key",
    ),
    (
        "deser:ref-upper-overwritten-target",
        "references to values that were overwritten by a duplicate key",
    ),
    (
        "php-src/ext/standard/tests/serialize/overwrite_untyped_ref.phpt",
        "references to values that were overwritten by a duplicate key",
    ),
];

fn is_known(case: &Case) -> bool {
    KNOWN
        .iter()
        .any(|(name, _)| case.sources.iter().any(|x| x == name))
}

#[test]
fn test_corpus_deserialize() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in load() {
        if case.fatal.is_some() || is_known(&case) {
            continue;
        }
        let input = case.input();
        let rv = deser_php::from_slice::<Php>(&input);
        checked += 1;
        let php_ok = case.value.is_some() || case.value_omitted;
        let expect_error = if input.is_empty() {
            // PHP returns `false` for empty input without error
            true
        } else if php_ok {
            // intended differences
            case.has_diagnostic("Extra data")
                || case.has_diagnostic("Numerical result out of range")
        } else if case.has_diagnostic("not found")
            || case.has_diagnostic("Undefined constant")
            || case.has_diagnostic("Maximum depth")
        {
            // errors that are not about the format
            continue;
        } else {
            assert!(case.error);
            true
        };
        match rv {
            Ok(_) if expect_error => failures.push(format!("{}: accepted", case.name())),
            Err(err) if !expect_error => {
                failures.push(format!("{}: rejected: {}", case.name(), err))
            }
            Ok(value) => {
                if let Some(ref expected) = case.value
                    && !matches(expected, &value)
                {
                    failures.push(format!(
                        "{}: different value\n  expected: {:?}\n  got: {:?}",
                        case.name(),
                        expected,
                        value
                    ));
                }
            }
            Err(_) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // the cases are not skipped by accident
    assert!(checked > 700, "only {} cases were checked", checked);
}

#[test]
fn test_corpus_serialize() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in load() {
        if case.fatal.is_some()
            || is_known(&case)
            || !case.diagnostics.is_empty()
            || case.value.is_none() && !case.value_omitted
        {
            continue;
        }
        let input = case.input();
        let Ok(value) = deser_php::from_slice::<Php>(&input) else {
            continue;
        };
        let expected = case.serialized();
        // duplicate keys are merged by PHP.  References are written as
        // they are while PHP writes them for the same objects (like enum
        // cases) and in its own way (`R:` to objects is `r:`).
        let has_references = |x: &[u8]| x.windows(2).any(|x| x == b"R:" || x == b"r:");
        if has_duplicate_keys(&value)
            || (has_references(&input) || has_references(&expected)) && expected != input
        {
            continue;
        }
        let output = deser_php::to_vec(&value).unwrap();
        checked += 1;
        if output != expected {
            failures.push(format!(
                "{}:\n  expected: {}\n  got:      {}",
                case.name(),
                String::from_utf8_lossy(&expected),
                String::from_utf8_lossy(&output)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(checked > 400, "only {} cases were checked", checked);
}

/// Returns `true` if a value contains a map with a key more than once.
fn has_duplicate_keys(value: &Php) -> bool {
    match value {
        Php::List(items) => items.iter().any(has_duplicate_keys),
        Php::Map(_, entries) => {
            entries
                .iter()
                .enumerate()
                .any(|(idx, (key, _))| entries[..idx].iter().any(|(other, _)| other == key))
                || entries.iter().any(|(_, value)| has_duplicate_keys(value))
        }
        Php::Classed(_, value) => has_duplicate_keys(value),
        _ => false,
    }
}
