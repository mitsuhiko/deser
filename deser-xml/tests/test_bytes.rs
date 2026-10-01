use deser::{BytesFormat, Deserialize, Serialize};
use deser_encoding::Hex;
use deser_xml::{Deserializer, DeserializerConfig, SerializerConfig, from_str, to_string};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    #[deser(rename = "@attr")]
    attr: Vec<u8>,
    data: Vec<u8>,
}

fn blob() -> Blob {
    Blob {
        attr: b"hi!".to_vec(),
        data: vec![0, 1, 255],
    }
}

#[test]
fn test_bytes_default() {
    let xml = to_string(&blob()).unwrap();
    assert_eq!(xml, r#"<Blob attr="aGkh"><data>AAH/</data></Blob>"#);
    assert_eq!(from_str::<Blob>(&xml).unwrap(), blob());
}

#[test]
fn test_bytes_config() {
    const HEX: BytesFormat = BytesFormat::encoded::<Hex>();
    let xml = SerializerConfig::new()
        .bytes(HEX)
        .to_string(&blob())
        .unwrap();
    assert_eq!(xml, r#"<Blob attr="686921"><data>0001ff</data></Blob>"#);
    let config = DeserializerConfig::new().bytes(HEX);
    assert_eq!(config.from_str::<Blob>(&xml).unwrap(), blob());
    assert_eq!(config.from_slice::<Blob>(xml.as_bytes()).unwrap(), blob());

    // the deserializer keeps the configuration
    let mut de = Deserializer::from_str_with_config(&xml, &config);
    assert_eq!(de.config(), &config);
    assert_eq!(de.deserialize::<Blob>().unwrap(), blob());

    // without it the hex text is not base64
    assert!(from_str::<Blob>(&xml).is_err());
}

#[cfg(feature = "io")]
#[test]
fn test_bytes_config_reader() {
    const HEX: BytesFormat = BytesFormat::encoded::<Hex>();
    let xml = SerializerConfig::new()
        .bytes(HEX)
        .to_string(&blob())
        .unwrap();
    let value: Blob = DeserializerConfig::new()
        .bytes(HEX)
        .from_reader(xml.as_bytes())
        .unwrap();
    assert_eq!(value, blob());
}
