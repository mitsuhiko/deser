//! Layered configuration: defaults, configuration files and overrides.
//!
//! Instead of deserializing a new value, every layer updates the existing
//! configuration (`update` instead of `deserialize`): the keys that are
//! given replace or merge into what is there, everything else is kept.
//! Nested structs and maps are merged, so a file only needs to contain what
//! it changes.
//!
//! On top of that:
//!
//! * `validate` checks values with errors that point at the value (with
//!   line, column and path) like the errors of the format,
//! * unknown keys (like typos) are collected as warnings with their location
//!   (the `UnknownFields` policy), while `deny_unknown_fields` makes them
//!   errors for a single type,
//! * overrides from the command line (`--set server.workers=16`) are read
//!   as a query string with dotted keys.  Everything in it is text, the
//!   types parse it.
use std::collections::BTreeMap;
use std::fmt;

use deser::de::{DeserializeDriver, Deserializer, IgnoredFields, UnknownFields};
use deser::{Deserialize, Error, Serialize};
use deser_path::{Path, PathLayer};
use deser_urlencoded::Nesting;

#[derive(Debug, Serialize, Deserialize)]
#[deser(validate = check_config)]
pub struct Config {
    name: String,
    server: Server,
    log: Log,
    features: BTreeMap<String, bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    host: String,
    #[deser(validate = non_zero)]
    port: u16,
    workers: usize,
    timeouts: Timeouts,
}

/// Typos in timeouts are errors, not warnings.
#[derive(Debug, Serialize, Deserialize)]
#[deser(deny_unknown_fields)]
pub struct Timeouts {
    connect_secs: u64,
    read_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Log {
    level: Level,
    file: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "lowercase")]
pub enum Level {
    Error,
    Info,
    Debug,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            name: "app".into(),
            server: Server {
                host: "127.0.0.1".into(),
                port: 8080,
                workers: 4,
                timeouts: Timeouts {
                    connect_secs: 5,
                    read_secs: 30,
                },
            },
            log: Log {
                level: Level::Info,
                file: None,
            },
            features: BTreeMap::from([("metrics".into(), true)]),
        }
    }
}

fn non_zero(port: &u16) -> Result<(), &'static str> {
    if *port == 0 {
        Err("the port must not be 0")
    } else {
        Ok(())
    }
}

fn check_config(config: &Config) -> Result<(), String> {
    let timeouts = &config.server.timeouts;
    if timeouts.read_secs < timeouts.connect_secs {
        return Err(format!(
            "the read timeout ({}s) is shorter than the connect timeout ({}s)",
            timeouts.read_secs, timeouts.connect_secs
        ));
    }
    Ok(())
}

/// Sets up a driver: errors get paths and unknown keys are collected.
fn setup(driver: &mut DeserializeDriver<'_, '_>, warnings: &IgnoredFields) {
    driver.push_layer(PathLayer::new());
    *driver.state_mut().get_mut::<UnknownFields>() = UnknownFields::Collect(warnings.clone());
}

/// Applies a TOML file to the configuration.
fn apply_file(config: &mut Config, source: &str, warnings: &IgnoredFields) -> Result<(), Error> {
    // the source is needed for the locations of the warnings
    let toml = deser_toml::DeserializerConfig::new().track_locations(true);
    deser_toml::Deserializer::from_str_with_config(source, &toml)
        .update_with(config, |driver| setup(driver, warnings))
}

/// Applies `--set` overrides to the configuration.
fn apply_overrides(config: &mut Config, overrides: &[&str]) -> Result<(), Error> {
    let query = overrides.join("&");
    let urlencoded = deser_urlencoded::DeserializerConfig::new().nesting(Nesting::Dots);
    let warnings = IgnoredFields::new();
    deser_urlencoded::Deserializer::from_str_with_config(&query, &urlencoded)
        .update_with(config, |driver| setup(driver, &warnings))?;
    // there are no files to fix, unknown overrides are errors
    match warnings.take().into_iter().next() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Formats an error or warning with its path and location.
struct Report<'a>(&'a Error);

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = self.0.attachment::<Path>() {
            write!(f, "{}: ", path)?;
        }
        write!(f, "{}", self.0.message())?;
        if let (Some(line), Some(column)) = (self.0.line(), self.0.column()) {
            write!(f, " (line {}, column {})", line, column)?;
        }
        Ok(())
    }
}

const SYSTEM: &str = r#"
name = "shop"

[server]
host = "0.0.0.0"
timeouts = { connect_secs = 2 }

[features]
search = true
"#;

const USER: &str = r#"
[server]
port = 9000
workres = 8

[log]
level = "debug"
file = "/var/log/shop.log"
"#;

fn main() {
    let mut config = Config::default();
    let warnings = IgnoredFields::new();

    apply_file(&mut config, SYSTEM, &warnings).unwrap();
    apply_file(&mut config, USER, &warnings).unwrap();
    apply_overrides(&mut config, &["server.workers=16", "features.metrics=off"]).unwrap();

    // the files only changed what they mention
    assert_eq!(config.name, "shop");
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 9000);
    assert_eq!(config.server.workers, 16);
    assert_eq!(config.server.timeouts.connect_secs, 2);
    assert_eq!(config.server.timeouts.read_secs, 30);
    assert_eq!(config.log.level, Level::Debug);
    // maps are merged as well
    assert_eq!(
        config.features,
        BTreeMap::from([("metrics".into(), false), ("search".into(), true)])
    );
    println!("{}\n", deser_toml::to_string(&config).unwrap());

    // the typo in the user file is a warning which points at it
    let warnings = warnings.take();
    for warning in &warnings {
        println!("warning: {}", Report(warning));
    }
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].attachment::<Path>().unwrap().to_string(),
        "server.workres"
    );
    assert_eq!(warnings[0].line(), Some(4));

    // validation errors point at the value
    let err = apply_file(
        &mut Config::default(),
        "[server]\nport = 0\n",
        &IgnoredFields::new(),
    )
    .unwrap_err();
    println!("error: {}", Report(&err));
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "server.port");
    assert_eq!(err.line(), Some(2));

    // the configuration is validated once the update is complete
    let err =
        apply_overrides(&mut Config::default(), &["server.timeouts.read_secs=1"]).unwrap_err();
    println!("error: {}", Report(&err));
    assert!(err.message().contains("shorter than the connect timeout"));

    // typos in types with `deny_unknown_fields` are errors
    let err = apply_file(
        &mut Config::default(),
        "[server.timeouts]\nread_sec = 10\n",
        &IgnoredFields::new(),
    )
    .unwrap_err();
    println!("error: {}", Report(&err));
    assert_eq!(
        err.message(),
        "unknown field `read_sec`, expected `connect_secs` or `read_secs`"
    );

    // overrides are parsed by the type of the value they end up in
    let err = apply_overrides(&mut Config::default(), &["server.port=http"]).unwrap_err();
    println!("error: {}", Report(&err));
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "server.port");
    let err = apply_overrides(&mut Config::default(), &["server.hots=x"]).unwrap_err();
    println!("error: {}", Report(&err));
}
