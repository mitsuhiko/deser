use std::collections::BTreeMap;

use deser::adapters::{Flag, Separated, TrimWhitespace};
use deser::de::{Deserializer as _, DuplicateKeys, IgnoredFields, UnknownFields};
use deser::{Context, Deserialize, ErrorKind};
use deser_env::{Case, Deserializer, DeserializerConfig, EnvVar, from_vars};
use deser_path::{Path, PathLayer};

type Nested = BTreeMap<String, BTreeMap<String, String>>;

fn env_var(err: &deser::Error) -> Option<&str> {
    err.attachment::<EnvVar>().map(|var| var.name())
}

#[test]
fn test_basics() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config<'a> {
        name: &'a str,
        port: u16,
        debug: bool,
        ratio: f32,
        max_connections: Option<u32>,
    }

    let name = String::from("shop");
    let config: Config = from_vars(
        "APP_",
        [
            ("APP_NAME", name.as_str()),
            ("APP_PORT", "8080"),
            ("APP_DEBUG", "on"),
            ("APP_RATIO", "0.5"),
            ("APP_UNKNOWN", "1"),
            ("OTHER_PORT", "x"),
            ("PATH", "/bin"),
        ],
    )
    .unwrap();
    assert_eq!(
        config,
        Config {
            name: "shop",
            port: 8080,
            debug: true,
            ratio: 0.5,
            max_connections: None,
        }
    );
    // values that are given borrowed are passed on borrowed
    assert!(
        name.as_bytes()
            .as_ptr_range()
            .contains(&config.name.as_ptr())
    );

    // owned variables work too (like from `std::env::vars`)
    let vars = vec![("APP_PORT".to_string(), "1".to_string())];
    let value: BTreeMap<String, u16> = from_vars("APP_", vars).unwrap();
    assert_eq!(value["port"], 1);

    // a name that is only the prefix is skipped, without prefix everything
    // is read
    let value: BTreeMap<String, String> = from_vars("APP_", [("APP_", "x")]).unwrap();
    assert!(value.is_empty());
    let value: BTreeMap<String, String> =
        from_vars("", [("HOME", "/root"), ("PATH", "/bin")]).unwrap();
    assert_eq!(value["home"], "/root");
    assert_eq!(value["path"], "/bin");
}

#[test]
fn test_nesting() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        server: Server,
        hosts: Vec<String>,
        features: BTreeMap<String, bool>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Server {
        max_connections: u32,
        timeouts: Timeouts,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Timeouts {
        read_secs: u64,
    }

    let config: Config = from_vars(
        "APP_",
        [
            ("APP_SERVER__MAX_CONNECTIONS", "5"),
            ("APP_SERVER__TIMEOUTS__READ_SECS", "30"),
            // the order does not matter, the variables are sorted
            ("APP_HOSTS__1", "b"),
            ("APP_HOSTS__0", "a"),
            ("APP_FEATURES__NEW_UI", "yes"),
            ("APP_FEATURES__BETA", "0"),
        ],
    )
    .unwrap();
    assert_eq!(
        config,
        Config {
            server: Server {
                max_connections: 5,
                timeouts: Timeouts { read_secs: 30 },
            },
            hosts: vec!["a".into(), "b".into()],
            features: BTreeMap::from([("new_ui".into(), true), ("beta".into(), false)]),
        }
    );

    // indexes with gaps are map keys
    let value: BTreeMap<String, BTreeMap<u32, String>> =
        from_vars("", [("A__1", "x"), ("A__3", "y")]).unwrap();
    assert_eq!(
        value["a"],
        BTreeMap::from([(1, "x".into()), (3, "y".into())])
    );
    // indexes and names mixed are a map
    let value: Nested = from_vars("", [("A__0", "x"), ("A__B", "y")]).unwrap();
    assert_eq!(value["a"]["0"], "x");
    assert_eq!(value["a"]["b"], "y");
    // sequences of structs
    #[derive(Debug, Deserialize, PartialEq)]
    struct Backend {
        host: String,
        weight: u8,
    }
    let value: BTreeMap<String, Vec<Backend>> = from_vars(
        "",
        [
            ("B__0__HOST", "a"),
            ("B__0__WEIGHT", "1"),
            ("B__1__HOST", "b"),
            ("B__1__WEIGHT", "2"),
        ],
    )
    .unwrap();
    assert_eq!(value["b"][1].host, "b");
    assert_eq!(value["b"][1].weight, 2);

    // a single value is a sequence of one element
    let value: BTreeMap<String, Vec<u32>> = from_vars("", [("A", "1")]).unwrap();
    assert_eq!(value["a"], [1]);
}

