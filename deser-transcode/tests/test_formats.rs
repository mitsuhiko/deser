use deser::ErrorKind;
use deser_transcode::transcode;

fn json_to_yaml(input: &str) -> String {
    let mut de = deser_json::Deserializer::from_str(input);
    let mut ser = deser_yaml::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    ser.finish()
}

fn yaml_to_json(input: &str) -> String {
    let mut de = deser_yaml::Deserializer::from_str(input);
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    ser.finish()
}

fn json_to_toml(input: &str) -> Result<String, deser::Error> {
    let mut de = deser_json::Deserializer::from_str(input);
    let mut ser = deser_toml::Serializer::new();
    transcode(&mut de, &mut ser)?;
    Ok(ser.finish())
}

fn json_to_cbor(input: &str) -> Vec<u8> {
    let mut de = deser_json::Deserializer::from_str(input);
    let mut ser = deser_cbor::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    ser.finish()
}

#[test]
fn test_json_to_yaml() {
    assert_eq!(
        json_to_yaml(r#"{"a": [1, 2.5, "x"], "b": {"c": null, "d": true}}"#),
        "a:\n  - 1\n  - 2.5\n  - x\nb:\n  c: null\n  d: true\n"
    );
    // strings that would read back as other types are quoted
    assert_eq!(
        json_to_yaml(r#"["1", "true", "null"]"#),
        "- '1'\n- 'true'\n- 'null'\n"
    );
    assert_eq!(json_to_yaml("42"), "42\n");
}

#[test]
fn test_yaml_to_json() {
    // implicit values are written as the types they were inferred as, keys
    // that are not strings are written the way JSON writes them
    assert_eq!(
        yaml_to_json("1: a\ntrue: false\nx: 1.0\ny: ~\nz: '2'\n"),
        r#"{"1":"a","true":false,"x":1.0,"y":null,"z":"2"}"#
    );
    // binary is base64 in JSON
    assert_eq!(yaml_to_json("!!binary aGVsbG8="), r#""aGVsbG8=""#);
}

#[test]
fn test_yaml_keys_that_json_cannot_express() {
    let mut de = deser_yaml::Deserializer::from_str("? [1, 2]\n: x\n");
    let mut ser = deser_json::Serializer::new();
    let err = transcode(&mut de, &mut ser).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
}

#[test]
fn test_json_to_toml() {
    // entries with null values are skipped
    assert_eq!(
        json_to_toml(r#"{"a": null, "b": {"c": [1, 2]}, "d": "x"}"#).unwrap(),
        "d = \"x\"\n\n[b]\nc = [1, 2]\n"
    );
    let err = json_to_toml("[1]").unwrap_err();
    assert_eq!(
        err.to_string(),
        "UnsupportedType: TOML documents must be tables"
    );
    let err = json_to_toml(r#"{"a": [null]}"#).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
}

#[test]
fn test_toml_to_json() {
    let mut de =
        deser_toml::Deserializer::from_str("a = 1979-05-27T07:32:00Z\nb = 1.5\n[t]\nc = [1, 2]\n");
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(
        ser.finish(),
        r#"{"a":"1979-05-27T07:32:00Z","b":1.5,"t":{"c":[1,2]}}"#
    );
}

#[test]
fn test_json_to_cbor() {
    // the lengths are known when the containers are written, the output is
    // the same as for a value that knows its lengths
    let input = r#"{"a": [1, 2, {"b": "c"}], "d": "x", "e": []}"#;
    let value: deser_value::Value = deser_json::from_str(input).unwrap();
    let expected = deser_cbor::to_vec(&value).unwrap();
    assert_eq!(json_to_cbor(input), expected);
    assert_eq!(expected[0], 0xa3);

    let long = format!("[{}]", vec!["0"; 1000].join(","));
    let cbor = json_to_cbor(&long);
    // an array with a two byte length
    assert_eq!(&cbor[..3], &[0x99, 0x03, 0xe8]);
}

#[test]
fn test_cbor_round_trip() {
    // tags and bytes survive
    let input = [
        0xa2, 0x61, 0x61, 0x42, 0x68, 0x69, 0x61, 0x62, 0xc1, 0x1a, 0x51, 0x4b, 0x67, 0xb0,
    ];
    let mut de = deser_cbor::Deserializer::from_slice(&input);
    let mut ser = deser_cbor::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), input);

    let mut de = deser_cbor::Deserializer::from_slice(&input);
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), r#"{"a":"aGk=","b":1363896240}"#);
}

#[test]
fn test_cbor_indefinite_lengths() {
    // indefinite lengths become known lengths
    let input = [0x9f, 0x01, 0xbf, 0x61, 0x61, 0x02, 0xff, 0xff];
    let mut de = deser_cbor::Deserializer::from_slice(&input);
    let mut ser = deser_cbor::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), [0x82, 0x01, 0xa1, 0x61, 0x61, 0x02]);
}

