//! Runs the Hjson test suite (see `scripts/update-hjson-test-data.sh`).
//!
//! Cases whose name starts with `fail` are invalid, the values of the
//! others are compared with the expected values in `NAME_result.json`.
//! The files are parsed in memory and from streams.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use deser_value::Value;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/hjson-tests");

/// Returns the test cases (with their names relative to the suite) and if
/// they are valid.
fn cases() -> Vec<(String, PathBuf, bool)> {
    let mut rv = Vec::new();
    for dir in [Path::new(DATA).to_path_buf(), Path::new(DATA).join("extra")] {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let file_name = path.file_name().unwrap().to_str().unwrap();
            let Some((stem, _)) = file_name.split_once("_test.") else {
                continue;
            };
            let name = path.strip_prefix(DATA).unwrap().to_str().unwrap();
            let name = name.replace('\\', "/");
            let valid = !stem.starts_with("fail");
            rv.push((name, path, valid));
        }
    }
    rv.sort();
    rv
}

/// Checks a value against the expected value (read from JSON).
///
/// Numbers are compared by value (Hjson numbers are implicit values) and
/// strings must be strings (`null` without quotes is null, a string with
/// quotes is not).
fn check(actual: &Value, expected: &Value) -> Result<(), String> {
    let ok = if expected.is_null() {
        actual.is_null()
    } else if let Some(expected) = expected.as_bool() {
        actual.as_bool() == Some(expected)
    } else if let Some(expected) = expected.as_str() {
        actual.as_str() == Some(expected)
    } else if let Some(expected) = expected.as_f64() {
        actual.as_f64() == Some(expected)
    } else if let Some(expected) = expected.as_seq() {
        let Some(actual) = actual.as_seq() else {
            return Err(format!("expected a sequence, got {actual:?}"));
        };
        if actual.len() != expected.len() {
            return Err(format!("expected {} items, got {actual:?}", expected.len()));
        }
        for (actual, expected) in actual.iter().zip(expected.iter()) {
            check(actual, expected)?;
        }
        true
    } else if let Some(expected) = expected.as_map() {
        let Some(actual) = actual.as_map() else {
            return Err(format!("expected a map, got {actual:?}"));
        };
        let actual: BTreeMap<&str, &Value> = actual
            .iter()
            .map(|(key, value)| (key.as_str().unwrap(), value))
            .collect();
        let expected: BTreeMap<&str, &Value> = expected
            .iter()
            .map(|(key, value)| (key.as_str().unwrap(), value))
            .collect();
        if actual.keys().ne(expected.keys()) {
            return Err(format!(
                "expected the keys {:?}, got {actual:?}",
                expected.keys()
            ));
        }
        for (key, actual) in &actual {
            check(actual, expected[key]).map_err(|err| format!("{key}: {err}"))?;
        }
        true
    } else {
        panic!("unexpected expected value {expected:?}");
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

    let mut reader = Reader::new(Chunked(input, size), deser_hjson::DeserializerConfig::new());
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
fn test_hjson_suite() {
    let mut count = 0;
    let mut failures = Vec::new();
    for (name, path, valid) in cases() {
        count += 1;
        let input = fs::read(&path).unwrap();
        let expected = valid.then(|| {
            let (stem, _) = path.to_str().unwrap().split_once("_test.").unwrap();
            let expected = fs::read(format!("{stem}_result.json")).unwrap();
            deser_json::from_slice::<Value>(&expected).unwrap()
        });

        let mut results = vec![(
            "in memory".to_string(),
            deser_hjson::from_slice::<Value>(&input),
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
            let failure = match (&expected, rv) {
                (Some(expected), Ok(value)) => check(&value, expected).err(),
                (Some(_), Err(err)) => Some(format!("failed: {err}")),
                (None, Ok(value)) => Some(format!("accepted: {value:?}")),
                (None, Err(_)) => None,
            };
            if let Some(failure) = failure {
                failures.push(format!("{name} ({how}): {failure}"));
            }
        }
    }

    assert!(count > 80, "only {count} tests found");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
