use std::collections::BTreeMap;

use deser::adapters::bytes::{Base64UrlNoPad, BytesFallback, BytesFormat, Hex, IntSeq};
use deser::{Deserialize, Serialize};
use deser_json::{DeserializerConfig, SerializerConfig, from_str, to_string};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    plain: Vec<u8>,
    array: [u8; 3],
    #[deser(as = BytesFallback<Hex>)]
    hex: Vec<u8>,
    #[deser(as = Base64UrlNoPad)]
    url: Vec<u8>,
    #[deser(as = BytesFallback<IntSeq>)]
    seq: Vec<u8>,
    #[deser(as = Option<Hex>)]
    missing: Option<Vec<u8>>,
    keys: BTreeMap<Vec<u8>, u32>,
    #[deser(as = BTreeMap<Hex, _>)]
    hex_keys: BTreeMap<Vec<u8>, u32>,
}

fn blob() -> Blob {
    Blob {
        plain: vec![0, 1, 255],
        array: [104, 105, 33],
        hex: vec![0xde, 0xad],
        url: vec![0xfb, 0xff],
        seq: vec![1, 2],
        missing: None,
        keys: [(vec![1], 1)].into_iter().collect(),
        hex_keys: [(vec![2], 2)].into_iter().collect(),
    }
}

#[test]
fn test_bytes_default() {
    assert_eq!(to_string(&b"".to_vec()).unwrap(), r#""""#);
    assert_eq!(to_string(&b"foobar".to_vec()).unwrap(), r#""Zm9vYmFy""#);
    assert_eq!(to_string(&&b"\xfb\xff"[..]).unwrap(), r#""+/8=""#);

    let json = to_string(&blob()).unwrap();
    assert_eq!(
        json,
        r#"{"plain":"AAH/","array":"aGkh","hex":"dead","url":"-_8","seq":[1,2],"missing":null,"keys":{"AQ==":1},"hex_keys":{"02":2}}"#
    );
    assert_eq!(from_str::<Blob>(&json).unwrap(), blob());
}

#[test]
fn test_bytes_lenient() {
    for json in [
        r#""+/8=""#,
        r#""-_8=""#,
        r#""+/8""#,
        r#""-_8""#,
        "[251,255]",
    ] {
        assert_eq!(from_str::<Vec<u8>>(json).unwrap(), [251, 255], "{}", json);
    }
    let err = from_str::<Vec<u8>>(r#"[1, "A"]"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected string, expected u8 at line 1 column 5"
    );
    let err = from_str::<Vec<u8>>(r#""A""#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: invalid base64 string at line 1 column 1"
    );
    let err = from_str::<[u8; 4]>(r#""AAH/""#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "WrongLength: byte array of wrong length at line 1 column 1"
    );
}

#[test]
fn test_bytes_config() {
    const SEQ: SerializerConfig = SerializerConfig::new().bytes(BytesFormat::SEQ);
    let json = SEQ.to_string(&blob()).unwrap();
    // values with adapters keep their format, keys cannot be sequences
    assert_eq!(
        json,
        r#"{"plain":[0,1,255],"array":[104,105,33],"hex":"dead","url":"-_8","seq":[1,2],"missing":null,"keys":{"AQ==":1},"hex_keys":{"02":2}}"#
    );
    assert_eq!(from_str::<Blob>(&json).unwrap(), blob());

    const HEX: BytesFormat = BytesFormat::encoded::<Hex>();
    let json = SerializerConfig::new()
        .bytes(HEX)
        .to_string(&blob())
        .unwrap();
    assert_eq!(
        json,
        r#"{"plain":"0001ff","array":"686921","hex":"dead","url":"-_8","seq":[1,2],"missing":null,"keys":{"01":1},"hex_keys":{"02":2}}"#
    );
    let value = DeserializerConfig::new()
        .bytes(HEX)
        .from_str::<Blob>(&json)
        .unwrap();
    assert_eq!(value, blob());

    // the default configuration decodes base64, hex is often valid base64
    assert!(from_str::<Vec<u8>>(r#""0001ff""#).is_err());
    assert_eq!(from_str::<Vec<u8>>(r#""deadbeef""#).unwrap().len(), 6);
}

#[test]
fn test_bytes_in_enums() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(tag = "type")]
    enum Message {
        Data { payload: Vec<u8> },
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(untagged)]
    enum Content {
        Number(u32),
        Bytes(Vec<u8>),
    }

    let value = Message::Data {
        payload: vec![1, 2, 3],
    };
    let json = to_string(&value).unwrap();
    assert_eq!(json, r#"{"type":"Data","payload":"AQID"}"#);
    // internally tagged enums buffer the payload
    let json = r#"{"payload":"AQID","type":"Data"}"#;
    assert_eq!(from_str::<Message>(json).unwrap(), value);

    const HEX: DeserializerConfig = DeserializerConfig::new().bytes(BytesFormat::encoded::<Hex>());
    let json = r#"{"payload":"010203","type":"Data"}"#;
    assert_eq!(HEX.from_str::<Message>(json).unwrap(), value);

    assert_eq!(
        from_str::<Content>(r#""AQID""#).unwrap(),
        Content::Bytes(vec![1, 2, 3])
    );
}
