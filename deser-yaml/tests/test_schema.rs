//! Checks scalar resolution against the YAML schema tests (vendored in
//! `tests/data/yaml-test-schema`).
//!
//! The data lists for every input how it is loaded under the YAML 1.2 core
//! schema and YAML 1.1.  Inputs marked as error (like `!!bool FaLSE`) must
//! fail.
use std::fs;
use std::path::Path;

use deser::Error;
use deser_yaml::{DeserializerConfig, Version};

mod common;

use common::{parse_json_stream, Value};

const DATA: &str = "tests/data/yaml-test-schema";

/// Loads the test data of a schema: the JSON file has the expected values,
/// the YAML file additionally lists the inputs that must fail.
fn load_cases(schema: &str) -> Vec<(String, Option<Value>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(DATA);
    let json = fs::read_to_string(root.join(format!("schema-{}.json", schema))).unwrap();
    let mut cases = Vec::new();
    match parse_json_stream(&json).pop() {
        Some(Value::Map(items)) => {
            for (key, value) in items {
                let (Value::Str(input), Value::Seq(info)) = (key, value) else {
                    panic!("unexpected schema data");
                };
                cases.push((input, Some(expected_value(&info))));
            }
        }
        _ => panic!("unexpected schema data"),
    }

    // the error cases only exist in the YAML file as `'input': error`
    let yaml = fs::read_to_string(root.join(format!("schema-{}.yaml", schema))).unwrap();
    for line in yaml.lines() {
        if let Some(input) = line
            .strip_suffix("': error")
            .and_then(|x| x.strip_prefix('\''))
        {
            cases.push((input.replace("''", "'"), None));
        }
    }
    cases
}

fn expected_value(info: &[Value]) -> Value {
    let (Value::Str(ty), Value::Str(loaded)) = (&info[0], &info[1]) else {
        panic!("unexpected schema data");
    };
    match (ty.as_str(), loaded.as_str()) {
        ("null", _) => Value::Null,
        ("bool", "true()") => Value::Bool(true),
        ("bool", "false()") => Value::Bool(false),
        ("int", value) => Value::Int(value.parse().unwrap()),
        ("float", value) => Value::Float(value.parse().unwrap()),
        ("inf", "inf()") => Value::Float(f64::INFINITY),
        ("inf", "inf-neg()") => Value::Float(f64::NEG_INFINITY),
        ("nan", _) => Value::Float(f64::NAN),
        ("str", value) => Value::Str(value.into()),
        other => panic!("unexpected schema data {:?}", other),
    }
}

fn load(input: &str, version: Version) -> Result<Value, Error> {
    DeserializerConfig::new().version(version).from_str(input)
}

fn run_schema(schema: &str, version: Version, directive: &str) {
    let cases = load_cases(schema);
    assert!(cases.len() > 200);
    let mut failures = Vec::new();
    for (input, expected) in cases {
        let yaml = format!("{}{}", directive, input.replace("#empty", ""));
        match (load(&yaml, version), expected) {
            (Ok(value), Some(expected)) if value == expected => {}
            (Err(_), None) => {}
            (rv, expected) => {
                failures.push(format!("{:?}: expected {:?}, got {:?}", yaml, expected, rv))
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn test_core_schema() {
    run_schema("core", Version::V1_2, "");
}

#[test]
fn test_core_schema_with_directive() {
    run_schema("core", Version::V1_1, "%YAML 1.2\n--- ");
}

#[test]
fn test_yaml11() {
    run_schema("yaml11", Version::V1_1, "");
}

#[test]
fn test_yaml11_with_directive() {
    run_schema("yaml11", Version::V1_2, "%YAML 1.1\n--- ");
}
