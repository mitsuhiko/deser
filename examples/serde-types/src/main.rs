//! Using types that only implement serde's traits with `deser-serde`.
//!
//! Most crates implement serde but not deser.  The `Serde` adapter uses the
//! serde implementation of a type in any deser format, so such types can be
//! used right away:
//!
//! * `semver::Version` and `semver::VersionReq` (which parse strings),
//! * a struct that derives serde's traits (standing in for a type of
//!   another crate),
//! * `serde_json::Value` for free-form data.
//!
//! The adapter composes with containers (`Vec<Serde>`, `Option<Serde>`,
//! `BTreeMap<_, Serde>`) and errors keep their location and path.
use std::collections::BTreeMap;

use deser::adapters::As;
use deser::{Deserialize, Serialize};
use deser_path::{Path, PathLayer};
use deser_serde::Serde;
use semver::{Version, VersionReq};

/// A type from another crate which only knows serde.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Person {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
}

/// A package manifest.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    name: String,
    #[deser(as = Serde)]
    version: Version,
    #[deser(as = Vec<Serde>)]
    authors: Vec<Person>,
    /// the values use serde, the keys are regular strings
    #[deser(as = BTreeMap<_, Serde>, default)]
    dependencies: BTreeMap<String, VersionReq>,
    /// missing and `None` like an `Option` without adapter
    #[deser(as = Option<Serde>)]
    metadata: Option<serde_json::Value>,
}

const TOML: &str = r#"
name = "shop"
version = "1.4.0-beta.2"
authors = [{ name = "Jane", email = "jane@example.com" }, { name = "John" }]

[dependencies]
deser = "0.8"
semver = ">=1.0.20, <2"

[metadata]
docs = { features = ["all"] }
"#;

fn load(input: &str) -> Result<Manifest, deser::Error> {
    deser_toml::Deserializer::from_str(input)
        .deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

fn main() {
    let manifest = load(TOML).unwrap();
    println!("{} {}", manifest.name, manifest.version);
    for (name, req) in &manifest.dependencies {
        println!("  depends on {} {}", name, req);
    }
    println!();
    assert_eq!(manifest.version.pre.as_str(), "beta.2");
    assert!(manifest.dependencies["semver"].matches(&Version::new(1, 0, 23)));
    assert_eq!(
        manifest.authors[1],
        Person {
            name: "John".into(),
            email: None
        }
    );
    assert_eq!(
        manifest.metadata,
        Some(serde_json::json!({"docs": {"features": ["all"]}}))
    );

    // the same value in other formats
    let pretty = deser_json::SerializerConfig::builder()
        .pretty(deser_json::Indent::Spaces(2))
        .build();
    println!("{}", pretty.to_string(&manifest).unwrap());
    println!("{}", deser_yaml::to_string(&manifest).unwrap());

    // errors of serde implementations point at the value
    let err = load(&TOML.replace("1.4.0-beta.2", "1.4")).unwrap_err();
    println!("error: {}", err);
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "version");
    assert_eq!(err.line(), Some(3));
    let err = load(&TOML.replace("{ name = \"John\" }", "{ nmae = \"John\" }")).unwrap_err();
    println!("error: {}", err);
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "authors[1]");

    // outside of the derive values are wrapped in `As`
    let version: As<Version, Serde> = deser_json::from_str(r#""2.0.0-rc.1""#).unwrap();
    assert!(version.into_inner() > Version::new(1, 9, 0));
}