#[test]
fn test_malformed_names() {
    // names with empty segments are taken as they are
    let value: BTreeMap<String, String> = from_vars(
        "",
        [
            ("__CF_USER_TEXT_ENCODING", "x"),
            ("A__", "y"),
            ("A____B", "z"),
            ("_", "w"),
        ],
    )
    .unwrap();
    assert_eq!(
        value,
        BTreeMap::from([
            ("__cf_user_text_encoding".into(), "x".into()),
            ("a__".into(), "y".into()),
            ("a____b".into(), "z".into()),
            ("_".into(), "w".into()),
        ])
    );
    // three underscores are the separator and an underscore
    let value: Nested = from_vars("", [("A___B", "x")]).unwrap();
    assert_eq!(value["a"]["_b"], "x");
}

#[test]
fn test_config() {
    // the separator can be changed
    let single = DeserializerConfig::builder().separator("_").build();
    let value: Nested = single
        .from_vars("APP_", [("APP_SERVER_PORT", "80")])
        .unwrap();
    assert_eq!(value["server"]["port"], "80");
    let flat = DeserializerConfig::builder().separator("").build();
    let value: BTreeMap<String, String> = flat.from_vars("", [("A__B", "1")]).unwrap();
    assert_eq!(value["a__b"], "1");

    // the case can be preserved
    let preserve = DeserializerConfig::builder().case(Case::Preserve).build();
    let value: Nested = preserve.from_vars("", [("Server__Port", "80")]).unwrap();
    assert_eq!(value["Server"]["Port"], "80");
    let value: Nested = from_vars("", [("Server__Port", "80")]).unwrap();
    assert_eq!(value["server"]["port"], "80");

    // the depth is limited
    let err = DeserializerConfig::builder()
        .max_depth(1)
        .build()
        .from_vars::<BTreeMap<String, Nested>, _, _, _>("", [("A__B__C", "1")])
        .unwrap_err();
    assert_eq!(err.message(), "name is nested too deeply");
    assert_eq!(env_var(&err), Some("A__B__C"));

    // bytes are base64
    let value: BTreeMap<String, Vec<u8>> = from_vars("", [("KEY", "AQID")]).unwrap();
    assert_eq!(value["key"], [1, 2, 3]);
}

#[test]
fn test_duplicate_keys() {
    #[derive(Debug, Deserialize)]
    struct Config {
        port: u16,
    }

    let vars = [("APP_PORT", "1"), ("APP_port", "2")];
    // sorted by name: `APP_PORT` comes first
    assert_eq!(from_vars::<Config, _, _, _>("APP_", vars).unwrap().port, 2);
    // the context overrides the default of the format
    let first = Context::with(DuplicateKeys::First);
    assert_eq!(
        Deserializer::from_vars("APP_", vars)
            .deserialize_in::<Config>(&first)
            .unwrap()
            .port,
        1
    );
    let strict = Context::with(DuplicateKeys::Error);
    let err = Deserializer::from_vars("APP_", vars)
        .deserialize_in::<Config>(&strict)
        .unwrap_err();
    assert_eq!(env_var(&err), Some("APP_port"));
    // with the case preserved these are different keys
    let preserve = DeserializerConfig::builder().case(Case::Preserve).build();
    let value: BTreeMap<String, u16> = preserve.from_vars("APP_", vars).unwrap();
    assert_eq!(value.len(), 2);
}

#[test]
fn test_collections() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        hosts: Vec<String>,
        ports: Vec<u16>,
        tags: Vec<String>,
    }

    // a single variable is a collection of one value, variables for the
    // same key are collected and missing collections are empty
    let config: Config = from_vars(
        "APP_",
        [("APP_HOSTS", "a"), ("APP_PORTS", "1"), ("APP_ports", "2")],
    )
    .unwrap();
    assert_eq!(
        config,
        Config {
            hosts: vec!["a".into()],
            ports: vec![1, 2],
            tags: vec![],
        }
    );
    // indexes are sequences
    let config: Config = from_vars("APP_", [("APP_HOSTS__0", "a"), ("APP_HOSTS__1", "b")]).unwrap();
    assert_eq!(config.hosts, ["a", "b"]);
}

