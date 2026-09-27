//! Runs the JSON5 test suite (see `scripts/update-json5-test-data.sh`).
//!
//! `.json` and `.json5` files are valid, their values are compared with
//! the ones computed by the reference (ECMAScript).  `.js` and `.txt` files
//! are invalid.  The files are parsed in memory and from streams.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use deser_value::Value;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/json5-tests");

/// Valid files whose values cannot be compared, they are only parsed.
const UNCOMPARED: &[(&str, &str)] = &[(
    "objects/duplicate-keys.json",
    "the last duplicate key wins in ECMAScript, deser-value rejects them",
)];

/// Returns the test files with their names relative to the suite.
fn files() -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, rv: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, rv);
            } else {
                rv.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    walk(Path::new(DATA), &mut paths);
    let mut rv: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let name = path.strip_prefix(DATA).unwrap().to_str().unwrap();
            (name.replace('\\', "/"), path)
        })
        .collect();
    rv.sort();
    rv
}

/// Checks a value against the encoding of the expected value.
///
/// The expected value is a sequence of its kind and its data, numbers are
/// strings and maps are sequences of keys and values.
fn check(actual: &Value, expected: &Value) -> Result<(), String> {
    let kind = expected[0].as_str().unwrap();
    let data = || &expected[1];
    let ok = match kind {
        "null" => actual.is_null(),
        "bool" => actual.as_bool() == data().as_bool(),
        "str" => actual.as_str() == data().as_str(),
        "number" => {
            let value: f64 = data().as_str().unwrap().parse().unwrap();
            match actual.as_f64() {
                Some(actual) if value.is_nan() => actual.is_nan(),
                // integers do not keep the sign of -0
                Some(actual) => actual == value,
                None => false,
            }
        }
        "seq" => {
            let (Some(actual), Some(expected)) = (actual.as_seq(), data().as_seq()) else {
                return Err(format!("expected a sequence, got {actual:?}"));
            };
            if actual.len() != expected.len() {
                return Err(format!("expected {} items, got {actual:?}", expected.len()));
            }
            for (actual, expected) in actual.iter().zip(expected.iter()) {
                check(actual, expected)?;
            }
            true
        }
        "map" => {
            let Some(actual) = actual.as_map() else {
                return Err(format!("expected a map, got {actual:?}"));
            };
            // the order of keys in ECMAScript differs (integer keys come
            // first) and the last of duplicate keys wins
            let actual: BTreeMap<&str, &Value> = actual
                .iter()
                .map(|(key, value)| (key.as_str().unwrap(), value))
                .collect();
            let expected: BTreeMap<&str, &Value> = data()
                .as_seq()
                .unwrap()
                .iter()
                .map(|item| (item[0].as_str().unwrap(), &item[1]))
                .collect();
            if actual.keys().ne(expected.keys()) {
                return Err(format!(
                    "expected the keys {:?}, got {actual:?}",
                    expected.keys()
                ));
            }
            for (actual, expected) in actual.values().zip(expected.values()) {
                check(actual, expected)?;
            }
            true
        }
        _ => panic!("unknown kind {kind}"),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("expected {expected:?}, got {actual:?}"))
    }
}

/// Parses the input from a stream in chunks of the given size.
#[cfg(feature = "io")]
fn read_chunked(input: &[u8], size: usize, borrowed: bool) -> Result<Value, deser::Error> {
    use deser::io::Reader;

    struct Chunked<'a>(&'a [u8], usize);

    impl std::io::Read for Chunked<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let len = self.1.min(buf.len()).min(self.0.len());
            buf[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        }
    }

    let mut reader = Reader::new(Chunked(input, size), deser_json5::DeserializerConfig::new());
    let value = if borrowed {
        // from the frame of the value
        reader.read_borrowed::<Value>()?
    } else {
        // while the input arrives
        reader.read::<Value>()?
    };
    let value = value.ok_or_else(|| deser::Error::new(deser::ErrorKind::EndOfFile, "no value"))?;
    reader.end()?;
    Ok(value)
}

#[test]
fn test_json5_suite() {
    let expected: BTreeMap<String, Value> =
        deser_json::from_str(include_str!("data/json5-tests.expected.json")).unwrap();

    let mut count = 0;
    let mut failures = Vec::new();
    for (name, path) in files() {
        let valid = if name.ends_with(".json") || name.ends_with(".json5") {
            true
        } else if name.ends_with(".js") || name.ends_with(".txt") {
            false
        } else {
            continue;
        };
        count += 1;
        let input = fs::read(&path).unwrap();

        if UNCOMPARED.iter().any(|(uncompared, _)| *uncompared == name) {
            if let Err(err) = deser_json5::from_slice::<deser::de::Recording>(&input) {
                failures.push(format!("{name}: failed: {err}"));
            }
            continue;
        }

        let mut results = vec![(
            "in memory".to_string(),
            deser_json5::from_slice::<Value>(&input),
        )];
        #[cfg(feature = "io")]
        for size in [1, 2, 3, 7, input.len().max(1)] {
            for borrowed in [false, true] {
                let how = if borrowed { "framed" } else { "fed" };
                results.push((
                    format!("{how} in chunks of {size}"),
                    read_chunked(&input, size, borrowed),
                ));
            }
        }

        for (how, rv) in results {
            let failure = match (valid, rv) {
                (true, Ok(value)) => check(&value, &expected[&name]).err(),
                (true, Err(err)) => Some(format!("failed: {err}")),
                (false, Ok(value)) => Some(format!("accepted: {value:?}")),
                (false, Err(_)) => None,
            };
            if let Some(failure) = failure {
                failures.push(format!("{name} ({how}): {failure}"));
            }
        }
    }

    assert!(count > 100, "only {count} tests found");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
