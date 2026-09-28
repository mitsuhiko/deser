use deser::ErrorKind;
use deser::de::Limits;
use deser_transcode::{Transcoder, transcode, transcode_with};

#[test]
fn test_stream() {
    let config = deser_json::DeserializerConfig::new().trailing(deser_json::Trailing::Newline);
    let mut de =
        deser_json::Deserializer::from_str_with_config("{\"a\": 1}\n[1, 2]\n\"x\"\n", &config);
    let mut ser = deser_yaml::Serializer::new();
    let mut transcoder = Transcoder::new();
    while !de.is_end() {
        transcoder.transcode(&mut de, &mut ser).unwrap();
    }
    assert_eq!(ser.finish(), "a: 1\n---\n- 1\n- 2\n---\nx\n");
}

#[test]
fn test_yaml_documents_to_json_lines() {
    let mut de = deser_yaml::Deserializer::from_str("a: 1\n---\nb: [x]\n");
    let mut out = String::new();
    let mut transcoder = Transcoder::new();
    while !de.is_end() {
        let mut ser = deser_json::Serializer::new();
        transcoder.transcode(&mut de, &mut ser).unwrap();
        out.push_str(&ser.finish());
        out.push('\n');
    }
    assert_eq!(out, "{\"a\":1}\n{\"b\":[\"x\"]}\n");
}

#[test]
fn test_empty_input() {
    let mut de = deser_yaml::Deserializer::from_str("");
    let mut ser = deser_json::Serializer::new();
    let err = transcode(&mut de, &mut ser).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
}

#[test]
fn test_layers() {
    let mut de = deser_json::Deserializer::from_str(r#"{"a": [1, 2, 3]}"#);
    let mut ser = deser_json::Serializer::new();
    let err = transcode_with(
        &mut de,
        &mut ser,
        |driver| driver.push_layer(Limits::new().max_items(2)),
        |_| {},
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: too many items at line 1 column 14"
    );

    let mut de = deser_json::Deserializer::from_str(r#"{"a": {"b": 1}}"#);
    let mut ser = deser_json::Serializer::new();
    transcode_with(
        &mut de,
        &mut ser,
        |_| {},
        |driver| driver.push_layer(deser_path::PathLayer::new()),
    )
    .unwrap();
    assert_eq!(ser.finish(), r#"{"a":{"b":1}}"#);
}

#[test]
fn test_reuse_after_error() {
    let mut transcoder = Transcoder::new();
    let mut de = deser_json::Deserializer::from_str("[1,");
    let mut ser = deser_json::Serializer::new();
    assert!(transcoder.transcode(&mut de, &mut ser).is_err());
    let mut de = deser_json::Deserializer::from_str("[2]");
    let mut ser = deser_json::Serializer::new();
    transcoder.transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), "[2]");
}