#[test]
fn test_value_and_nested() {
    let err = from_vars::<Nested, _, _, _>("", [("DB", "x"), ("DB__POOL", "4")]).unwrap_err();
    assert_eq!(err.message(), "variable has a value and nested variables");
    assert_eq!(env_var(&err), Some("DB"));
}

#[test]
fn test_empty_values() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Options {
        #[deser(as = Flag)]
        verbose: bool,
        #[deser(as = Flag)]
        quiet: bool,
        port: Option<u16>,
        name: Option<String>,
        #[deser(as = Flag)]
        color: bool,
    }

    let options: Options = from_vars(
        "APP_",
        [
            ("APP_VERBOSE", ""),
            ("APP_PORT", ""),
            ("APP_NAME", ""),
            ("APP_COLOR", "off"),
        ],
    )
    .unwrap();
    assert_eq!(
        options,
        Options {
            verbose: true,
            quiet: false,
            port: None,
            name: Some("".into()),
            color: false,
        }
    );

    // without an option the empty value is an error
    let err = from_vars::<BTreeMap<String, u16>, _, _, _>("", [("PORT", "")]).unwrap_err();
    assert_eq!(err.message(), "invalid value \"\", expected u16");
}

#[test]
fn test_lists() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        #[deser(as = Separated)]
        hosts: Vec<String>,
        #[deser(as = Separated<',', TrimWhitespace>, default)]
        ports: Vec<u16>,
        #[deser(as = Separated<':'>, default)]
        path: Vec<String>,
    }

    let config: Config = from_vars(
        "APP_",
        [
            ("APP_HOSTS", "a,b"),
            ("APP_PORTS", "80, 443"),
            ("APP_PATH", "/usr/bin:/bin"),
        ],
    )
    .unwrap();
    assert_eq!(config.hosts, ["a", "b"]);
    assert_eq!(config.ports, [80, 443]);
    assert_eq!(config.path, ["/usr/bin", "/bin"]);

    // indexes work as well
    let config: Config =
        from_vars("APP_", [("APP_HOSTS__0", "a,b"), ("APP_HOSTS__1", "c")]).unwrap();
    assert_eq!(config.hosts, ["a,b", "c"]);

    let err = from_vars::<Config, _, _, _>("APP_", [("APP_HOSTS", ""), ("APP_PORTS", "80,x")])
        .unwrap_err();
    assert_eq!(err.message(), "invalid value \"x\", expected u16");
    assert_eq!(env_var(&err), Some("APP_PORTS"));
}

#[test]
fn test_buffered() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Paginate {
        limit: u32,
        offset: u32,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "type", rename_all = "lowercase")]
    enum Storage {
        S3 {
            bucket: String,
            #[deser(flatten)]
            paginate: Paginate,
        },
        Local {
            path: String,
        },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        storage: Storage,
    }

    let vars = [
        ("APP_STORAGE__BUCKET", "b"),
        ("APP_STORAGE__LIMIT", "10"),
        ("APP_STORAGE__OFFSET", "20"),
        ("APP_STORAGE__TYPE", "s3"),
    ];
    let config: Config = from_vars("APP_", vars).unwrap();
    assert_eq!(
        config.storage,
        Storage::S3 {
            bucket: "b".into(),
            paginate: Paginate {
                limit: 10,
                offset: 20
            },
        }
    );

    // the errors of buffered values have the name of the variable
    let err = from_vars::<Config, _, _, _>(
        "APP_",
        [
            ("APP_STORAGE__BUCKET", "b"),
            ("APP_STORAGE__LIMIT", "ten"),
            ("APP_STORAGE__OFFSET", "20"),
            ("APP_STORAGE__TYPE", "s3"),
        ],
    )
    .unwrap_err();
    assert_eq!(err.message(), "invalid value \"ten\", expected u32");
    assert_eq!(env_var(&err), Some("APP_STORAGE__LIMIT"));
}

