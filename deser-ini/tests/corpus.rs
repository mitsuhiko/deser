//! Runs the INI test corpus (vendored in `tests/data`, see its README).
//!
//! INI has no shared test suite, the corpus has the inputs of the test
//! suites of other INI parsers and real world files with the results of
//! reference implementations.  The cases are:
//!
//! * `configparser/<input>`: [`DeserializerConfig::python`] has to read the
//!   input like Python's `configparser` (the `lenient` variant of the
//!   references).  A value that starts on the line after its key has no
//!   line break in front (`configparser` keeps one).
//! * `inih/<input>`: the plain INI files of inih (`no_multiline` variant of
//!   the references), read without continuation lines, quotes and keys
//!   without values.
//! * `git/<input>`: [`DeserializerConfig::git`] has to read the git config
//!   inputs like git.
//! * `default/<input>`: the default configuration has to read the real
//!   world files.
//!
//! Inputs that are not UTF-8 have to fail.  Every value that is read is
//! also serialized with the matching [`SerializerConfig`] and has to read
//! back as the same value (without keys with null values, which are not
//! written).
//!
//! This uses a custom harness so that every case is reported individually.
//! Arguments that are not flags filter the cases by name.  Cases that are
//! expected to fail are listed in `corpus_known_failures.txt`, the run fails
//! if any other case fails or if a listed case passes.  Set
//! `DESER_INI_BLESS=1` to rewrite the list from the results.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use deser_ini::{Continuation, DeserializerConfig, Quotes, SerializerConfig};
use deser_value::Value;

const KNOWN_FAILURES: &str = "tests/corpus_known_failures.txt";
const DATA: &str = "tests/data";
const GIT_SUITES: &[&str] = &[
    "gitoxide",
    "gcfg",
    "go-git",
    "dulwich",
    "jgit",
    "isomorphic-git",
];

/// The key of an entry: the section, the subsection and the key.
type Key = (Option<String>, Option<String>, String);

/// A read file: the values of the keys (`None` for keys without value) and
/// the sections (and subsections).
#[derive(Debug, Default, PartialEq)]
struct Flat {
    entries: BTreeMap<Key, Vec<Option<String>>>,
    tables: BTreeSet<(String, Option<String>)>,
}

impl Flat {
    /// Keeps only the last value of every key.
    fn last_values(mut self) -> Flat {
        for values in self.entries.values_mut() {
            let last = values.pop().unwrap();
            *values = vec![last];
        }
        self
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Configparser,
    Inih,
    Git,
    Default,
}

struct Case {
    name: String,
    kind: Kind,
    input: PathBuf,
    /// The expected result (`None` if the input has to be read without
    /// error).
    expected: Option<Value>,
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
    let bless = std::env::var("DESER_INI_BLESS").is_ok_and(|x| x == "1");

