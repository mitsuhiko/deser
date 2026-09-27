//! Runs the test cases of PapaParse (vendored in `tests/data`).
//!
//! PapaParse is a CSV parser for JavaScript.  Its test cases for parsing
//! (`core` and `parse`) and writing (`unparse`) are extracted into JSON by
//! `scripts/update-csv-test-data.sh`.  The configuration of a case is
//! translated into the configurations of deser-csv, cases with options
//! that have no equivalent (like dynamic typing or guessing the delimiter
//! with a callback) are skipped.  Parsed records are compared with the
//! expected data, cases where PapaParse reports errors have to fail.
//! Written output is compared without the line break after the last record
//! (which PapaParse does not write).
//!
//! PapaParse is lenient in many places where deser-csv is strict by
//! default, and it guesses the delimiter, which is why there are known
//! failures.  They are listed with the reason in
//! `papaparse_test_suite_known_failures.txt`.  The run fails if any other
//! test fails or if a listed test passes.  Set `DESER_CSV_BLESS=1` to
//! rewrite the list from the results.  Arguments that are not flags filter
//! the cases by name, then the details of known failures are shown as
//! well.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic;
use std::path::Path;
use std::process::ExitCode;

use deser_csv::{DeserializerConfig, Escape, Headers, QuoteStyle, SerializerConfig, Terminator};
use deser_value::{Kind, Value};

const KNOWN_FAILURES: &str = "tests/papaparse_test_suite_known_failures.txt";
const SUITE: &str = "tests/data/papaparse/test-cases.json";

enum Outcome {
    Pass,
    Skip,
    Fail(String),
}