#[test]
fn test_json_to_msgpack() {
    let input = r#"{"a": [1, -2, 1.5], "b": null}"#;
    let mut de = deser_json::Deserializer::from_str(input);
    let mut ser = deser_msgpack::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    let out = ser.finish();
    let value: deser_value::Value = deser_json::from_str(input).unwrap();
    assert_eq!(out, deser_msgpack::to_vec(&value).unwrap());

    let mut de = deser_msgpack::Deserializer::from_slice(&out);
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), r#"{"a":[1,-2,1.5],"b":null}"#);
}

#[test]
fn test_xml_to_json() {
    // elements are multimaps, repeated elements are repeated keys
    let mut de = deser_xml::Deserializer::from_str(r#"<r a="1"><b>x</b><b>y</b><c>2</c></r>"#);
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), r#"{"@a":"1","b":"x","b":"y","c":"2"}"#);
}

#[test]
fn test_query_string_to_json() {
    // the text of query strings stays text
    let mut de = deser_urlencoded::Deserializer::from_str("a=1&b=x&c[]=1&c[]=2");
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), r#"{"a":"1","b":"x","c":["1","2"]}"#);
}

#[test]
fn test_value() {
    // values are a format too
    let value = deser_value::to_value(&vec![1, 2]).unwrap();
    let mut de = deser_value::Deserializer::new(&value);
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), "[1,2]");

    let mut de = deser_json::Deserializer::from_str(r#"{"a": [true]}"#);
    let mut ser = deser_value::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), [deser_value::value!({"a": [true]})]);
}

#[test]
fn test_parse_errors_are_located() {
    let mut de = deser_json::Deserializer::from_str("{\"a\": [1,\n 2,, 3]}");
    let mut ser = deser_yaml::Serializer::new();
    let err = transcode(&mut de, &mut ser).unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(2), Some(4)));
}

#[test]
fn test_deep_nesting() {
    // nothing recurses and the recording is serialized in linear time
    let depth = if cfg!(miri) { 100 } else { 200_000 };
    let input = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    let mut de = deser_json::Deserializer::from_str(&input);
    let mut ser = deser_json::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    assert_eq!(ser.finish(), input);

    let cbor = json_to_cbor(&input);
    assert_eq!(cbor.len(), depth);
    assert!(cbor[..depth - 1].iter().all(|&b| b == 0x81));
    assert_eq!(cbor[depth - 1], 0x80);
}

fn to_xml<'de>(de: &mut impl deser::de::Deserializer<'de>) -> Result<String, deser::Error> {
    let mut ser = deser_xml::Serializer::with_config(&deser_xml::SerializerConfig::new().root("r"));
    transcode(de, &mut ser)?;
    Ok(ser.finish())
}

#[test]
fn test_json_to_xml() {
    // keys with the attribute prefix are attributes, sequences are
    // repeated elements, nulls are left out
    let mut de = deser_json::Deserializer::from_str(
        r#"{"@id": 1, "item": [{"@n": 1, "$text": "a"}, "b"], "none": null, "x": true}"#,
    );
    assert_eq!(
        to_xml(&mut de).unwrap(),
        r#"<r id="1"><item n="1">a</item><item>b</item><x>true</x></r>"#
    );

    // values without a name need the configured one
    let mut de = deser_json::Deserializer::from_str(r#"{"a": 1}"#);
    let mut ser = deser_xml::Serializer::new();
    let err = transcode(&mut de, &mut ser).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);

    // what XML cannot express is an error
    let mut de = deser_json::Deserializer::from_str(r#"{"a": [[1]]}"#);
    assert_eq!(
        to_xml(&mut de).unwrap_err().kind(),
        ErrorKind::UnsupportedType
    );
}

#[test]
fn test_xml_to_xml() {
    // the root element (its name and namespaces) is kept, the configured
    // name is only for values without one
    let input = r#"<feed xmlns="urn:atom" a="1"><b>x</b><c>2</c><b>y</b></feed>"#;
    let mut de = deser_xml::Deserializer::from_str(input);
    assert_eq!(to_xml(&mut de).unwrap(), input);
}

#[test]
fn test_yaml_to_xml() {
    let mut de =
        deser_yaml::Deserializer::from_str("title: T\nentry:\n  - '@id': 1\n  - '@id': 2\n");
    assert_eq!(
        to_xml(&mut de).unwrap(),
        r#"<r><title>T</title><entry id="1"/><entry id="2"/></r>"#
    );
}

#[test]
fn test_csv_to_msgpack_and_cbor() {
    // CSV only estimates the number of records (from the first one, which
    // is shorter here), the binary formats write the counted length
    let input = "a,b\n1,x\n22,yyyyyy\n333,zzzzzzzzzzzz\n";
    let value: deser_value::Value = deser_csv::from_str(input).unwrap();

    let mut de = deser_csv::Deserializer::from_str(input);
    let mut ser = deser_msgpack::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    let msgpack = ser.finish();
    assert_eq!(msgpack[0], 0x93);
    assert_eq!(msgpack, deser_msgpack::to_vec(&value).unwrap());

    let mut de = deser_csv::Deserializer::from_str(input);
    let mut ser = deser_cbor::Serializer::new();
    transcode(&mut de, &mut ser).unwrap();
    let cbor = ser.finish();
    assert_eq!(cbor[0], 0x83);
    assert_eq!(cbor, deser_cbor::to_vec(&value).unwrap());
}
