//! Runs the official YAML test suite (vendored in `tests/data`).
//!
//! Every test case parses `in.yaml` and compares the produced events with
//! `test.event`.  If the case has an `in.json` file, the documents are also
//! deserialized and compared with it (ignoring tags).  Cases with an `error` file must fail to parse.  The
//! events before the error are not compared: parsers detect errors at
//! different points and the suite's partial event streams are not reliable
//! (some are placeholders).
//!
//! This uses a custom harness so that every case is reported individually
//! without pulling in dependencies.  Arguments that are not flags filter
//! the cases by id or description (case insensitive).  If filters are
//! given, the details of known failures are shown as well.
//!
//! Tests that are expected to fail are listed in
//! `yaml_test_suite_known_failures.txt`.  The run fails if any other test
//! fails or if a listed test passes.  Set `DESER_YAML_BLESS=1` to rewrite
//! the list from the results.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic;
use std::path::Path;
use std::process::ExitCode;

use deser_yaml::__private::parse_to_test_events;
use deser_yaml::Deserializer;

mod common;

use common::{Value, parse_json_stream};

const KNOWN_FAILURES: &str = "tests/yaml_test_suite_known_failures.txt";
const SUITE: &str = "tests/data/yaml-test-suite";

struct Case {
    /// The test id, `2G84/00` for sub cases.
    id: String,
    description: String,
    tags: Vec<String>,
    input: String,
    events: String,
    json: Option<String>,
    is_error: bool,
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
    let bless = std::env::var("DESER_YAML_BLESS").is_ok_and(|x| x == "1");

    let cases = load_cases(&root.join(SUITE));
    let known_failures = load_known_failures(&root.join(KNOWN_FAILURES));
    let selected: Vec<&Case> = cases
        .iter()
        .filter(|case| {
            filters.is_empty()
                || filters.iter().any(|f| {
                    case.id.to_lowercase().contains(f)
                        || case.description.to_lowercase().contains(f)
                })
        })
        .collect();

    if list_only {
        for case in &selected {
            println!("{}: test", case.id);
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
        let is_known = known_failures.contains_key(&case.id);
        match run_case(case) {
            Outcome::Pass => {
                passed += 1;
                if is_known {
                    unexpected_passes.push(case.id.clone());
                }
            }
            Outcome::Fail(details) => {
                failures.insert(case.id.clone());
                if !is_known {
                    new_failures.push(case.id.clone());
                }
                if !is_known || !filters.is_empty() {
                    print_failure(case, is_known, &details);
                }
            }
        }
    }

    println!(
        "yaml-test-suite: {} cases, {} passed, {} failed ({} known)",
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
             (or run with DESER_YAML_BLESS=1):",
            KNOWN_FAILURES
        );
        for id in &unexpected_passes {
            println!("  {}", id);
        }
    }
    if !new_failures.is_empty() {
        println!("\nnew failures:");
        for id in &new_failures {
            println!("  {}", id);
        }
    }

    if new_failures.is_empty() && unexpected_passes.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_case(case: &Case) -> Outcome {
    match check_events(case) {
        Outcome::Pass => {}
        fail => return fail,
    }
    match case.json {
        Some(ref json) if !case.is_error => check_json(case, json),
        _ => Outcome::Pass,
    }
}

/// Deserializes all documents and compares them with the JSON.
fn check_json(case: &Case, json: &str) -> Outcome {
    let rv = panic::catch_unwind(|| {
        Deserializer::from_str(&case.input)
            .iter::<Value>()
            .collect::<Result<Vec<_>, _>>()
    });
    let docs = match rv {
        Ok(Ok(docs)) => docs,
        Ok(Err(err)) => return Outcome::Fail(format!("deserialization failed: {}", err)),
        Err(_) => return Outcome::Fail("deserializer panicked".into()),
    };
    let docs: Vec<Value> = docs.into_iter().map(Value::untagged).collect();
    let expected = parse_json_stream(json);
    if docs.len() == expected.len() && docs.iter().zip(&expected).all(|(a, b)| json_eq(a, b)) {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "deserialized value does not match in.json\n  expected: {:?}\n  actual:   {:?}",
            expected, docs
        ))
    }
}

/// Compares a deserialized value with a value from JSON.
///
/// JSON cannot represent everything YAML can: object keys are unordered,
/// integers and floats are not distinguished (`450.00` is written as
/// `450`) and binary data is represented as base64 string.
fn json_eq(yaml: &Value, json: &Value) -> bool {
    match (yaml, json) {
        (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => *a as f64 == *b,
        (Value::Bytes(a), Value::Str(b)) => {
            let b: String = b.chars().filter(|c| !c.is_whitespace()).collect();
            base64(a) == b
        }
        (Value::Seq(a), Value::Seq(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_eq(a, b))
        }
        (Value::Map(a), Value::Map(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.iter().any(|(k2, v2)| json_eq(k, k2) && json_eq(v, v2)))
        }
        (a, b) => a == b,
    }
}

fn base64(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rv = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |acc, &b| acc << 8 | b as u32) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                rv.push(CHARS[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                rv.push('=');
            }
        }
    }
    rv
}

