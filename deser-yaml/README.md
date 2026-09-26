# deser-yaml

[YAML](https://yaml.org/) support for
[deser](https://github.com/mitsuhiko/deser).  YAML is full of surprises
(is `no` a string or a boolean?  is `1.10` a version or a float?).  This
crate aims to be correct rather than clever: it passes the official test
suite, lets you pick YAML 1.1 or 1.2 semantics, and the serializer quotes
exactly what needs quoting so that what you write is what other readers
(including PyYAML) read back.

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    name: String,
    version: String,
    ports: Vec<u16>,
    debug: bool,
}

let config: Config = deser_yaml::from_str("
name: web
version: '1.10'
ports: [80, 443]
debug: false
").unwrap();
assert_eq!(config.version, "1.10");

// strings that look like other types are quoted
assert_eq!(
    deser_yaml::to_string(&config).unwrap(),
    "name: web\nversion: '1.10'\nports:\n  - 80\n  - 443\ndebug: false\n"
);

// errors point at the problem
let err = deser_yaml::from_str::<Config>("name: web\nversion: '1'\nports: [80, https]\ndebug: no")
    .unwrap_err();
assert_eq!((err.line(), err.column()), (Some(3), Some(13)));
```

Features:

* Anchors and aliases (with a limit against alias bombs), tags,
  `!!binary` bytes, `!!timestamp` date-times and multi-document streams.
* Configurable output: indentation, quoting, flow or block collections
  (also per value through hints), literal block scalars.
* Source locations in errors and through `deser_location::Spanned`.
* Deeply nested input does not overflow the stack.

## Conformance

The parser is written from scratch against the official
[YAML test suite](https://github.com/yaml/yaml-test-suite) and the
[YAML schema tests](https://github.com/perlpunk/yaml-test-schema).  The
serializer writes block style YAML that reads back as the same values,
also for YAML 1.1 readers.  The conformance tests serialize every valid
document of the test suite with several configurations and read it back.

### Running the Tests

The test data is vendored in `tests/data` and can be updated with
`scripts/update-yaml-test-data.sh`.  Run the suite with:

```sh
cargo test -p deser-yaml --test yaml_test_suite
```

Pass test ids or parts of test descriptions to only run some tests and to
see the details of failures, including known ones:

```sh
cargo test -p deser-yaml --test yaml_test_suite -- 2G84 "spec example 2.4"
```

Tests that are expected to fail are listed in
`tests/yaml_test_suite_known_failures.txt`.  The run fails if a test fails
that is not listed there or if a listed test passes.  After fixing tests,
update the list with:

```sh
DESER_YAML_BLESS=1 cargo test -p deser-yaml --test yaml_test_suite
```
