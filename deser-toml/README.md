# deser-toml

[TOML 1.1](https://toml.io/en/v1.1.0) support for
[deser](https://github.com/mitsuhiko/deser).  TOML is the format of choice
for configuration files that humans edit, and those humans make mistakes.
This crate is built to tell them exactly where: errors carry line, column
and (with `deser-path`) the path to the value, also for internally tagged
enums whose tag comes after the fields.

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    name: String,
    servers: Vec<Server>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

let config: Config = deser_toml::from_str(r#"
name = "web"

[[servers]]
host = "a.example.com"
port = 8080
"#).unwrap();
assert_eq!(config.servers[0].port, 8080);

// sequences of maps are written as arrays of tables
assert_eq!(
    deser_toml::to_string(&config).unwrap(),
    "name = \"web\"\n\n[[servers]]\nhost = \"a.example.com\"\nport = 8080\n"
);

// errors point at the problem
let err = deser_toml::from_str::<Config>("name = \"web\"\nservers = [{host = \"a\", port = 80800}]")
    .unwrap_err();
assert_eq!((err.line(), err.column()), (Some(2), Some(32)));
```

* A from scratch implementation of TOML 1.1 which passes the
  [toml-test](https://github.com/toml-lang/toml-test) suite.
* Date-times are passed through deser as the well-known `Datetime`
  extension type which falls back to strings.  The date and time types of
  `jiff`, `chrono` and `time` can be used directly with the respective
  features of deser.
* Arbitrarily nested arrays and inline tables do not overflow the stack.
* The serializer writes maps as tables and sequences of maps as arrays of
  tables.  The output is compatible with TOML 1.0.
* Source locations (see `DeserializerConfig::track_locations`).

## Conformance tests

The test data is vendored in `tests/data` and can be updated with
`scripts/update-toml-test-data.sh`.  Run the suite with:

```sh
cargo test -p deser-toml --test toml_test_suite
```

Pass parts of test names to only run some tests and to see the details of
failures, including known ones:

```sh
cargo test -p deser-toml --test toml_test_suite -- valid/string invalid/table
```

Every valid test is also serialized again and has to roundtrip.  Tests that
are expected to fail are listed in
`tests/toml_test_suite_known_failures.txt`.  The run fails if a test fails
that is not listed there or if a listed test passes.  After fixing tests,
update the list with:

```sh
DESER_TOML_BLESS=1 cargo test -p deser-toml --test toml_test_suite
```
