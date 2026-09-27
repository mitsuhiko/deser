//! Renaming keys without breaking old files or other programs.
//!
//! This is the configuration file of a deployment tool which changed its
//! format: the keys went from snake_case to kebab-case, `listen` became
//! `bind` and the health check went from `kind` and `data` to `type` and
//! `config`.  Old files have to keep working and other programs, which are
//! not upgraded yet, read the files too.
//!
//! * `alias_all = "snake_case"` accepts the old names of all fields,
//! * `tag_alias` and `content_alias` accept the old keys of an adjacently
//!   tagged enum,
//! * `rename_all` and `rename` with separate names for serialization and
//!   deserialization roll out a rename in two phases: first all readers
//!   learn the new names while the old ones are still written, then the
//!   writers switch to the new names,
//! * `skip_deserializing` writes a key for people reading the file that is
//!   never read back.
use deser::{Deserialize, Serialize};

/// The tool that writes the files.
const TOOL: &str = "deployctl 2.0";

/// Phase 1: reads the new names and the old ones, but still writes the old
/// ones so that programs which only know them keep working.
mod phase1 {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    #[deser(
        rename_all(serialize = "snake_case", deserialize = "kebab-case"),
        alias_all = "snake_case"
    )]
    pub struct Deployment {
        pub service_name: String,
        pub max_connections: u32,
        /// Written as `listen`, read as `bind` or `listen`.
        #[deser(rename(serialize = "listen", deserialize = "bind"), alias = "listen")]
        pub address: String,
        /// Written for people reading the file, always the running tool when
        /// read.
        #[deser(skip_deserializing, default = TOOL.to_string())]
        pub generated_by: String,
        pub health_check: HealthCheck,
    }

    /// Written with the old keys (`kind` and `data`), the new keys are
    /// aliases.
    #[derive(Debug, Serialize, Deserialize)]
    #[deser(
        tag = "kind",
        content = "data",
        tag_alias = "type",
        content_alias = "config",
        rename_all = "lowercase"
    )]
    pub enum HealthCheck {
        Http { path: String, port: u16 },
        Tcp { port: u16 },
        Command(Vec<String>),
    }
}

/// Phase 2: all readers know the new names, so they are written as well.
/// The old names are still read, for files written before.
mod phase2 {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    #[deser(rename_all = "kebab-case", alias_all = "snake_case")]
    pub struct Deployment {
        pub service_name: String,
        pub max_connections: u32,
        #[deser(rename = "bind", alias = "listen")]
        pub address: String,
        #[deser(skip_deserializing, default = TOOL.to_string())]
        pub generated_by: String,
        pub health_check: HealthCheck,
    }

    /// Now the old keys are the aliases.
    #[derive(Debug, Serialize, Deserialize)]
    #[deser(
        tag = "type",
        content = "config",
        tag_alias = "kind",
        content_alias = "data",
        rename_all = "lowercase"
    )]
    pub enum HealthCheck {
        Http { path: String, port: u16 },
        Tcp { port: u16 },
        Command(Vec<String>),
    }
}

/// A file from before the change.
const OLD_FILE: &str = r#"
service_name = "api"
max_connections = 100
listen = "0.0.0.0:8080"
generated_by = "deployctl 1.4"

[health_check]
kind = "http"
data = { path = "/healthz", port = 8080 }
"#;

/// A file in the new format.
const NEW_FILE: &str = r#"
service-name = "worker"
max-connections = 10
bind = "127.0.0.1:9000"

[health-check]
type = "command"
config = ["worker", "--ping"]
"#;

fn main() {
    // phase 1 reads both files
    let old: phase1::Deployment = deser_toml::from_str(OLD_FILE).unwrap();
    let new: phase1::Deployment = deser_toml::from_str(NEW_FILE).unwrap();
    println!("{:#?}", old);
    println!("{:#?}", new);
    assert_eq!(old.address, "0.0.0.0:8080");
    assert_eq!(new.address, "127.0.0.1:9000");
    // the key in the file is not read, it's the tool that runs
    assert_eq!(old.generated_by, TOOL);

    // and writes the old names, which programs that were not upgraded read
    let written = deser_toml::to_string(&new).unwrap();
    println!("phase 1 writes:\n{}", written);
    assert!(written.contains("listen = "));
    assert!(written.contains("kind = \"command\""));

    // phase 2 reads the same files (and what phase 1 wrote)
    let old: phase2::Deployment = deser_toml::from_str(OLD_FILE).unwrap();
    let again: phase2::Deployment = deser_toml::from_str(&written).unwrap();
    assert_eq!(old.service_name, "api");
    assert_eq!(again.service_name, "worker");

    // and writes the new names
    let written = deser_toml::to_string(&old).unwrap();
    println!("phase 2 writes:\n{}", written);
    assert!(written.contains("bind = "));
    assert!(written.contains("type = \"http\""));

    // a name and its alias are the same key: giving both is an error, both
    // for fields and for the tag
    let err = deser_toml::from_str::<phase2::Deployment>(
        r#"
        service-name = "api"
        max-connections = 1
        bind = "0.0.0.0:8080"
        listen = "0.0.0.0:8081"
        health-check = { type = "tcp", config = { port = 8080 } }
        "#,
    )
    .unwrap_err();
    println!("error: {}", err);
    let err = deser_toml::from_str::<phase2::HealthCheck>(
        r#"
        type = "tcp"
        kind = "http"
        config = { port = 8080 }
        "#,
    )
    .unwrap_err();
    println!("error: {}", err);
}
