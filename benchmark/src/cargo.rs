//! Cargo manifests (from the benchmarks of toml-rs) and lock files.
//!
//! The manifest types are the ones of the toml-rs benchmarks.  The manifest
//! of cargo is a typical manifest, the one of web-sys is dominated by a
//! table of about 1,600 features.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

type Map<V> = BTreeMap<String, V>;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    package: Package,
    lib: Option<Lib>,
    #[deser(default)]
    #[serde(default)]
    bin: Vec<Bin>,
    #[deser(default)]
    #[serde(default)]
    features: Map<Vec<String>>,
    #[deser(default)]
    #[serde(default)]
    dependencies: Map<Dependency>,
    #[deser(default)]
    #[serde(default)]
    build_dependencies: Map<Dependency>,
    #[deser(default)]
    #[serde(default)]
    dev_dependencies: Map<Dependency>,
    #[deser(default)]
    #[serde(default)]
    target: Map<Target>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
struct Package {
    name: String,
    version: String,
    edition: Option<String>,
    #[deser(default)]
    #[serde(default)]
    authors: Vec<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    documentation: Option<String>,
    readme: Option<String>,
    description: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
struct Lib {
    name: Option<String>,
    path: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
struct Bin {
    name: String,
    #[deser(default)]
    #[serde(default)]
    test: bool,
    #[deser(default)]
    #[serde(default)]
    doc: bool,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged)]
#[serde(untagged)]
enum Dependency {
    Version(String),
    Full(DependencyFull),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
struct DependencyFull {
    version: Option<String>,
    path: Option<String>,
    #[deser(default)]
    #[serde(default)]
    default_features: bool,
    #[deser(default)]
    #[serde(default)]
    optional: bool,
    #[deser(default)]
    #[serde(default)]
    features: Vec<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
struct Target {
    #[deser(default)]
    #[serde(default)]
    dependencies: Map<Dependency>,
    #[deser(default)]
    #[serde(default)]
    build_dependencies: Map<Dependency>,
    #[deser(default)]
    #[serde(default)]
    dev_dependencies: Map<Dependency>,
}

/// A `Cargo.lock`: a long array of tables.
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct Lockfile {
    version: u32,
    package: Vec<LockedPackage>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    #[deser(default)]
    #[serde(default)]
    dependencies: Vec<String>,
}
