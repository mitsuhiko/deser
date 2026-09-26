# deser-yaml

YAML support for deser.

The parser is written from scratch against the official
[YAML test suite](https://github.com/yaml/yaml-test-suite) and the
[YAML schema tests](https://github.com/perlpunk/yaml-test-schema).  The
serializer writes block style YAML that reads back as the same values,
also for YAML 1.1 readers.  The conformance tests serialize every valid
document of the test suite with several configurations and read it back.

## Conformance tests

The test data is vendored in `tests/data` and can be updated with
`scripts/update-yaml-test-data.sh`.  Run the suite with:

```
cargo test -p deser-yaml --test yaml_test_suite
```

Pass test ids or parts of test descriptions to only run some tests and to
see the details of failures, including known ones:

```
cargo test -p deser-yaml --test yaml_test_suite -- 2G84 "spec example 2.4"
```

Tests that are expected to fail are listed in
`tests/yaml_test_suite_known_failures.txt`.  The run fails if a test fails
that is not listed there or if a listed test passes.  After fixing tests,
update the list with:

```
DESER_YAML_BLESS=1 cargo test -p deser-yaml --test yaml_test_suite
```
