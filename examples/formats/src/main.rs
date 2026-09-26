//! One type, many formats.
//!
//! The same derived type is written as JSON, YAML, TOML and CBOR.  Types
//! that are not part of the core data model (UUIDs, timestamps, dates,
//! durations) are passed through deser as extension
//! values.  Every format handles the ones it supports natively (TOML dates,
//! CBOR tags) and writes the fallback (typically a
//! string) for the others.  The same goes for types reading the data: the
//! `Plain` struct does not know about date-times and gets the fallbacks.
//!
//! Hints are preferences for the presentation that formats may ignore:
//! `Compact` asks for inline tables in TOML and flow style in YAML.
use std::collections::BTreeMap;
use std::time::Duration;

use deser::hints::Compact;
use deser::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    id: uuid::Uuid,
    version: String,
    published: jiff::Timestamp,
    support_until: jiff::civil::Date,
    build_time: Duration,
    #[deser(as = Compact)]
    checksums: BTreeMap<String, String>,
}

/// Reads the same data without knowing about any of the extension types.
#[derive(Debug, Deserialize)]
pub struct Plain {
    published: String,
    support_until: String,
}

fn main() {
    let release = Release {
        id: "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap(),
        version: "1.0.0".into(),
        published: "2024-06-19T15:22:45Z".parse().unwrap(),
        support_until: "2026-06-30".parse().unwrap(),
        build_time: Duration::from_secs(754),
        checksums: BTreeMap::from([
            ("md5".into(), "d41d8cd98f00b204".into()),
            ("sha1".into(), "da39a3ee5e6b4b0d".into()),
        ]),
    };

    let json = deser_json::to_string(&release).unwrap();
    println!("JSON:\n{}\n", json);
    assert_eq!(deser_json::from_str::<Release>(&json).unwrap(), release);

    let yaml = deser_yaml::to_string(&release).unwrap();
    println!("YAML:\n{}", yaml);
    assert_eq!(deser_yaml::from_str::<Release>(&yaml).unwrap(), release);

    let toml = deser_toml::to_string(&release).unwrap();
    println!("TOML:\n{}", toml);
    assert_eq!(deser_toml::from_str::<Release>(&toml).unwrap(), release);

    // the UUID (tag 37), the timestamp (tag 1) and the date (tag 1004) are
    // native in CBOR
    let cbor = deser_cbor::to_vec(&release).unwrap();
    println!("CBOR: {} bytes\n", cbor.len());
    assert_eq!(deser_cbor::from_slice::<Release>(&cbor).unwrap(), release);

    // TOML has native date-times, a type that does not know them gets
    // their fallback (a string)
    let plain: Plain = deser_toml::from_str(&toml).unwrap();
    println!("{:#?}", plain);
    assert_eq!(plain.published, "2024-06-19T15:22:45Z");
    assert_eq!(plain.support_until, "2026-06-30");
}
