//! The YAML document of the benchmark of serde-saphyr
//! (`examples/benchmark.rs`): a list of items with long quoted strings that
//! refer to shared defaults with anchors and aliases.  The generator is the
//! same, the document is smaller (1 MiB instead of 25 MiB).
//!
//! Only the YAML input has aliases, the other formats (and the YAML that is
//! serialized) have them expanded.
use std::fmt::Write as _;

use deser::{Deserialize, Serialize};

/// The size of the generated document.
const TARGET_SIZE: usize = 1024 * 1024;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct Document {
    defaults: Defaults,
    items: Vec<Item>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Defaults {
    enabled: bool,
    roles: Vec<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Item {
    enabled: bool,
    roles: Vec<String>,
    id: u64,
    name: String,
    details: Details,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Details {
    description: String,
    notes: Vec<String>,
}

/// Generates the YAML document.
pub fn yaml() -> String {
    let mut yaml = String::with_capacity(TARGET_SIZE + 4096);
    yaml.push_str("---\n");
    yaml.push_str("defaults:\n");
    yaml.push_str("  enabled: &defaults_enabled true\n");
    yaml.push_str("  roles: &defaults_roles\n");
    yaml.push_str("    - reader\n");
    yaml.push_str("    - writer\n");
    yaml.push_str("items:\n");

    let mut index = 0usize;
    while yaml.len() < TARGET_SIZE {
        write!(
            yaml,
            "  - enabled: *defaults_enabled\n    roles: *defaults_roles\n    id: {index}\n    name: item_{index:05}\n    details:\n      description: \"Item number {index:05} includes repeated notes for benchmarking performance.\"\n      notes:\n"
        )
        .unwrap();
        for note_index in 0..20 {
            writeln!(
                yaml,
                "        - \"Note {note_index:02} for item {index:05}. This is repeated content to enlarge the YAML payload size considerably.\""
            )
            .unwrap();
        }
        index += 1;
    }

    yaml
}
