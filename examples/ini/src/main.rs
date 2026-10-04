//! INI files with `deser-ini`.
//!
//! INI has no specification, every reader understands it a little
//! differently.  The default of `deser-ini` reads what is common today and
//! presets read the files of Python's `configparser` and git's config files:
//!
//! * an application config with comments after values, quoted values, keys
//!   without values, repeated keys, values on continuation lines and a
//!   section that picks an enum variant,
//! * errors with the path of the value and its line and column,
//! * `setup.cfg` with the `configparser` preset,
//! * `.gitconfig` with git's syntax (subsections, quoting and escapes),
//! * writing values as INI files.
use std::collections::BTreeMap;

use deser::adapters::{Flag, Separated};
use deser::{Deserialize, Serialize};
use deser_ini::{DeserializerConfig, SerializerConfig};
use deser_path::{Path, PathLayer};

const APP_INI: &str = r#"
; keys before the first section belong to the file
name = shop
greeting = "  Welcome! "   ; quoted to keep the spaces

[server]
host = 0.0.0.0
port = 8080
# keys without value are switches
reuse_port
allowed_origins =
    https://shop.example.com
    https://admin.example.com

[database]
kind = postgres
host = db.internal ; the primary
pool = 16

[features]
flag = search
flag = recommendations
"#;

#[derive(Debug, Serialize, Deserialize)]
pub struct App {
    name: String,
    greeting: String,
    server: Server,
    database: Database,
    features: Features,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    host: String,
    port: u16,
    /// given without value, missing means `false`
    #[deser(as = Flag)]
    reuse_port: bool,
    /// one origin per continuation line
    #[deser(as = Separated<'\n'>, default)]
    allowed_origins: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "kind", rename_all = "lowercase")]
pub enum Database {
    Postgres { host: String, pool: u32 },
    Sqlite { path: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Features {
    /// the key is repeated for every flag
    flag: Vec<String>,
}

const SETUP_CFG: &str = "
[metadata]
name = deser-demo
version = 1.0

[options]
python_requires = >=3.9
install_requires =
    requests>=2.0
    # comments in continuation lines are skipped
    rich ; python_version >= '3.10'
";

#[derive(Debug, Deserialize)]
pub struct SetupCfg {
    metadata: Metadata,
    options: Options,
}

#[derive(Debug, Deserialize)]
pub struct Metadata {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
pub struct Options {
    python_requires: String,
    #[deser(as = Separated<'\n'>)]
    install_requires: Vec<String>,
}

const GITCONFIG: &str = r#"
[User]
	name = Jane Doe
	email = jane@example.com
[alias]
	lg = "log --graph --oneline" # a comment
	ignored = !git ls-files -v | grep "^[a-z]"
[remote "origin"]
	url = git@github.com:jane/shop.git
	fetch = +refs/heads/*:refs/remotes/origin/*
[branch "main"]
	remote = origin
	merge = refs/heads/main
"#;

#[derive(Debug, Serialize, Deserialize)]
pub struct GitConfig {
    user: User,
    #[deser(default)]
    alias: BTreeMap<String, String>,
    /// `[remote "origin"]` is a subsection: `remote.origin`
    #[deser(default)]
    remote: BTreeMap<String, Remote>,
    #[deser(default)]
    branch: BTreeMap<String, Branch>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    name: String,
    email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Remote {
    url: String,
    /// git repeats `fetch` for every refspec
    fetch: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Branch {
    remote: String,
    merge: String,
}

fn main() {
    // an application config in the default dialect
    let app: App = deser_ini::from_str(APP_INI).unwrap();
    println!("{:#?}", app);
    assert_eq!(app.greeting, "  Welcome! ");
    assert_eq!(app.server.port, 8080);
    assert!(app.server.reuse_port);
    assert_eq!(app.server.allowed_origins.len(), 2);
    assert!(matches!(app.database, Database::Postgres { pool: 16, .. }));
    assert_eq!(app.features.flag, ["search", "recommendations"]);

    // writing it back: keys before sections, quotes and continuation
    // lines where needed, repeated keys for lists
    let written = deser_ini::to_string(&app).unwrap();
    println!("\n{}", written);
    let reread: App = deser_ini::from_str(&written).unwrap();
    assert_eq!(reread.greeting, app.greeting);
    assert_eq!(reread.server.allowed_origins, app.server.allowed_origins);

    // errors carry the path of the value and where it is
    let broken = APP_INI.replace("pool = 16", "pool = sixteen");
    let err = deser_ini::Deserializer::from_str(&broken)
        .deserialize_with::<App, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    println!("error: {}", err);
    assert_eq!(
        err.attachment::<Path>().unwrap().to_string(),
        "database.pool"
    );
    assert_eq!(err.line(), Some(18));

    // `setup.cfg` is read with Python's configparser: no comments after
    // values (the `;` is part of the requirement) and no quotes
    let setup: SetupCfg = DeserializerConfig::python().from_str(SETUP_CFG).unwrap();
    println!("\n{:#?}", setup);
    assert_eq!(setup.metadata.name, "deser-demo");
    assert_eq!(setup.metadata.version, "1.0");
    assert_eq!(setup.options.python_requires, ">=3.9");
    assert_eq!(
        setup.options.install_requires,
        ["requests>=2.0", "rich ; python_version >= '3.10'"]
    );

    // `.gitconfig` with git's syntax: names are lowercased, subsections
    // are nested maps and values can be quoted in parts
    let mut git: GitConfig = DeserializerConfig::git().from_str(GITCONFIG).unwrap();
    println!("\n{:#?}", git);
    assert_eq!(git.user.name, "Jane Doe");
    assert_eq!(git.alias["lg"], "log --graph --oneline");
    assert_eq!(git.alias["ignored"], "!git ls-files -v | grep ^[a-z]");
    assert_eq!(git.remote["origin"].fetch.len(), 1);
    assert_eq!(git.branch["main"].remote, "origin");

    // add an alias with characters that git needs quoted and write the
    // file again
    git.alias
        .insert("todo".into(), "!git grep -n \"TODO\" # find work".into());
    let written = SerializerConfig::git().to_string(&git).unwrap();
    println!("{}", written);
    assert!(written.contains("\ttodo = \"!git grep -n \\\"TODO\\\" # find work\"\n"));
    let reread: GitConfig = DeserializerConfig::git().from_str(&written).unwrap();
    assert_eq!(reread.alias["todo"], git.alias["todo"]);
}