fn main() -> ExitCode {
    // miri cannot read the files (and the other tests cover the unsafe code)
    if cfg!(miri) {
        println!("papaparse: skipped in miri");
        return ExitCode::SUCCESS;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| arg.to_lowercase())
        .collect();
    let list_only = std::env::args().any(|arg| arg == "--list");
    let bless = std::env::var("DESER_CSV_BLESS").is_ok_and(|x| x == "1");

    let cases = load_cases(&root.join(SUITE));
    let known_failures = load_known_failures(&root.join(KNOWN_FAILURES));
    let selected: Vec<&Value> = cases
        .iter()
        .filter(|case| {
            let name = name(case).to_lowercase();
            filters.is_empty() || filters.iter().any(|f| name.contains(f))
        })
        .collect();

    if list_only {
        for case in &selected {
            println!("{}: test", name(case));
        }
        return ExitCode::SUCCESS;
    }

    // panics are reported as failures, silence the default hook
    panic::set_hook(Box::new(|_| {}));

    let mut passed = 0;
    let mut skipped = 0;
    let mut failures = BTreeSet::new();
    let mut new_failures = Vec::new();
    let mut unexpected_passes = Vec::new();

    for case in &selected {
        let name = name(case);
        let is_known = known_failures.contains_key(name);
        let outcome = match panic::catch_unwind(panic::AssertUnwindSafe(|| run_case(case))) {
            Ok(outcome) => outcome,
            Err(_) => Outcome::Fail("panicked".into()),
        };
        match outcome {
            Outcome::Pass => {
                passed += 1;
                if is_known {
                    unexpected_passes.push(name.to_string());
                }
            }
            Outcome::Skip => skipped += 1,
            Outcome::Fail(details) => {
                failures.insert(name.to_string());
                if !is_known {
                    new_failures.push(name.to_string());
                }
                if !is_known || !filters.is_empty() {
                    print_failure(case, is_known, &details);
                }
            }
        }
    }

    println!(
        "papaparse: {} cases, {} passed, {} skipped, {} failed ({} known)",
        selected.len(),
        passed,
        skipped,
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
             (or run with DESER_CSV_BLESS=1):",
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

fn name(case: &Value) -> &str {
    get(case, "name")
        .and_then(|name| name.kind().as_str())
        .unwrap()
}

fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value.kind() {
        Kind::Map(map) => map.get(key),
        _ => None,
    }
}

fn run_case(case: &Value) -> Outcome {
    let config = get(case, "config").unwrap();
    let input = get(case, "input").unwrap();
    let expected = get(case, "expected").unwrap();
    if name(case).starts_with("unparse:") {
        run_unparse(config, input, expected)
    } else {
        run_parse(config, input.kind().as_str().unwrap(), expected)
    }
}

/// Returns a single ASCII character.
fn single_byte(value: &Value) -> Option<u8> {
    match value.kind().as_str()?.as_bytes() {
        &[byte] if byte.is_ascii() => Some(byte),
        _ => None,
    }
}

/// Translates the configuration of a case for parsing.
fn parse_config(config: &Value) -> Option<DeserializerConfig> {
    // PapaParse does not skip blank lines and accepts records with other
    // numbers of fields (without names)
    let mut rv = DeserializerConfig::new()
        .headers(Headers::None)
        .skip_blank_lines(false)
        .flexible(true);
    let Kind::Map(map) = config.kind() else {
        return None;
    };
    let mut quote = b'"';
    for (key, value) in map.iter() {
        rv = match (key.as_str()?, value.kind()) {
            ("delimiter", _) => rv.delimiter(single_byte(value)?),
            ("quoteChar", _) => {
                quote = single_byte(value)?;
                rv.quote(Some(quote))
            }
            ("escapeChar", _) => rv,
            ("comments", Kind::Bool(false)) => rv,
            ("comments", Kind::Bool(true)) => rv.comment(Some(b'#')),
            ("comments", _) => rv.comment(Some(single_byte(value)?)),
            ("header", Kind::Bool(true)) => rv.headers(Headers::First).flexible(false),
            ("header", Kind::Bool(false)) => rv,
            ("skipEmptyLines", Kind::Bool(yes)) => rv.skip_blank_lines(*yes),
            // PapaParse also skips lines with whitespace
            ("skipEmptyLines", Kind::Str(greedy)) if greedy == "greedy" => {
                rv.skip_blank_lines(true)
            }
            ("newline", Kind::Str(newline)) if ["\n", "\r", "\r\n"].contains(&newline.as_str()) => {
                rv
            }
            ("newline", _) => rv.terminator(Terminator::Byte(single_byte(value)?)),
            ("fastMode", Kind::Bool(true)) => rv.quote(None),
            ("fastMode", Kind::Bool(false)) => rv,
            // dynamic typing, previews, skipping lines, ...
            _ => return None,
        };
    }
    // the escape character is used instead of doubled quotes
    if let Some(escape) = map.get("escapeChar") {
        let escape = single_byte(escape)?;
        if escape != quote {
            rv = rv.escape(Escape::Char(escape)).double_quote(false);
        }
    }
    Some(rv)
}

fn run_parse(config: &Value, input: &str, expected: &Value) -> Outcome {
    let Some(config) = parse_config(config) else {
        return Outcome::Skip;
    };
    let expects_error = get(expected, "errors")
        .and_then(|errors| errors.kind().as_seq())
        .is_some_and(|errors| !errors.is_empty());
    let expected = get(expected, "data").unwrap();
    match (config.from_str::<Value>(input), expects_error) {
        (Ok(value), false) if value == *expected => Outcome::Pass,
        (Ok(value), false) => Outcome::Fail(format!(
            "records do not match\n  expected: {:?}\n  actual:   {:?}",
            expected, value
        )),
        (Ok(value), true) => Outcome::Fail(format!(
            "expected an error, but parsing succeeded\n  value: {:?}",
            value
        )),
        (Err(_), true) => Outcome::Pass,
        (Err(err), false) => Outcome::Fail(format!(
            "unexpected error: {}\n  expected: {:?}",
            err, expected
        )),
    }
}

/// Translates the configuration of a case for writing.
fn unparse_config(config: &Value) -> Option<SerializerConfig> {
    // PapaParse writes CRLF by default
    let mut rv = SerializerConfig::new().terminator(Terminator::CrLf);
    let Kind::Map(map) = config.kind() else {
        return None;
    };
    let mut quote = b'"';
    for (key, value) in map.iter() {
        rv = match (key.as_str()?, value.kind()) {
            ("delimiter", _) => rv.delimiter(single_byte(value)?),
            ("quoteChar", _) => {
                quote = single_byte(value)?;
                rv.quote(Some(quote))
            }
            ("escapeChar", _) => rv,
            ("quotes", Kind::Bool(true)) => rv.quote_style(QuoteStyle::Always),
            ("quotes", Kind::Bool(false)) => rv,
            ("header", Kind::Bool(yes)) => rv.headers(*yes),
            ("newline", Kind::Str(newline)) if newline == "\r\n" => rv,
            ("newline", Kind::Str(newline)) if newline == "\n" => {
                rv.terminator(Terminator::Newline)
            }
            ("newline", _) => rv.terminator(Terminator::Byte(single_byte(value)?)),
            ("escapeFormulae", Kind::Bool(yes)) => rv.escape_formulas(*yes),
            // quotes for some columns, skipping empty lines, ...
            _ => return None,
        };
    }
    if let Some(escape) = map.get("escapeChar") {
        let escape = single_byte(escape)?;
        if escape != quote {
            rv = rv.escape(Escape::Char(escape)).double_quote(false);
        }
    }
    Some(rv)
}

fn run_unparse(config_value: &Value, input: &Value, expected: &Value) -> Outcome {
    let Some(config) = unparse_config(config_value) else {
        return Outcome::Skip;
    };
    // `{fields, data}` inputs have no equivalent
    if input.kind().as_seq().is_none() {
        return Outcome::Skip;
    }
    let expected = expected.kind().as_str().unwrap();
    let mut output = match config.to_string(input) {
        Ok(output) => output,
        Err(err) => return Outcome::Fail(format!("unexpected error: {}", err)),
    };
    // PapaParse does not end the last record
    let terminator = match get(config_value, "newline").and_then(|newline| newline.kind().as_str())
    {
        Some(newline) => newline.to_string(),
        None => "\r\n".to_string(),
    };
    if output.ends_with(&terminator) {
        output.truncate(output.len() - terminator.len());
    }
    if output == expected {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "output does not match\n  expected: {:?}\n  actual:   {:?}",
            expected, output
        ))
    }
}