fn check_events(case: &Case) -> Outcome {
    let (events, error) = match panic::catch_unwind(|| parse_to_test_events(&case.input)) {
        Ok(rv) => rv,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|x| x.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            return Outcome::Fail(format!("parser panicked: {}", msg));
        }
    };

    match (case.is_error, error) {
        (false, None) if events == case.events => Outcome::Pass,
        (false, None) => Outcome::Fail(diff(&case.events, &events)),
        (false, Some(err)) => Outcome::Fail(format!(
            "unexpected error: {}\n{}",
            err,
            diff(&case.events, &events)
        )),
        (true, None) => Outcome::Fail(format!(
            "expected an error, but parsing succeeded\n{}",
            diff(&case.events, &events)
        )),
        (true, Some(_)) => Outcome::Pass,
    }
}

fn print_failure(case: &Case, is_known: bool, details: &str) {
    println!(
        "--- {} {}({}) [{}]",
        case.id,
        if is_known {
            "KNOWN FAILURE "
        } else {
            "FAILED "
        },
        case.description,
        case.tags.join(", ")
    );
    println!("input:");
    for line in case.input.split_inclusive('\n') {
        // make whitespace visible
        let line = line.replace('\t', "→").replace(' ', "·");
        print!("  | {}", line.replace('\n', "↵\n"));
    }
    if !case.input.ends_with('\n') {
        println!("∎");
    }
    println!("{}\n", details);
}

/// Renders expected and actual events side by side.
fn diff(expected: &str, actual: &str) -> String {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let width = expected
        .iter()
        .map(|x| x.chars().count())
        .max()
        .unwrap_or(0)
        .max(8);
    let mut out = format!("  {:width$}   actual\n", "expected", width = width);
    for idx in 0..expected.len().max(actual.len()) {
        let left = expected.get(idx).copied().unwrap_or("");
        let right = actual.get(idx).copied().unwrap_or("");
        let marker = if expected.get(idx) == actual.get(idx) {
            ' '
        } else {
            '!'
        };
        out.push_str(&format!(
            "{} {:width$} | {}\n",
            marker,
            left,
            right,
            width = width
        ));
    }
    out.pop();
    out
}

fn load_cases(suite: &Path) -> Vec<Case> {
    let mut tags = BTreeMap::new();
    for line in fs::read_to_string(suite.join("tags.txt")).unwrap().lines() {
        let mut parts = line.split_whitespace();
        if let Some(id) = parts.next() {
            tags.insert(id.to_string(), parts.map(|x| x.to_string()).collect());
        }
    }

    let mut dirs = Vec::new();
    for entry in fs::read_dir(suite.join("cases")).unwrap() {
        let path = entry.unwrap().path();
        if path.join("in.yaml").is_file() {
            dirs.push(path);
        } else {
            for sub in fs::read_dir(&path).unwrap() {
                dirs.push(sub.unwrap().path());
            }
        }
    }
    dirs.sort();

    let cases_dir = suite.join("cases");
    dirs.into_iter()
        .map(|dir| {
            let id = rel_id(&cases_dir, &dir);
            let top_id = id.split('/').next().unwrap();
            Case {
                description: read(&dir.join("===")).trim().to_string(),
                tags: tags.get(top_id).cloned().unwrap_or_default(),
                input: read(&dir.join("in.yaml")),
                events: read(&dir.join("test.event")),
                json: fs::read_to_string(dir.join("in.json")).ok(),
                is_error: dir.join("error").is_file(),
                id,
            }
        })
        .collect()
}

fn rel_id(base: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(base).unwrap();
    rel.components()
        .map(|x| x.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {}", path.display(), err))
}

/// Loads the known failures: one id per line, optionally followed by a
/// comment.  Returns the ids with their comments.
fn load_known_failures(path: &Path) -> BTreeMap<String, String> {
    let contents = fs::read_to_string(path).unwrap_or_default();
    contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| match line.split_once(char::is_whitespace) {
            Some((id, comment)) => (id.to_string(), comment.trim().to_string()),
            None => (line.to_string(), String::new()),
        })
        .collect()
}

fn write_known_failures(path: &Path, failures: &BTreeSet<String>, old: &BTreeMap<String, String>) {
    let mut out = String::from(
        "# Tests of the YAML test suite that are expected to fail.\n\
         # Format: <id> [# reason].  Regenerate with DESER_YAML_BLESS=1.\n",
    );
    for id in failures {
        match old.get(id) {
            Some(comment) if !comment.is_empty() => out.push_str(&format!("{} {}\n", id, comment)),
            _ => out.push_str(&format!("{}\n", id)),
        }
    }
    fs::write(path, out).unwrap();
}
