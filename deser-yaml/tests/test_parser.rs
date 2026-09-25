//! Parser tests for cases that are not covered by the YAML test suite.
use deser_yaml::__private::parse_to_test_events;

#[track_caller]
fn assert_events(input: &str, expected: &str) {
    let (events, err) = parse_to_test_events(input);
    assert_eq!(err, None, "unexpected error for {:?}", input);
    assert_eq!(events, expected, "wrong events for {:?}", input);
}

#[track_caller]
fn assert_error(input: &str, expected: &str) {
    let (_, err) = parse_to_test_events(input);
    let err = err.unwrap_or_else(|| panic!("expected an error for {:?}", input));
    assert!(
        err.contains(expected),
        "unexpected error for {:?}: {}",
        input,
        err
    );
}

#[test]
fn test_empty_block_scalar_before_less_indented_key() {
    assert_events(
        "a: |\n   \nb: c",
        "+STR\n+DOC\n+MAP\n=VAL :a\n=VAL |\n=VAL :b\n=VAL :c\n-MAP\n-DOC\n-STR\n",
    );
    assert_events(
        "a: |+\n   \nb: c",
        "+STR\n+DOC\n+MAP\n=VAL :a\n=VAL |\\n\n=VAL :b\n=VAL :c\n-MAP\n-DOC\n-STR\n",
    );
}

#[test]
fn test_block_scalar_without_final_line_break() {
    assert_events("|\n  x", "+STR\n+DOC\n=VAL |x\\n\n-DOC\n-STR\n");
    assert_events("|-\n  x", "+STR\n+DOC\n=VAL |x\n-DOC\n-STR\n");
}

#[test]
fn test_quoted_scalar_continuation_indentation() {
    assert_error("key: \"a\n\"", "invalid indentation");
    assert_events(
        "key: \"a\n \"",
        "+STR\n+DOC\n+MAP\n=VAL :key\n=VAL \"a \n-MAP\n-DOC\n-STR\n",
    );
}

#[test]
fn test_flow_pairs() {
    assert_events(
        "[a: 1, : x]",
        "+STR\n+DOC\n+SEQ []\n+MAP {}\n=VAL :a\n=VAL :1\n-MAP\n+MAP {}\n=VAL :\n=VAL :x\n-MAP\n-SEQ\n-DOC\n-STR\n",
    );
}

#[test]
fn test_implicit_key_length_limit() {
    let key = "a".repeat(1025);
    assert_error(&format!("{}: b", key), "mapping values are not allowed");
    let key = "a".repeat(1000);
    assert_events(
        &format!("{}: b", key),
        &format!(
            "+STR\n+DOC\n+MAP\n=VAL :{}\n=VAL :b\n-MAP\n-DOC\n-STR\n",
            key
        ),
    );
}

#[test]
fn test_deep_nesting() {
    // nesting does not use the native stack
    let depth = 100_000;
    let input = "[".repeat(depth) + &"]".repeat(depth);
    let (events, err) = parse_to_test_events(&input);
    assert_eq!(err, None);
    assert_eq!(events.lines().count(), depth * 2 + 4);

    let input = "{".repeat(depth) + &"}".repeat(depth) + ": x";
    assert_error(&input, "mapping values are not allowed");
}
