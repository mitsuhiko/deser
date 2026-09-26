use std::collections::BTreeMap;

use deser::io::{Reader, Writer};
use deser::{Deserialize, Serialize};
use deser_toml::{DeserializerConfig, SerializerConfig};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    ports: Vec<u16>,
}

#[test]
fn test_roundtrip() {
    let config = Config {
        name: "web".into(),
        ports: vec![80, 443],
    };
    let mut out = Vec::new();
    deser_toml::to_writer(&mut out, &config).unwrap();
    assert_eq!(out, b"name = \"web\"\nports = [80, 443]\n");
    assert_eq!(
        deser_toml::from_reader::<Config, _>(&out[..]).unwrap(),
        config
    );
}

#[test]
fn test_empty_stream_is_an_empty_table() {
    let value: BTreeMap<String, u32> = deser_toml::from_reader(&b""[..]).unwrap();
    assert!(value.is_empty());
}

#[test]
fn test_single_document() {
    let mut reader = Reader::new(&b"a = 1\n"[..], DeserializerConfig::new());
    assert!(reader.read::<BTreeMap<String, u32>>().unwrap().is_some());
    assert!(reader.read::<BTreeMap<String, u32>>().unwrap().is_none());

    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&BTreeMap::from([("a", 1)])).unwrap();
    assert!(writer.write(&BTreeMap::from([("b", 2)])).is_err());
    assert_eq!(writer.into_inner(), b"a = 1\n");
}

#[test]
fn test_errors() {
    let err = deser_toml::from_reader::<Config, _>(&b"name = \"web\"\nports = [80, x]\n"[..])
        .unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(2), Some(14)));
}
