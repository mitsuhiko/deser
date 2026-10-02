use deser::{BytesFormat, Context, Deserialize, Serialize};
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
    // the same context is used for writing and reading
    let hex = Context::new().with(BytesFormat::encoded::<Hex>());
    let xml = SerializerConfig::new()
        .to_string_with(&blob(), |driver| driver.set_context(&hex))
        .unwrap();
    assert_eq!(xml, r#"<Blob attr="686921"><data>0001ff</data></Blob>"#);
    assert_eq!(
        Deserializer::from_str(&xml)
            .deserialize_in::<Blob>(&hex)
            .unwrap(),
        blob()
    );
    assert_eq!(
        Deserializer::from_slice(xml.as_bytes())
            .deserialize_in::<Blob>(&hex)
            .unwrap(),
        blob()
    );

    // without it the hex text is not base64
    assert!(from_str::<Blob>(&xml).is_err());
}

#[cfg(feature = "io")]
#[test]
fn test_bytes_config_reader() {
    let hex = Context::new().with(BytesFormat::encoded::<Hex>());
    let xml = SerializerConfig::new()
        .to_string_with(&blob(), |driver| driver.set_context(&hex))
        .unwrap();
    let mut reader = DeserializerConfig::new().reader(xml.as_bytes());
    reader.set_context(hex);
    let value: Blob = reader.read().unwrap().unwrap();
    assert_eq!(value, blob());
}
