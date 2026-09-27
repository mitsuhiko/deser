//! Configuration from environment variables with `deser-env`.
//!
//! The variables with a prefix (`SHOP_`) are a map, `__` separates nested
//! keys and names are lowercased, so `SHOP_SERVER__MAX_CONNECTIONS` is
//! `server.max_connections`.  Everything in the environment is text which
//! the types parse, also when values are buffered:
//!
//! * nested structs and maps (`SHOP_FEATURES__NEW_UI=on`),
//! * lists in one variable with the `Separated` adapter
//!   (`SHOP_ALLOWED_ORIGINS=a, b`) and lists of structs with indexes
//!   (`SHOP_BACKENDS__0__HOST`),
//! * an internally tagged enum with a flattened struct, where the numbers
//!   still parse,
//! * variables that are set but empty: `None` for optional numbers and
//!   `true` for flags,
//! * errors and warnings that name the variable,
//! * writing a configuration back into variables for a child process.
//!
//! A real program reads the environment of the process with
//! `deser_env::from_env("SHOP_")`.  Setting variables of the running
//! process is `unsafe`, so the example passes them in with `from_vars`.
use std::collections::BTreeMap;

use deser::adapters::{Flag, Separated, TrimWhitespace};
use deser::de::{IgnoredFields, UnknownFields};
use deser::{Deserialize, Serialize};
use deser_env::{Deserializer, EnvVar};
use deser_path::{Path, PathLayer};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    name: String,
    server: Server,
    /// `SHOP_ALLOWED_ORIGINS=https://a.example, https://b.example`
    #[deser(as = Separated<',', TrimWhitespace>, default)]
    allowed_origins: Vec<String>,
    #[deser(default)]
    backends: Vec<Backend>,
    storage: Storage,
    #[deser(default)]
    features: BTreeMap<String, bool>,
    /// `SHOP_DEBUG=` switches it on
    #[deser(as = Flag)]
    debug: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    port: u16,
    /// `SHOP_SERVER__MAX_CONNECTIONS=` is `None`
    max_connections: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Backend {
    host: String,
    weight: u8,
}

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "kind", rename_all = "lowercase")]
pub enum Storage {
    Local {
        path: String,
    },
    S3 {
        bucket: String,
        #[deser(flatten)]
        retry: Retry,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Retry {
    attempts: u32,
    backoff_ms: u64,
}

const ENV: &[(&str, &str)] = &[
    ("SHOP_NAME", "shop"),
    ("SHOP_SERVER__PORT", "8080"),
    ("SHOP_SERVER__MAX_CONNECTIONS", ""),
    (
        "SHOP_ALLOWED_ORIGINS",
        "https://a.example, https://b.example",
    ),
    ("SHOP_BACKENDS__0__HOST", "10.0.0.1"),
    ("SHOP_BACKENDS__0__WEIGHT", "3"),
    ("SHOP_BACKENDS__1__HOST", "10.0.0.2"),
    ("SHOP_BACKENDS__1__WEIGHT", "1"),
    // the tag comes last and the numbers of the flattened struct parse
    ("SHOP_STORAGE__BUCKET", "shop-assets"),
    ("SHOP_STORAGE__ATTEMPTS", "3"),
    ("SHOP_STORAGE__BACKOFF_MS", "250"),
    ("SHOP_STORAGE__KIND", "s3"),
    ("SHOP_FEATURES__NEW_CHECKOUT", "on"),
    ("SHOP_FEATURES__RECOMMENDATIONS", "no"),
    ("SHOP_DEBUG", ""),
    // a typo
    ("SHOP_SERVER__PROT", "80"),
    // variables without the prefix are not read
    ("PATH", "/usr/bin"),
    ("HOME", "/home/shop"),
];

fn main() {
    let warnings = IgnoredFields::new();
    let config: Config = Deserializer::from_vars("SHOP_", ENV.iter().copied())
        .deserialize_with(|driver| {
            driver.push_layer(PathLayer::new());
            *driver.state_mut().get_mut::<UnknownFields>() =
                UnknownFields::Collect(warnings.clone());
        })
        .unwrap();
    println!("{:#?}\n", config);

    assert_eq!(config.server.port, 8080);
    assert_eq!(config.server.max_connections, None);
    assert_eq!(
        config.allowed_origins,
        ["https://a.example", "https://b.example"]
    );
    assert_eq!(config.backends[1].weight, 1);
    assert!(matches!(
        config.storage,
        Storage::S3 {
            retry: Retry { attempts: 3, .. },
            ..
        }
    ));
    assert!(config.features["new_checkout"]);
    assert!(config.debug);

    // the typo is a warning with the path and the name of the variable
    let warnings = warnings.take();
    for warning in &warnings {
        println!("warning: {}", warning);
    }
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].attachment::<EnvVar>().unwrap().name(),
        "SHOP_SERVER__PROT"
    );
    assert_eq!(
        warnings[0].attachment::<Path>().unwrap().to_string(),
        "server.prot"
    );

    // errors name the variable, also for values that were buffered
    let mut env = ENV.to_vec();
    env.retain(|(name, _)| *name != "SHOP_STORAGE__ATTEMPTS");
    env.push(("SHOP_STORAGE__ATTEMPTS", "three"));
    let err = deser_env::from_vars::<Config, _, _, _>("SHOP_", env).unwrap_err();
    println!("error: {}", err);
    assert_eq!(
        err.attachment::<EnvVar>().unwrap().name(),
        "SHOP_STORAGE__ATTEMPTS"
    );
    // a variable cannot have a value and nested variables
    let mut env = ENV.to_vec();
    env.push(("SHOP_ALLOWED_ORIGINS__0", "c"));
    let err = deser_env::from_vars::<Config, _, _, _>("SHOP_", env).unwrap_err();
    println!("error: {}", err);

    // the configuration can be passed to a child process
    // (`Command::new("worker").envs(vars)`)
    let vars = deser_env::to_vars("SHOP_", &config).unwrap();
    println!();
    for (name, value) in &vars {
        println!("{}={}", name, value);
    }
    let back: Config = deser_env::from_vars("SHOP_", vars).unwrap();
    assert_eq!(back.allowed_origins, config.allowed_origins);
    assert_eq!(back.backends.len(), 2);
}
