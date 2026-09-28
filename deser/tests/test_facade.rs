//! Checks that deser re-exports everything at the root of deser-core.
//!
//! deser re-exports the items of deser-core one by one (so that they are
//! documented as part of deser), new items are easily forgotten.
use std::collections::BTreeSet;
use std::path::Path;

/// Returns the names of the public items a crate root defines or re-exports.
///
/// This understands the subset of Rust the two crate roots are written in:
/// `pub mod` declarations and `pub use` statements of paths starting with
/// one of `prefixes`.  Hidden items (starting with `__`) are skipped.
fn public_names(source: &str, prefixes: &[&str]) -> BTreeSet<String> {
    let mut rv = BTreeSet::new();
    for line in source.lines() {
        if let Some(rest) = line.trim().strip_prefix("pub mod ") {
            rv.insert(rest.trim_end_matches([';', '{', ' ']).to_string());
        }
    }
    let mut rest = source;
    while let Some(start) = rest.find("pub use ") {
        let stmt = &rest[start + "pub use ".len()..];
        let end = stmt.find(';').unwrap();
        rest = &stmt[end..];
        let stmt = &stmt[..end];
        if !prefixes.iter().any(|p| stmt.starts_with(&format!("{p}::"))) {
            continue;
        }
        // the last segment of every path in the (possibly nested) group
        for path in stmt.split([',', '{', '}']) {
            let name = path.trim().rsplit("::").next().unwrap();
            if !name.is_empty() && name != "self" {
                rv.insert(name.to_string());
            }
        }
    }
    rv.retain(|name| !name.starts_with("__"));
    rv
}

#[test]
fn test_facade_reexports_core() {
    // miri cannot read the files (and there is no unsafe code under test)
    if cfg!(miri) {
        return;
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core_dir = manifest_dir.join("../deser-core/src");
    // deser-core is only next to deser in the repository
    let Ok(core) = std::fs::read_to_string(core_dir.join("lib.rs")) else {
        return;
    };
    let macros = std::fs::read_to_string(core_dir.join("macros.rs")).unwrap();
    let facade = std::fs::read_to_string(manifest_dir.join("src/lib.rs")).unwrap();

    let mut expected = public_names(&core, &["self"]);
    let mut exported = false;
    for line in macros.lines().map(str::trim) {
        if line == "#[macro_export]" {
            exported = true;
        } else if let Some(name) = line.strip_prefix("macro_rules! ") {
            if exported {
                expected.insert(name.trim_end_matches([' ', '{']).to_string());
            }
            exported = false;
        }
    }
    expected.retain(|name| !name.starts_with("__"));
    let actual = public_names(&facade, &["deser_core", "crate"]);

    let missing: Vec<_> = expected.difference(&actual).collect();
    assert!(
        missing.is_empty(),
        "deser does not re-export these items of deser-core: {missing:?}"
    );
}
