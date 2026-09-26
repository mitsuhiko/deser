use std::collections::BTreeMap;

use deser::adapters::{ByteSeq, EncodedStr};
use deser::bytes::{Base64UrlNoPad, BytesFormat, Hex};
use deser::{Deserialize, Serialize};
use deser_toml::{DeserializerConfig, SerializerConfig, from_str, to_string};

mod common;

use common::Value;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    plain: Vec<u8>,
    array: [u8; 3],
    #[deser(as = Hex)]
    hex: Vec<u8>,
    #[deser(as = EncodedStr<Base64UrlNoPad>)]
    url: Vec<u8>,
    #[deser(as = ByteSeq)]
    seq: Vec<u8>,
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
        keys: [(vec![1], 1)].into_iter().collect(),
        hex_keys: [(vec![2], 2)].into_iter().collect(),
    }
}

#[test]
fn test_bytes_default() {
    let toml = to_string(&blob()).unwrap();
    assert_eq!(
        toml,
        "plain = \"AAH/\"\n\
         array = \"aGkh\"\n\
         hex = \"dead\"\n\
         url = \"-_8\"\n\
         seq = [1, 2]\n\
         \n\
         [keys]\n\
         \"AQ==\" = 1\n\
         \n\
         [hex_keys]\n\
         02 = 2\n"
    );
    assert_eq!(from_str::<Blob>(&toml).unwrap(), blob());
}

#[test]
fn test_bytes_config() {
    const HEX: BytesFormat = BytesFormat::encoded::<Hex>();
    let toml = SerializerConfig::new()
        .bytes(HEX)
        .to_string(&blob())
        .unwrap();
    assert!(toml.starts_with("plain = \"0001ff\"\narray = \"686921\"\n"));
    let value = DeserializerConfig::new()
        .bytes(HEX)
        .from_str::<Blob>(&toml)
        .unwrap();
    assert_eq!(value, blob());

    let toml = SerializerConfig::new()
        .bytes(BytesFormat::SEQ)
        .to_string(&blob())
        .unwrap();
    assert!(toml.starts_with("plain = [0, 1, 255]\narray = [104, 105, 33]\n"));
    assert_eq!(from_str::<Blob>(&toml).unwrap(), blob());
}

/// JSON and TOML represent bytes the same way.
#[test]
fn test_same_as_json() {
    for format in [
        BytesFormat::BASE64,
        BytesFormat::SEQ,
        BytesFormat::encoded::<Hex>(),
    ] {
        let value = blob();
        let toml = SerializerConfig::new()
            .bytes(format)
            .to_string(&value)
            .unwrap();
        let json = deser_json::SerializerConfig::new()
            .bytes(format)
            .to_string(&value)
            .unwrap();

        // convert both into a generic value to compare them
        let from_toml: BTreeMap<String, Value> = deser_toml::DeserializerConfig::new()
            .bytes(format)
            .from_str(&toml)
            .unwrap();
        let from_json: BTreeMap<String, Value> = deser_json::DeserializerConfig::new()
            .bytes(format)
            .from_str(&json)
            .unwrap();
        assert_eq!(from_toml, from_json, "{:?}", format);

        let value_from_toml: Blob = deser_toml::DeserializerConfig::new()
            .bytes(format)
            .from_str(&toml)
            .unwrap();
        let value_from_json: Blob = deser_json::DeserializerConfig::new()
            .bytes(format)
            .from_str(&json)
            .unwrap();
        assert_eq!(value_from_toml, value);
        assert_eq!(value_from_json, value);
    }
}
