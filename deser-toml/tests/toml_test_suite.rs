//! Runs the toml-test suite (vendored in `tests/data`).
//!
//! Only the tests that apply to TOML 1.1 are vendored.  Valid tests are
//! parsed and compared with the expected (tagged) JSON.  The parsed value
//! is also serialized again and the output has to parse to the same value.
//! Invalid tests have to fail to parse.
//!
//! This uses a custom harness so that every case is reported individually
//! without pulling in dependencies.  Arguments that are not flags filter the
//! cases by name.  If filters are given, the details of known failures are
//! shown as well.
//!
//! Tests that are expected to fail are listed in
//! `toml_test_suite_known_failures.txt`.  The run fails if any other test
//! fails or if a listed test passes.  Set `DESER_TOML_BLESS=1` to rewrite
//! the list from the results.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic;
use std::path::Path;
use std::process::ExitCode;

mod common;

use common::Value;

const KNOWN_FAILURES: &str = "tests/toml_test_suite_known_failures.txt";
const SUITE: &str = "tests/data/toml-test";

struct Case {
    /// The path of the test without extension, e.g. `valid/array/array`.
    name: String,
    input: Vec<u8>,
    /// The expected JSON for valid tests.
    json: Option<String>,
}

enum Outcome {
    Pass,
    Fail(String),
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| arg.to_lowercase())
        .collect();
    let list_only = std::env::args().any(|arg| arg == "--list");
    let bless = std::env::var("DESER_TOML_BLESS").is_ok_and(|x| x == "1");

    let cases = load_cases(&root.join(SUITE));
    let known_failures = load_known_failures(&root.join(KNOWN_FAILURES));
    let selected: Vec<&Case> = cases
        .iter()
        .filter(|case| {
            filters.is_empty() || filters.iter().any(|f| case.name.to_lowercase().contains(f))
        })
        .collect();

    if list_only {
        for case in &selected {
            println!("{}: test", case.name);
        }
        return ExitCode::SUCCESS;
    }

    // panics are reported as failures, silence the default hook
    panic::set_hook(Box::new(|_| {}));

    let mut passed = 0;
    let mut failures = BTreeSet::new();
    let mut new_failures = Vec::new();
    let mut unexpected_passes = Vec::new();

    for case in &selected {
        let is_known = known_failures.contains_key(&case.name);
        let outcome = match panic::catch_unwind(|| run_case(case)) {
            Ok(outcome) => outcome,
            Err(_) => Outcome::Fail("panicked".into()),
        };
        match outcome {
            Outcome::Pass => {
                passed += 1;
                if is_known {
                    unexpected_passes.push(case.name.clone());
                }
            }
            Outcome::Fail(details) => {
                failures.insert(case.name.clone());
                if !is_known {
                    new_failures.push(case.name.clone());
                }
                if !is_known || !filters.is_empty() {
                    print_failure(case, is_known, &details);
                }
            }
        }
    }

    println!(
        "toml-test: {} cases, {} passed, {} failed ({} known)",
        selected.len(),
        passed,
        failures.len(),
        failures.len() - new_failures.len(),
    );

    if bless {
        if !filters.is_empty() {
            eprintln!("refusing to bless with filters");
            return ExitCode::FAILURE;
        }
        write_known_failures(&root.join(KNOWN_FAILURES), &failures, &known_failures);
        println!("updated {}", KNOWN_FAILURES);
        return ExitCode::SUCCESS;
    }

    if !unexpected_passes.is_empty() {
        println!(
            "\nthe following tests pass now, remove them from {} \
             (or run with DESER_TOML_BLESS=1):",
            KNOWN_FAILURES
        );
        for name in &unexpected_passes {
            println!("  {}", name);
        }
    }
    if !new_failures.is_empty() {
        println!("\nnew failures:");
        for name in &new_failures {
            println!("  {}", name);
        }
    }

    if new_failures.is_empty() && unexpected_passes.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_case(case: &Case) -> Outcome {
    let rv = deser_toml::from_slice::<Value>(&case.input);
    let json = match (&case.json, rv) {
        (None, Ok(value)) => {
            return Outcome::Fail(format!(
                "expected an error, but parsing succeeded\n  value: {:?}",
                value
            ));
        }
        (None, Err(_)) => return Outcome::Pass,
        (Some(_), Err(err)) => return Outcome::Fail(format!("unexpected error: {}", err)),
        (Some(json), Ok(value)) => (json, value),
    };
    let (json, value) = json;

    let expected = match deser_json::from_str::<Value>(json)
        .map_err(|err| err.to_string())
        .and_then(Value::from_tagged_json)
    {
        Ok(expected) => expected,
        Err(err) => return Outcome::Fail(format!("cannot load expected JSON: {}", err)),
    };
    if value != expected {
        return Outcome::Fail(format!(
            "value does not match the JSON\n  expected: {:?}\n  actual:   {:?}",
            expected, value
        ));
    }

    // the value has to survive a roundtrip through the serializer
    let toml = match deser_toml::to_string(&value) {
        Ok(toml) => toml,
        Err(err) => return Outcome::Fail(format!("serialization failed: {}", err)),
    };
    match deser_toml::from_str::<Value>(&toml) {
        Ok(roundtripped) if roundtripped == value => Outcome::Pass,
        Ok(roundtripped) => Outcome::Fail(format!(
            "value changed in roundtrip\n  serialized:\n{}\n  expected: {:?}\n  actual:   {:?}",
            indent(&toml),
            value,
            roundtripped
        )),
        Err(err) => Outcome::Fail(format!(
            "cannot parse serialized value: {}\n  serialized:\n{}",
            err,
            indent(&toml)
        )),
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|line| format!("    | {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn print_failure(case: &Case, is_known: bool, details: &str) {
    println!(
        "--- {} {}",
        case.name,
        if is_known { "KNOWN FAILURE" } else { "FAILED" },
    );
    println!("input:");
    let input = String::from_utf8_lossy(&case.input);
    for line in input.split_inclusive('\n') {
        // make whitespace visible
        let line = line.replace('\t', "→").replace(' ', "·").replace('\r', "␍");
        print!("  | {}", line.replace('\n', "↵\n"));
    }
    if !input.ends_with('\n') {
        println!("∎");
    }
    println!("{}\n", details);
}

fn load_cases(suite: &Path) -> Vec<Case> {
    let list = fs::read_to_string(suite.join("files-toml-1.1.0")).unwrap();
    let mut rv = Vec::new();
    for file in list.lines() {
        let Some(name) = file.strip_suffix(".toml") else {
            continue;
        };
        let json = if name.starts_with("valid/") {
            Some(fs::read_to_string(suite.join(format!("{}.json", name))).unwrap())
        } else {
            None
        };
        rv.push(Case {
            name: name.to_string(),
            input: fs::read(suite.join(file)).unwrap(),
            json,
        });
    }
    rv
}

/// Loads the known failures: one name per line, optionally followed by a
/// comment.  Returns the names with their comments.
fn load_known_failures(path: &Path) -> BTreeMap<String, String> {
    let contents = fs::read_to_string(path).unwrap_or_default();
    contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| match line.split_once(char::is_whitespace) {
            Some((name, comment)) => (name.to_string(), comment.trim().to_string()),
            None => (line.to_string(), String::new()),
        })
        .collect()
}

fn write_known_failures(path: &Path, failures: &BTreeSet<String>, old: &BTreeMap<String, String>) {
    let mut out = String::from(
        "# Tests of toml-test that are expected to fail.\n\
         # Format: <name> [# reason].  Regenerate with DESER_TOML_BLESS=1.\n",
    );
    for name in failures {
        match old.get(name) {
            Some(comment) if !comment.is_empty() => {
                out.push_str(&format!("{} {}\n", name, comment))
            }
            _ => out.push_str(&format!("{}\n", name)),
        }
    }
    fs::write(path, out).unwrap();
}