fn print_failure(case: &Value, is_known: bool, details: &str) {
    println!(
        "--- {} {}",
        name(case),
        if is_known { "KNOWN FAILURE" } else { "FAILED" },
    );
    println!("  input:    {:?}", get(case, "input").unwrap());
    println!("  config:   {:?}", get(case, "config").unwrap());
    println!("  {}\n", details);
}

fn load_cases(path: &Path) -> Vec<Value> {
    let json = fs::read_to_string(path).unwrap();
    deser_json::from_str(&json).unwrap()
}

/// Loads the known failures: one name per line, optionally followed by a
/// comment (`# reason`).  Returns the names with their comments.
fn load_known_failures(path: &Path) -> BTreeMap<String, String> {
    let contents = fs::read_to_string(path).unwrap_or_default();
    contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| match line.split_once(" # ") {
            Some((name, comment)) => (name.trim().to_string(), comment.trim().to_string()),
            None => (line.to_string(), String::new()),
        })
        .collect()
}

fn write_known_failures(path: &Path, failures: &BTreeSet<String>, old: &BTreeMap<String, String>) {
    let mut out = String::from(
        "# Test cases of PapaParse that are expected to fail.\n\
         # Format: <name> [# reason].  Regenerate with DESER_CSV_BLESS=1.\n",
    );
    for name in failures {
        match old.get(name) {
            Some(comment) if !comment.is_empty() => {
                out.push_str(&format!("{} # {}\n", name, comment))
            }
            _ => out.push_str(&format!("{}\n", name)),
        }
    }
    fs::write(path, out).unwrap();
}