#[test]
fn test_errors() {
    #[derive(Debug, Deserialize)]
    struct Config {
        server: Server,
    }

    #[derive(Debug, Deserialize)]
    struct Server {
        port: u16,
        host: String,
    }

    let err = from_vars::<Config, _, _, _>(
        "APP_",
        [("APP_SERVER__PORT", "http"), ("APP_SERVER__HOST", "x")],
    )
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
    assert_eq!(env_var(&err), Some("APP_SERVER__PORT"));
    assert_eq!(err.offset(), None);
    assert_eq!(
        err.to_string(),
        "InvalidValue: invalid value \"http\", expected u16 (environment variable APP_SERVER__PORT)"
    );

    // missing fields do not come from a variable
    let err = from_vars::<Config, _, _, _>("APP_", [("APP_SERVER__PORT", "80")]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
    assert_eq!(env_var(&err), None);

    // the path layer works along
    let err = Deserializer::from_vars("APP_", [("APP_SERVER__PORT", "x")])
        .deserialize_with::<Config, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "server.port");
    assert_eq!(env_var(&err), Some("APP_SERVER__PORT"));
}

#[test]
fn test_unknown_fields() {
    #[derive(Debug, Deserialize)]
    struct Config {
        #[deser(default)]
        server: Server,
    }

    #[derive(Debug, Deserialize, Default)]
    struct Server {
        #[deser(default)]
        port: u16,
    }

    let warnings = IgnoredFields::new();
    Deserializer::from_vars(
        "APP_",
        [
            ("APP_SERVER__PROT", "80"),
            ("APP_SERVR__PORT", "80"),
            ("APP_SERVR__HOST", "x"),
        ],
    )
    .deserialize_with::<Config, _>(|driver| {
        UnknownFields::Collect(warnings.clone()).set(driver.state_mut());
    })
    .unwrap();
    let warnings = warnings.take();
    let names: Vec<_> = warnings.iter().map(|x| env_var(x).unwrap()).collect();
    // a key that more than one variable shares has the name up to the key
    assert_eq!(names, ["APP_SERVER__PROT", "APP_SERVR"]);
    assert_eq!(
        warnings[0].message(),
        "unknown field `prot`, expected `port`"
    );
}

#[test]
fn test_update() {
    #[derive(Debug, Deserialize)]
    struct Config {
        name: String,
        server: Server,
    }

    #[derive(Debug, Deserialize)]
    struct Server {
        host: String,
        port: u16,
    }

    let mut config = Config {
        name: "app".into(),
        server: Server {
            host: "localhost".into(),
            port: 80,
        },
    };
    Deserializer::from_vars("APP_", [("APP_SERVER__PORT", "8080")])
        .update(&mut config)
        .unwrap();
    assert_eq!(config.name, "app");
    assert_eq!(config.server.host, "localhost");
    assert_eq!(config.server.port, 8080);
}

#[test]
fn test_from_env() {
    #[derive(Debug, Deserialize)]
    struct Package {
        name: String,
        version: String,
    }

    // cargo sets these for the tests
    let package: Package = deser_env::from_env("CARGO_PKG_").unwrap();
    assert_eq!(package.name, "deser-env");
    assert_eq!(package.version, env!("CARGO_PKG_VERSION"));

    let name: String = deser_env::var("CARGO_PKG_NAME").unwrap();
    assert_eq!(name, "deser-env");
    let missing: Option<u32> = deser_env::var("DESER_ENV_TEST_MISSING").unwrap();
    assert_eq!(missing, None);
    let err = deser_env::var::<u32>("DESER_ENV_TEST_MISSING").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
    assert_eq!(env_var(&err), Some("DESER_ENV_TEST_MISSING"));
    // a variable is a collection of one value, a missing one is empty
    let names: Vec<String> = deser_env::var("CARGO_PKG_NAME").unwrap();
    assert_eq!(names, ["deser-env"]);
    let missing: Vec<u32> = deser_env::var("DESER_ENV_TEST_MISSING").unwrap();
    assert!(missing.is_empty());
    let err = deser_env::var::<u32>("CARGO_PKG_NAME").unwrap_err();
    assert_eq!(err.message(), "invalid value \"deser-env\", expected u32");
    assert_eq!(env_var(&err), Some("CARGO_PKG_NAME"));
}
