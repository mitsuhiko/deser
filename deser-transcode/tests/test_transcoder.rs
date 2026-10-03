use deser::ErrorKind;
use deser::de::Limits;
use deser_transcode::{Transcoder, transcode, transcode_with};

#[test]
fn test_stream() {
    let config = deser_json::DeserializerConfig::builder()
        .trailing(deser_json::Trailing::Newline)
        .build();
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
fn test_io_streams() {
    use std::io::Read;

    /// A reader that returns the input in chunks of a few bytes.
    struct Chunked<'a>(&'a [u8]);

    impl Read for Chunked<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let len = buf.len().min(self.0.len()).min(3);
            buf[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        }
    }

    // JSON Lines (framed) from a reader into YAML documents on a writer
    let config = deser_json::DeserializerConfig::builder()
        .trailing(deser_json::Trailing::Newline)
        .build();
    let mut de = config.reader(Chunked(b"{\"a\": \"x\"}\n[1, 2]\n"));
    let mut ser = deser_yaml::SerializerConfig::new().writer(Vec::new());
    let mut transcoder = Transcoder::new();
    while !de.is_end().unwrap() {
        transcoder.transcode(&mut de, &mut ser).unwrap();
    }
    assert_eq!(ser.into_inner(), b"a: x\n---\n- 1\n- 2\n");

    // a CBOR sequence (fed) into JSON Lines
    let mut cbor = deser_cbor::Serializer::new();
    cbor.serialize(&vec!["a", "b"]).unwrap();
    cbor.serialize(&42u32).unwrap();
    let cbor = cbor.finish();
    let mut de = deser_cbor::DeserializerConfig::new().reader(Chunked(&cbor));
    let lines = deser_json::SerializerConfig::builder()
        .trailing(deser_json::Trailing::Newline)
        .build();
    let mut ser = lines.writer(Vec::new());
    while !de.is_end().unwrap() {
        transcoder.transcode(&mut de, &mut ser).unwrap();
    }
    assert_eq!(ser.into_inner(), b"[\"a\",\"b\"]\n42\n");
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
        |driver| driver.set_context(deser::Context::with(Limits::builder().max_items(2).build())),
        |_| {},
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "LimitExceeded: too many items at line 1 column 14"
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
