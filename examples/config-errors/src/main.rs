//! Good error messages for configuration files.
//!
//! Errors carry the location in the input (line and column) and, with the
//! `PathLayer` of `deser-path`, the path to the value they refer to.  This
//! works for all formats and also for values which are buffered: the
//! internally tagged `Backend` below has to record its fields until it sees
//! the `type` which can come last.  The recording remembers where every
//! value came from.
use deser::de::Format;
use deser::{Deserialize, Error};
use deser_path::PathLayer;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub name: String,
    pub servers: Vec<Server>,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub backend: Backend,
}

#[derive(Debug, Deserialize)]
#[deser(tag = "type", rename_all = "lowercase")]
pub enum Backend {
    Http { url: String, timeout: u32 },
    File { path: String },
}

fn load_toml(input: &str) -> Result<Config, Error> {
    deser_toml::Deserializer::from_str(input)
        .deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

fn load_yaml(input: &str) -> Result<Config, Error> {
    deser_yaml::Deserializer::from_str(input)
        .deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

const TOML: &str = r#"
name = "demo"

[[servers]]
host = "a.example.com"
port = 8080
backend = { type = "file", path = "/srv/www" }

[[servers]]
host = "b.example.com"
port = 8081

[servers.backend]
url = "https://example.com/"
timeout = 30
type = "http"
"#;

const YAML: &str = r#"
name: demo
servers:
  - host: a.example.com
    port: 8080
    backend: {type: file, path: /srv/www}
  - host: b.example.com
    port: 8081
    backend:
      url: https://example.com/
      timeout: 30
      type: http
"#;

fn main() {
    let config = load_toml(TOML).unwrap();
    println!("{:#?}\n", config);
    assert!(matches!(
        config.servers[1].backend,
        Backend::Http { timeout: 30, .. }
    ));

    // a port that does not fit into a u16
    let err = load_toml(&TOML.replace("8081", "80810")).unwrap_err();
    println!("{}", err);
    assert_eq!(err.path(), Some("servers[1].port"));
    assert_eq!((err.line(), err.column()), (Some(11), Some(8)));

    // a type error in a buffered value
    let err = load_toml(&TOML.replace("timeout = 30", "timeout = \"30s\"")).unwrap_err();
    println!("{}", err);
    assert_eq!(err.path(), Some("servers[1].backend.timeout"));
    assert_eq!((err.line(), err.column()), (Some(15), Some(11)));

    // the same in YAML
    let err = load_yaml(&YAML.replace("timeout: 30", "timeout: 30s")).unwrap_err();
    println!("{}", err);
    assert_eq!(err.path(), Some("servers[1].backend.timeout"));
    assert_eq!((err.line(), err.column()), (Some(11), Some(16)));

    // syntax errors have a location too
    let err = load_toml(&TOML.replace("port = 8080", "port = ")).unwrap_err();
    println!("{}", err);
    assert_eq!(err.line(), Some(6));
}