    let cases = load_cases(&root.join(DATA));
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
        let outcome = match panic::catch_unwind(panic::AssertUnwindSafe(|| run_case(case))) {
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
        "ini corpus: {} cases, {} passed, {} failed ({} known)",
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
            "\nthe following cases pass now, remove them from {} \
             (or run with DESER_INI_BLESS=1):",
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

fn configs(kind: Kind) -> (DeserializerConfig, SerializerConfig) {
    match kind {
        Kind::Configparser => (DeserializerConfig::python(), SerializerConfig::python()),
        Kind::Inih => (
            DeserializerConfig::builder()
                .continuation(Continuation::None)
                .quotes(Quotes::None)
                .allow_no_value(false)
                .build(),
            SerializerConfig::builder()
                .continuation(Continuation::None)
                .quotes(Quotes::None)
                .build(),
        ),
        Kind::Git => (DeserializerConfig::git(), SerializerConfig::git()),
        Kind::Default => (DeserializerConfig::new(), SerializerConfig::new()),
    }
}

fn run_case(case: &Case) -> Outcome {
    let input = fs::read(&case.input).unwrap();
    let (de_config, ser_config) = configs(case.kind);
    let rv = de_config.from_slice::<Value>(&input);
    if std::str::from_utf8(&input).is_err() {
        return match rv {
            Ok(value) => Outcome::Fail(format!("input is not UTF-8 but was read: {:?}", value)),
            Err(_) => Outcome::Pass,
        };
    }
    let expected = match case
        .expected
        .as_ref()
        .map(|expected| expected_flat(case.kind, expected))
    {
        Some(Ok(expected)) => expected,
        Some(Err(err)) => {
            return match rv {
                Ok(value) => {
                    Outcome::Fail(format!("expected {}, but read {:?}", err, flatten(&value)))
                }
                Err(_) => Outcome::Pass,
            };
        }
        None => None,
    };
    let value = match rv {
        Ok(value) => value,
        Err(err) => return Outcome::Fail(format!("unexpected error: {}", err)),
    };
    let flat = match flatten(&value) {
        Ok(flat) => flat,
        Err(err) => return Outcome::Fail(err),
    };
    if let Some(expected) = expected {
        let actual = match case.kind {
            Kind::Git => Flat {
                entries: flat.entries.clone(),
                tables: BTreeSet::new(),
            },
            _ => Flat {
                entries: flat.entries.clone(),
                tables: flat.tables.clone(),
            }
            .last_values(),
        };
        if actual != expected {
            return Outcome::Fail(diff(&expected, &actual));
        }
    }

    // the value has to survive a roundtrip through the serializer
    let written = match ser_config.to_string(&value) {
        Ok(written) => written,
        Err(err) => return Outcome::Fail(format!("serialization failed: {}", err)),
    };
    let reread = match de_config.from_str::<Value>(&written) {
        Ok(reread) => reread,
        Err(err) => {
            return Outcome::Fail(format!(
                "cannot read serialized value: {}\n  serialized:\n{}",
                err,
                indent(&written)
            ));
        }
    };
    let without_nulls = |flat: Flat| Flat {
        entries: flat
            .entries
            .into_iter()
            .filter(|(_, values)| values != &[None])
            .collect(),
        tables: flat.tables,
    };
    match flatten(&reread) {
        Ok(reread) if without_nulls(reread.clone_flat()) == without_nulls(flat.clone_flat()) => {
            Outcome::Pass
        }
        Ok(reread) => Outcome::Fail(format!(
            "value changed in roundtrip\n  serialized:\n{}\n{}",
            indent(&written),
            diff(&without_nulls(flat), &without_nulls(reread))
        )),
        Err(err) => Outcome::Fail(err),
    }
}

impl Flat {
    fn clone_flat(&self) -> Flat {
        Flat {
            entries: self.entries.clone(),
            tables: self.tables.clone(),
        }
    }
}

/// Describes the differences of two read files.
fn diff(expected: &Flat, actual: &Flat) -> String {
    let mut out = String::new();
    for (key, values) in &expected.entries {
        match actual.entries.get(key) {
            Some(actual) if actual == values => {}
            Some(actual) => out.push_str(&format!(
                "  {:?}: expected {:?}, got {:?}\n",
                key, values, actual
            )),
            None => out.push_str(&format!("  {:?}: missing (expected {:?})\n", key, values)),
        }
    }
    for (key, values) in &actual.entries {
        if !expected.entries.contains_key(key) {
            out.push_str(&format!("  {:?}: unexpected {:?}\n", key, values));
        }
    }
    for table in expected.tables.difference(&actual.tables) {
        out.push_str(&format!("  missing section {:?}\n", table));
    }
    for table in actual.tables.difference(&expected.tables) {
        out.push_str(&format!("  unexpected section {:?}\n", table));
    }
    format!("results differ:\n{}", out)
}

/// Returns the values of a value read from an INI file.
fn values(value: &Value) -> Result<Vec<Option<String>>, String> {
    let items: Vec<&Value> = match value.as_seq() {
        Some(seq) if seq.is_repeated() => seq.iter().collect(),
        _ => vec![value],
    };
    items
        .into_iter()
        .map(|item| {
            if item.is_null() {
                Ok(None)
            } else {
                item.as_str()
                    .map(|text| Some(text.to_string()))
                    .ok_or_else(|| format!("unexpected value {:?}", item))
            }
        })
        .collect()
}

/// Flattens a value read from an INI file.
fn flatten(value: &Value) -> Result<Flat, String> {
    let mut flat = Flat::default();
    let root = value.as_map().ok_or("the file is not a map")?;
    for (key, value) in root.iter() {
        let key = key.as_str().ok_or("key is not a string")?.to_string();
        let Some(section) = value.as_map() else {
            flat.entries.insert((None, None, key), values(value)?);
            continue;
        };
        flat.tables.insert((key.clone(), None));
        for (name, value) in section.iter() {
            let name = name.as_str().ok_or("key is not a string")?.to_string();
            let Some(subsection) = value.as_map() else {
                flat.entries
                    .insert((Some(key.clone()), None, name), values(value)?);
                continue;
            };
            flat.tables.insert((key.clone(), Some(name.clone())));
            for (sub_key, value) in subsection.iter() {
                let sub_key = sub_key.as_str().ok_or("key is not a string")?.to_string();
                flat.entries.insert(
                    (Some(key.clone()), Some(name.clone()), sub_key),
                    values(value)?,
                );
            }
        }
    }
    Ok(flat)
}

/// Returns the text of a string of the references (`None` for null).
fn reference_text(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .map(|text| Some(text.to_string()))
        .ok_or_else(|| "text that is not UTF-8".to_string())
}

/// Resolves a variant of a reference (`same_as`).
fn variant<'v>(reference: &'v Value, name: &str) -> &'v Value {
    let value = reference.get(name).expect("missing variant");
    match value.get("same_as").and_then(|v| v.as_str()) {
        Some(other) => reference.get(other).expect("missing variant"),
        None => value,
    }
}

/// Converts the expected result of a case, `Ok(None)` if an error is
/// expected (returned as `Err`).
fn expected_flat(kind: Kind, expected: &Value) -> Result<Option<Flat>, String> {
    let mut flat = Flat::default();
    match kind {
        Kind::Configparser => {
            let result = variant(expected.get("configparser").unwrap(), "lenient");
            if let Some(error) = result.get("error") {
                return Err(format!("an error ({:?})", error.get("type")));
            }
            for section in result.get("sections").unwrap().as_seq().unwrap().iter() {
                if let Some(section) = reference_text(section)? {
                    flat.tables.insert((section, None));
                }
            }
            for entry in result.get("entries").unwrap().as_seq().unwrap().iter() {
                let section = reference_text(entry.get(0).unwrap())?;
                let key = reference_text(entry.get(1).unwrap())?.unwrap();
                // a value that starts on the next line has no line break
                // in front
                let value = reference_text(entry.get(2).unwrap())?
                    .map(|value| value.trim_start_matches('\n').to_string());
                flat.entries.insert((section, None, key), vec![value]);
            }
            Ok(Some(flat))
        }
        Kind::Inih => {
            let result = variant(expected.get("inih").unwrap(), "no_multiline");
            if let Some(line) = result.get("error_line").filter(|line| !line.is_null()) {
                return Err(format!("an error (line {:?})", line.as_u64()));
            }
            for event in result.get("events").unwrap().as_seq().unwrap().iter() {
                let section = reference_text(event.get(2).unwrap())?.unwrap();
                if event.get(0).and_then(|v| v.as_str()) == Some("section") {
                    flat.tables.insert((section, None));
                    continue;
                }
                let section = Some(section).filter(|section| !section.is_empty());
                let key = reference_text(event.get(3).unwrap())?.unwrap();
                let value = reference_text(event.get(4).unwrap())?;
                flat.entries.insert((section, None, key), vec![value]);
            }
            Ok(Some(flat))
        }
        Kind::Git => {
            let result = expected.get("git").unwrap();
            if let Some(error) = result.get("error") {
                return Err(format!("an error ({:?})", error.get("message")));
            }
            for entry in result.get("entries").unwrap().as_seq().unwrap().iter() {
                let section = reference_text(entry.get(0).unwrap())?;
                let subsection = reference_text(entry.get(1).unwrap())?;
                let key = reference_text(entry.get(2).unwrap())?.unwrap();
                let value = reference_text(entry.get(3).unwrap())?;
                flat.entries
                    .entry((section, subsection, key))
                    .or_default()
                    .push(value);
            }
            Ok(Some(flat))
        }
        Kind::Default => Ok(None),
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
    let input = fs::read(&case.input).unwrap();
    let input = String::from_utf8_lossy(&input);
    if input.len() < 2000 {
        println!("input:");
        for line in input.split_inclusive('\n') {
            // make whitespace visible
            let line = line.replace('\t', "→").replace(' ', "·").replace('\r', "␍");
            print!("  | {}", line.replace('\n', "↵\n"));
        }
        if !input.ends_with('\n') {
            println!("∎");
        }
    } else {
        println!("input: {}", case.input.display());
    }
    println!("{}\n", details);
}

/// Returns the files in a directory and its subdirectories.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn load_json(path: &Path) -> Value {
    deser_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn load_cases(data: &Path) -> Vec<Case> {
    let mut rv = Vec::new();
    let relative = |path: &Path| {
        path.strip_prefix(data)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };

    let mut references = Vec::new();
    walk(&data.join("references"), &mut references);
    references.sort();
    for reference in references {
        let Some(name) = relative(&reference)
            .strip_prefix("references/")
            .and_then(|name| name.strip_suffix(".json"))
            .map(str::to_string)
        else {
            continue;
        };
        let expected = load_json(&reference);
        let input = data.join(&name);
        for (kind, prefix) in [(Kind::Configparser, "configparser"), (Kind::Inih, "inih")] {
            rv.push(Case {
                name: format!("{}/{}", prefix, name),
                kind,
                input: input.clone(),
                expected: Some(expected.clone()),
            });
        }
        if name.starts_with("real-world/") {
            rv.push(Case {
                name: format!("default/{}", name),
                kind: Kind::Default,
                input,
                expected: None,
            });
        }
    }

    let mut git = Vec::new();
    for suite in GIT_SUITES {
        walk(&data.join("suites").join(suite).join("captured"), &mut git);
    }
    walk(&data.join("real-world"), &mut git);
    git.sort();
    for path in git {
        let name = relative(&path);
        let input = if let Some(input) = name.strip_suffix(".git.json") {
            input.to_string()
        } else if name.contains("/captured/") && name.ends_with(".json") {
            name.replace(".json", ".config")
        } else {
            continue;
        };
        rv.push(Case {
            name: format!("git/{}", input),
            kind: Kind::Git,
            input: data.join(&input),
            expected: Some(load_json(&path)),
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
        .map(|line| match line.split_once(" #") {
            Some((name, comment)) => (name.trim().to_string(), format!("#{}", comment)),
            None => (line.to_string(), String::new()),
        })
        .collect()
}

fn write_known_failures(path: &Path, failures: &BTreeSet<String>, old: &BTreeMap<String, String>) {
    let mut out = String::from(
        "# Cases of the INI corpus that are expected to fail.\n\
         # Format: <name> [# reason].  Regenerate with DESER_INI_BLESS=1.\n",
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
