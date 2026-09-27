# deser-env

Environment variables for [deser](https://github.com/mitsuhiko/deser).

The variables with a prefix are read as a map, `__` separates nested keys
(a single `_` separates words in names).  Everything in the environment is
text, only the type a value is deserialized into knows what it means.
deser passes the text on as lexical atoms which are parsed by the type they
end up in, also after buffering, so numbers parse in flattened structs and
internally tagged enums:

```rust
use deser::adapters::{Separated, TrimWhitespace};
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    name: String,
    server: Server,
    #[deser(as = Separated<',', TrimWhitespace>)]
    hosts: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Server {
    port: u16,
    max_connections: Option<u32>,
}

// `deser_env::from_env("APP_")` reads the environment of the process
let config: Config = deser_env::from_vars("APP_", [
    ("APP_NAME", "shop"),
    ("APP_SERVER__PORT", "8080"),
    ("APP_HOSTS", "a, b"),
])
.unwrap();
assert_eq!(config.server.port, 8080);
assert_eq!(config.hosts, ["a", "b"]);

let vars = deser_env::to_vars("APP_", &config).unwrap();
assert_eq!(vars[1], ("APP_SERVER__PORT".to_string(), "8080".to_string()));
```

What works:

* Nested structs, maps and enums (`APP_SERVER__PORT`), sequences with
  indexes (`APP_HOSTS__0`) and lists in one variable with the `Separated`
  adapter (`APP_HOSTS=a,b`).
* Names are lowercased (`Case::Preserve` keeps them), the separator can be
  configured.
* Empty values are `None` for optionals of types that do not accept them
  (`APP_PORT=` for an `Option<u16>`).  The `Flag` adapter makes a variable
  that is set but empty `true`.
* Errors (also of unknown fields collected as warnings) carry the name of
  the variable.
* Updating an existing value, for instance to override a configuration
  file.
* Serializing into name-value pairs, for instance for `Command::envs`.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-env)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)
