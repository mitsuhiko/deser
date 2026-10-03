//! Values that are buffered keep what PHP's format has beyond the data
//! model: classes, the visibility of properties and references.
use deser::de::Recording;
use deser_value::Value;

const INPUT: &[u8] = b"a:3:{i:0;O:3:\"Foo\":2:{s:4:\"\0*\0a\";E:7:\"Foo:Bar\";\
s:6:\"\0Foo\0b\";C:3:\"Baz\":2:{xy}}i:1;r:2;i:5;S:1:\"\\61\";}";

/// What `serialize` writes for `INPUT` (`S:` is written as `s:`).
const OUTPUT: &[u8] = b"a:3:{i:0;O:3:\"Foo\":2:{s:4:\"\0*\0a\";E:7:\"Foo:Bar\";\
s:6:\"\0Foo\0b\";C:3:\"Baz\":2:{xy}}i:1;r:2;i:5;s:1:\"a\";}";

#[test]
fn test_value_round_trip() {
    let value: Value = deser_php::from_slice(INPUT).unwrap();
    assert_eq!(deser_php::to_vec(&value).unwrap(), OUTPUT);
}

#[test]
fn test_recording_round_trip() {
    let recording: Recording = deser_php::from_slice(INPUT).unwrap();
    assert_eq!(deser_php::to_vec(&recording).unwrap(), OUTPUT);
}

#[test]
fn test_to_json() {
    // classes are dropped, references are their number and integer keys
    // are strings
    let value: Value = deser_php::from_slice(INPUT).unwrap();
    assert_eq!(
        deser_json::to_string(&value).unwrap(),
        r#"{"0":{"a":"Bar","b":"eHk="},"1":2,"5":"a"}"#
    );
}

#[test]
fn test_from_json() {
    let value: Value = deser_json::from_str(r#"{"a": [1, 2.5, null], "7": {"b": true}}"#).unwrap();
    assert_eq!(
        deser_php::to_vec(&value).unwrap(),
        b"a:2:{s:1:\"a\";a:3:{i:0;i:1;i:1;d:2.5;i:2;N;}i:7;a:1:{s:1:\"b\";b:1;}}"
    );
}

#[test]
fn test_empty_arrays() {
    // buffered values remember that the empty array can be a map
    #[derive(Debug, PartialEq, deser::Deserialize)]
    struct Settings {
        tags: Vec<String>,
        options: std::collections::BTreeMap<String, String>,
    }
    let input = br#"a:2:{s:4:"tags";a:0:{}s:7:"options";a:0:{}}"#;
    let value: Value = deser_php::from_slice(input).unwrap();
    let settings: Settings = deser_value::from_value(&value).unwrap();
    assert!(settings.tags.is_empty() && settings.options.is_empty());
    let recording: Recording = deser_php::from_slice(input).unwrap();
    let mut out = None::<Settings>;
    let mut state = deser::State::new();
    let sink = <Settings as deser::Deserialize>::deserialize_into(&mut out, &mut state);
    recording.replay(sink, &mut state).unwrap();
    let settings = out.unwrap();
    assert!(settings.tags.is_empty() && settings.options.is_empty());
}
