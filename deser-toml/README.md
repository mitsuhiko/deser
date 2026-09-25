# deser-toml

[TOML 1.1](https://toml.io/en/v1.1.0) support for deser.

```rust
use std::collections::BTreeMap;

let value: BTreeMap<String, Vec<u32>> = deser_toml::from_str("ports = [80, 443]").unwrap();
assert_eq!(value["ports"], [80, 443]);
assert_eq!(deser_toml::to_string(&value).unwrap(), "ports = [80, 443]\n");
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
* Source locations (see `Deserializer::track_locations`).

## Conformance tests

The test data is vendored in `tests/data` and can be updated with
`scripts/update-toml-test-data.sh`.  Run the suite with:

```
cargo test -p deser-toml --test toml_test_suite
```

Pass parts of test names to only run some tests and to see the details of
failures, including known ones:

```
cargo test -p deser-toml --test toml_test_suite -- valid/string invalid/table
```

Every valid test is also serialized again and has to roundtrip.  Tests that
are expected to fail are listed in
`tests/toml_test_suite_known_failures.txt`.  The run fails if a test fails
that is not listed there or if a listed test passes.  After fixing tests,
update the list with:

```
DESER_TOML_BLESS=1 cargo test -p deser-toml --test toml_test_suite
```
